use std::path::Path;

use anyhow::{Context, Result};
use gpui::{
    AnyWindowHandle, App, Bounds, Pixels, Point, Size, WindowBounds, WindowOptions, point, px, size,
};

/// Opens a URL in the user's default browser.
pub(crate) fn open_url(url: &str) -> Result<()> {
    open::that(url).with_context(|| format!("failed to open url: {url}"))
}

/// Opens a file or directory with the system's default application.
pub(crate) fn open_path(path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    open::that(path).with_context(|| format!("failed to open path: {}", path.display()))
}

/// Opens the README.md file in the current working directory.
pub(crate) fn open_documentation() -> Result<()> {
    open_path("README.md")
}

#[derive(Clone, Copy)]
pub(crate) enum AuxiliaryWindowPlacement {
    Centered,
    Near {
        position: Point<Pixels>,
        offset_x: Pixels,
        offset_y: Pixels,
    },
}

#[derive(Clone, Copy)]
pub(crate) struct AuxiliaryWindowSpec {
    preferred_size: Size<Pixels>,
    min_size: Option<Size<Pixels>>,
    max_width_ratio: f32,
    max_height_ratio: f32,
    resizable: bool,
    placement: AuxiliaryWindowPlacement,
}

impl AuxiliaryWindowSpec {
    pub(crate) fn new(preferred_size: Size<Pixels>) -> Self {
        Self {
            preferred_size,
            min_size: None,
            max_width_ratio: 0.9,
            max_height_ratio: 0.9,
            resizable: true,
            placement: AuxiliaryWindowPlacement::Centered,
        }
    }

    pub(crate) fn with_min_size(mut self, min_size: Size<Pixels>) -> Self {
        self.min_size = Some(min_size);
        self
    }

    pub(crate) fn with_max_ratio(mut self, width: f32, height: f32) -> Self {
        self.max_width_ratio = width;
        self.max_height_ratio = height;
        self
    }

    pub(crate) fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    pub(crate) fn near(
        mut self,
        position: Point<Pixels>,
        offset_x: Pixels,
        offset_y: Pixels,
    ) -> Self {
        self.placement = AuxiliaryWindowPlacement::Near {
            position,
            offset_x,
            offset_y,
        };
        self
    }
}

fn window_bounds_for_handle(handle: AnyWindowHandle, cx: &mut App) -> Option<Bounds<Pixels>> {
    handle
        .update(cx, |_, window, _| match window.window_bounds() {
            WindowBounds::Fullscreen(bounds)
            | WindowBounds::Maximized(bounds)
            | WindowBounds::Windowed(bounds) => bounds,
        })
        .ok()
}

/// Prefer the native window that currently owns keyboard/mouse focus.
///
/// This includes auxiliary windows, so a child opened from Connection Manager or Settings stays
/// anchored to that exact UI context instead of jumping to whichever TinyShell workspace happened
/// to be activated most recently.
fn active_native_window_bounds(cx: &mut App) -> Option<Bounds<Pixels>> {
    let handle = cx.active_window()?;
    window_bounds_for_handle(handle, cx)
}

/// Returns the bounds of the most recently active TinyShell workspace window.
///
/// This is a fallback for background-triggered auxiliary windows where no native window is active
/// at the instant placement is calculated.
fn active_workspace_bounds() -> Option<Bounds<Pixels>> {
    let registry = crate::app::window_registry();
    let guard = match registry.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            tracing::warn!("window registry lock was poisoned while positioning a child window");
            poisoned.into_inner()
        }
    };

    guard
        .iter()
        .filter(|entry| {
            entry.screen_bounds.size.width > px(0.) && entry.screen_bounds.size.height > px(0.)
        })
        .max_by_key(|entry| entry.activation_seq)
        .map(|entry| entry.screen_bounds)
}

fn bounds_center(bounds: Bounds<Pixels>) -> Point<Pixels> {
    point(
        bounds.origin.x + bounds.size.width / 2.,
        bounds.origin.y + bounds.size.height / 2.,
    )
}

fn display_bounds_for_point(cx: &App, position: Point<Pixels>) -> Option<Bounds<Pixels>> {
    cx.displays()
        .iter()
        .find_map(|display| {
            let bounds = display.bounds();
            bounds.contains(&position).then_some(bounds)
        })
        .or_else(|| cx.primary_display().map(|display| display.bounds()))
}

fn effective_window_size(
    spec: AuxiliaryWindowSpec,
    display_bounds: Bounds<Pixels>,
) -> Size<Pixels> {
    let max_width_ratio = spec.max_width_ratio.clamp(0.1, 1.0);
    let max_height_ratio = spec.max_height_ratio.clamp(0.1, 1.0);
    let max_width = display_bounds.size.width * max_width_ratio;
    let max_height = display_bounds.size.height * max_height_ratio;
    let min_size = spec.min_size.unwrap_or(size(px(0.), px(0.)));

    size(
        spec.preferred_size.width.max(min_size.width).min(max_width),
        spec.preferred_size
            .height
            .max(min_size.height)
            .min(max_height),
    )
}

