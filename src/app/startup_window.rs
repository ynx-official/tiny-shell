use std::time::{Duration, Instant};

use gpui::{
    App, Bounds, Context, Entity, FontWeight, IntoElement as _, ParentElement as _, Pixels, Point,
    Render, Styled as _, Window, WindowBounds, div, point, px, size,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, Size, h_flex, spinner::Spinner, v_flex,
};
use rust_i18n::t;

use crate::TinyShell;

const STARTUP_WIDTH: f32 = 320.;
const STARTUP_HEIGHT: f32 = 184.;
const EXPANSION_DURATION: Duration = Duration::from_millis(220);

/// Lightweight first-frame content that stays visible while the full workspace
/// is initialized and the native window expands to its persisted size.
pub(crate) struct StartupWindow {
    workspace: Option<Entity<TinyShell>>,
    revealed: bool,
}

impl StartupWindow {
    pub(crate) fn new() -> Self {
        Self {
            workspace: None,
            revealed: false,
        }
    }

    pub(crate) fn workspace(&self) -> Option<Entity<TinyShell>> {
        self.workspace.clone()
    }

    pub(crate) fn install_workspace(&mut self, workspace: Entity<TinyShell>) {
        self.workspace = Some(workspace);
    }

    pub(crate) fn reveal(&mut self) {
        self.revealed = true;
    }
}

impl Render for StartupWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        if self.revealed
            && let Some(workspace) = self.workspace.as_ref()
        {
            return div()
                .size_full()
                .overflow_hidden()
                .child(workspace.clone())
                .into_any_element();
        }

        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Icon::new(IconName::SquareTerminal)
                            .with_size(Size::Medium)
                            .text_color(cx.theme().primary),
                    )
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(t!("app_name").to_string()),
                    ),
            )
            .child(
                Spinner::new()
                    .with_size(Size::Small)
                    .color(cx.theme().primary),
            )
            .into_any_element()
    }
}

pub(crate) fn startup_window_bounds(
    target: Option<WindowBounds>,
    cx: &App,
) -> Option<WindowBounds> {
    target.map(|target| {
        let target = target.get_bounds();
        let display = target_display_bounds(target, cx);
        WindowBounds::Windowed(startup_bounds_for_platform(target, display))
    })
}

fn target_display_bounds(target: Bounds<Pixels>, cx: &App) -> Bounds<Pixels> {
    cx.displays()
        .into_iter()
        .map(|display| display.bounds())
        .max_by(|left, right| overlap_area(target, *left).total_cmp(&overlap_area(target, *right)))
        .filter(|display| overlap_area(target, *display) > 0.)
        .or_else(|| cx.primary_display().map(|display| display.bounds()))
        .unwrap_or(target)
}

fn overlap_area(left: Bounds<Pixels>, right: Bounds<Pixels>) -> f32 {
    let width = (left.right().min(right.right()) - left.left().max(right.left()))
        .as_f32()
        .max(0.);
    let height = (left.bottom().min(right.bottom()) - left.top().max(right.top()))
        .as_f32()
        .max(0.);
    width * height
}

#[cfg(target_os = "windows")]
fn startup_bounds_for_platform(target: Bounds<Pixels>, display: Bounds<Pixels>) -> Bounds<Pixels> {
    centered_startup_bounds(target, display)
}

#[cfg(not(target_os = "windows"))]
fn startup_bounds_for_platform(target: Bounds<Pixels>, _display: Bounds<Pixels>) -> Bounds<Pixels> {
    Bounds::new(target.origin, compact_startup_size(target))
}

#[cfg(any(target_os = "windows", test))]
fn centered_startup_bounds(target: Bounds<Pixels>, display: Bounds<Pixels>) -> Bounds<Pixels> {
    let compact_size = compact_startup_size(target);
    Bounds::new(
        point(
            display.origin.x + (display.size.width - compact_size.width) / 2.,
            display.origin.y + (display.size.height - compact_size.height) / 2.,
        ),
        compact_size,
    )
}

fn compact_startup_size(target: Bounds<Pixels>) -> gpui::Size<Pixels> {
    size(
        px(target.size.width.as_f32().min(STARTUP_WIDTH)),
        px(target.size.height.as_f32().min(STARTUP_HEIGHT)),
    )
}

#[derive(Clone, Copy)]
pub(crate) struct StartupExpansion {
    start: Bounds<Pixels>,
    target: Bounds<Pixels>,
    started_at: Instant,
}

impl StartupExpansion {
    pub(crate) fn new(start: Bounds<Pixels>, target: Bounds<Pixels>, started_at: Instant) -> Self {
        Self {
            start,
            target,
            started_at,
        }
    }

    pub(crate) fn frame_at(self, now: Instant) -> StartupExpansionFrame {
        let progress = now.saturating_duration_since(self.started_at).as_secs_f32()
            / EXPANSION_DURATION.as_secs_f32();
        let progress = progress.clamp(0., 1.);
        let eased = 1. - (1. - progress).powi(5);
        StartupExpansionFrame {
            bounds: Bounds::new(
                point(
                    px(lerp(
                        self.start.origin.x.as_f32(),
                        self.target.origin.x.as_f32(),
                        eased,
                    )),
                    px(lerp(
                        self.start.origin.y.as_f32(),
                        self.target.origin.y.as_f32(),
                        eased,
                    )),
                ),
                size(
                    px(lerp(
                        self.start.size.width.as_f32(),
                        self.target.size.width.as_f32(),
                        eased,
                    )),
                    px(lerp(
                        self.start.size.height.as_f32(),
                        self.target.size.height.as_f32(),
                        eased,
                    )),
                ),
            ),
            complete: progress >= 1.,
        }
    }
}

