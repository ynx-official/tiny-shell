use std::path::Path;

use anyhow::{Context, Result};
use gpui::{App, Bounds, Pixels, Point, Size, WindowBounds, point, px, size};

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

/// Returns the bounds of the most recently active TinyShell workspace window.
///
/// Auxiliary windows are intentionally centered relative to the owning workspace instead of
/// `displays().first()`. This keeps dialogs/editors on the same monitor as the user's current
/// workspace and makes multi-monitor placement deterministic.
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
        .or_else(|| cx.displays().first().map(|display| display.bounds()))
}

fn constrained_window_size(
    preferred_size: Size<Pixels>,
    display_bounds: Bounds<Pixels>,
    max_width_ratio: f32,
    max_height_ratio: f32,
) -> Size<Pixels> {
    let max_width_ratio = max_width_ratio.clamp(0.1, 1.0);
    let max_height_ratio = max_height_ratio.clamp(0.1, 1.0);
    size(
        preferred_size
            .width
            .min(display_bounds.size.width * max_width_ratio),
        preferred_size
            .height
            .min(display_bounds.size.height * max_height_ratio),
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

/// Standard placement for auxiliary windows such as settings, connection management and editors.
///
/// Placement priority:
/// 1. Center on the most recently active TinyShell workspace window.
/// 2. Use the display containing that workspace.
/// 3. Fall back to the first display only when no workspace bounds are available.
///
/// The final bounds are clamped to the target display, so a child window cannot spill off-screen
/// when the parent is close to a monitor edge or spans multiple displays.
pub(crate) fn centered_child_window_bounds(
    cx: &App,
    preferred_size: Size<Pixels>,
    max_width_ratio: f32,
    max_height_ratio: f32,
) -> Option<WindowBounds> {
    let workspace_bounds = active_workspace_bounds();
    let anchor = workspace_bounds.map(bounds_center);
    let display_bounds = anchor
        .and_then(|position| display_bounds_for_point(cx, position))
        .or_else(|| cx.displays().first().map(|display| display.bounds()))?;
    let window_size = constrained_window_size(
        preferred_size,
        display_bounds,
        max_width_ratio,
        max_height_ratio,
    );
    let center_bounds = workspace_bounds.unwrap_or(display_bounds);
    let origin = point(
        center_bounds.origin.x + (center_bounds.size.width - window_size.width) / 2.,
        center_bounds.origin.y + (center_bounds.size.height - window_size.height) / 2.,
    );
    let origin = clamp_window_origin(origin, window_size, display_bounds);

    Some(WindowBounds::Windowed(Bounds::new(origin, window_size)))
}

/// Placement for drag-detached windows.
///
/// Unlike ordinary auxiliary windows these should stay near the user's drop point. The display
/// under the drop position is selected first, then the resulting bounds are clamped to it.
pub(crate) fn window_bounds_near_position(
    cx: &App,
    preferred_size: Size<Pixels>,
    max_width_ratio: f32,
    max_height_ratio: f32,
    position: Point<Pixels>,
    offset_x: Pixels,
    offset_y: Pixels,
) -> Option<WindowBounds> {
    let display_bounds = display_bounds_for_point(cx, position)?;
    let window_size = constrained_window_size(
        preferred_size,
        display_bounds,
        max_width_ratio,
        max_height_ratio,
    );
    let origin = clamp_window_origin(
        point(position.x - offset_x, position.y - offset_y),
        window_size,
        display_bounds,
    );

    Some(WindowBounds::Windowed(Bounds::new(origin, window_size)))
}