fn clamp_window_origin(
    origin: Point<Pixels>,
    window_size: Size<Pixels>,
    display_bounds: Bounds<Pixels>,
) -> Point<Pixels> {
    let max_x = display_bounds.origin.x + display_bounds.size.width - window_size.width;
    let max_y = display_bounds.origin.y + display_bounds.size.height - window_size.height;
    point(
        origin.x.clamp(display_bounds.origin.x, max_x),
        origin.y.clamp(display_bounds.origin.y, max_y),
    )
}

fn centered_bounds(
    spec: AuxiliaryWindowSpec,
    anchor_bounds: Bounds<Pixels>,
    display_bounds: Bounds<Pixels>,
) -> WindowBounds {
    let window_size = effective_window_size(spec, display_bounds);
    let origin = point(
        anchor_bounds.origin.x + (anchor_bounds.size.width - window_size.width) / 2.,
        anchor_bounds.origin.y + (anchor_bounds.size.height - window_size.height) / 2.,
    );
    let origin = clamp_window_origin(origin, window_size, display_bounds);
    WindowBounds::Windowed(Bounds::new(origin, window_size))
}

fn near_bounds(
    spec: AuxiliaryWindowSpec,
    position: Point<Pixels>,
    offset_x: Pixels,
    offset_y: Pixels,
    display_bounds: Bounds<Pixels>,
) -> WindowBounds {
    let window_size = effective_window_size(spec, display_bounds);
    let origin = clamp_window_origin(
        point(position.x - offset_x, position.y - offset_y),
        window_size,
        display_bounds,
    );
    WindowBounds::Windowed(Bounds::new(origin, window_size))
}

/// Builds the canonical `WindowOptions` for every non-primary TinyShell window.
///
/// Ordinary auxiliary windows center on the currently active native parent, then fall back to the
/// latest active TinyShell workspace and finally the primary display. Drag-detached windows use
/// their drop position instead. Size limits and minimum size are resolved together so the OS cannot
/// silently enlarge a window after the centering calculation and make it appear offset.
pub(crate) fn auxiliary_window_options(cx: &mut App, spec: AuxiliaryWindowSpec) -> WindowOptions {
    let active_bounds = match spec.placement {
        AuxiliaryWindowPlacement::Centered => {
            active_native_window_bounds(cx).or_else(active_workspace_bounds)
        }
        AuxiliaryWindowPlacement::Near { .. } => None,
    };

    let window_bounds = match spec.placement {
        AuxiliaryWindowPlacement::Centered => {
            let anchor_bounds =
                active_bounds.or_else(|| cx.primary_display().map(|display| display.bounds()));
            anchor_bounds.and_then(|anchor_bounds| {
                let display_bounds = display_bounds_for_point(cx, bounds_center(anchor_bounds))?;
                Some(centered_bounds(spec, anchor_bounds, display_bounds))
            })
        }
        AuxiliaryWindowPlacement::Near {
            position,
            offset_x,
            offset_y,
        } => display_bounds_for_point(cx, position)
            .map(|display_bounds| near_bounds(spec, position, offset_x, offset_y, display_bounds)),
    };

    let effective_min_size = spec.min_size.map(|minimum| {
        if let Some(WindowBounds::Windowed(bounds)) = window_bounds.as_ref() {
            size(
                minimum.width.min(bounds.size.width),
                minimum.height.min(bounds.size.height),
            )
        } else {
            minimum
        }
    });

    let mut options = WindowOptions {
        is_movable: true,
        is_resizable: spec.resizable,
        is_minimizable: true,
        window_min_size: effective_min_size,
        window_bounds,
        ..Default::default()
    };

    #[cfg(not(target_os = "macos"))]
    if let Ok(image) = image::load_from_memory(include_bytes!("../../assets/icons/tiny-shell.png"))
    {
        options.icon = Some(std::sync::Arc::new(image.into_rgba8()));
    }

    options
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimum_size_is_applied_before_display_cap() {
        let spec = AuxiliaryWindowSpec::new(size(px(420.), px(220.)))
            .with_min_size(size(px(560.), px(360.)))
            .with_max_ratio(0.9, 0.9);
        let display = Bounds::new(point(px(0.), px(0.)), size(px(1000.), px(700.)));

        assert_eq!(
            effective_window_size(spec, display),
            size(px(560.), px(360.))
        );
    }

    #[test]
    fn display_cap_wins_when_minimum_cannot_fit() {
        let spec = AuxiliaryWindowSpec::new(size(px(600.), px(400.)))
            .with_min_size(size(px(560.), px(360.)))
            .with_max_ratio(0.5, 0.5);
        let display = Bounds::new(point(px(0.), px(0.)), size(px(800.), px(600.)));

        assert_eq!(
            effective_window_size(spec, display),
            size(px(400.), px(300.))
        );
    }
}