pub(crate) struct StartupExpansionFrame {
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) complete: bool,
}

#[cfg(target_os = "windows")]
pub(crate) fn move_startup_window(window: &Window, origin: Point<Pixels>) {
    use std::sync::atomic::{AtomicBool, Ordering};

    static MOVE_WARNING_REPORTED: AtomicBool = AtomicBool::new(false);

    if let Err(error) = move_windows_window(window, origin)
        && !MOVE_WARNING_REPORTED.swap(true, Ordering::AcqRel)
    {
        tracing::warn!(%error, "failed to animate startup window position");
    }
}

#[cfg(target_os = "windows")]
fn move_windows_window(window: &Window, origin: Point<Pixels>) -> anyhow::Result<()> {
    use anyhow::{Context as _, anyhow, bail};
    use raw_window_handle::RawWindowHandle;
    use windows_sys::Win32::{
        Foundation::{POINT, RECT},
        Graphics::Gdi::ClientToScreen,
        UI::WindowsAndMessaging::{
            GetWindowRect, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos,
        },
    };

    let handle = raw_window_handle::HasWindowHandle::window_handle(window).map_err(|error| {
        anyhow!("GPUI did not expose a native startup window handle: {error:?}")
    })?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        bail!("startup window is not backed by Win32");
    };
    let hwnd = handle.hwnd.get() as windows_sys::Win32::Foundation::HWND;
    let mut outer_rect = RECT::default();
    let mut client_origin = POINT::default();

    // GPUI stores global logical client coordinates, while SetWindowPos expects
    // physical outer-window coordinates. Preserve the native border offset so
    // the content itself follows the interpolated logical bounds exactly.
    if unsafe { GetWindowRect(hwnd, &mut outer_rect) } == 0 {
        return Err(std::io::Error::last_os_error()).context("GetWindowRect failed");
    }
    if unsafe { ClientToScreen(hwnd, &mut client_origin) } == 0 {
        return Err(std::io::Error::last_os_error()).context("ClientToScreen failed");
    }

    let scale_factor = window.scale_factor();
    let x = (origin.x.as_f32() * scale_factor).round() as i32 + (outer_rect.left - client_origin.x);
    let y = (origin.y.as_f32() * scale_factor).round() as i32 + (outer_rect.top - client_origin.y);
    let result = unsafe {
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            x,
            y,
            0,
            0,
            SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER,
        )
    };
    if result == 0 {
        return Err(std::io::Error::last_os_error()).context("SetWindowPos failed");
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn move_startup_window(_window: &Window, _origin: Point<Pixels>) {}

fn lerp(start: f32, target: f32, progress: f32) -> f32 {
    start + (target - start) * progress
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use gpui::{Bounds, point, px, size};

    use super::{StartupExpansion, centered_startup_bounds};

    #[test]
    fn startup_window_uses_compact_size_and_centers_on_target_display() {
        let target = Bounds::new(point(px(120.), px(80.)), size(px(1200.), px(760.)));
        let display = Bounds::new(point(px(0.), px(0.)), size(px(1920.), px(1080.)));

        assert_eq!(
            centered_startup_bounds(target, display),
            Bounds::new(point(px(800.), px(448.)), size(px(320.), px(184.)))
        );
    }

    #[test]
    fn startup_window_never_grows_beyond_a_small_target() {
        let target = Bounds::new(point(px(12.), px(16.)), size(px(280.), px(160.)));
        let display = Bounds::new(point(px(0.), px(0.)), size(px(1920.), px(1080.)));

        assert_eq!(
            centered_startup_bounds(target, display),
            Bounds::new(point(px(820.), px(460.)), target.size)
        );
    }

    #[test]
    fn startup_expansion_reaches_target_with_ease_out_motion() {
        let started_at = Instant::now();
        let start = Bounds::new(point(px(800.), px(448.)), size(px(320.), px(184.)));
        let target = Bounds::new(point(px(120.), px(80.)), size(px(1200.), px(760.)));
        let expansion = StartupExpansion::new(start, target, started_at);

        let first = expansion.frame_at(started_at);
        assert_eq!(first.bounds, start);
        assert!(!first.complete);

        let midpoint = expansion.frame_at(started_at + Duration::from_millis(110));
        assert!(midpoint.bounds.origin.x < start.origin.x);
        assert!(midpoint.bounds.origin.x > target.origin.x);
        assert!(midpoint.bounds.size.width > px(760.));
        assert!(midpoint.bounds.size.width < px(1200.));
        assert!(!midpoint.complete);

        let final_frame = expansion.frame_at(started_at + Duration::from_millis(220));
        assert_eq!(final_frame.bounds, target);
        assert!(final_frame.complete);
    }
}
