use std::time::{Duration, Instant};

use gpui::{
    Bounds, Context, Entity, FontWeight, IntoElement as _, ParentElement as _, Pixels, Render,
    Size as GpuiSize, Styled as _, Window, WindowBounds, div, px, size,
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

pub(crate) fn startup_window_bounds(target: Option<WindowBounds>) -> Option<WindowBounds> {
    target.map(|target| {
        let target = target.get_bounds();
        WindowBounds::Windowed(Bounds::new(
            target.origin,
            size(
                px(target.size.width.as_f32().min(STARTUP_WIDTH)),
                px(target.size.height.as_f32().min(STARTUP_HEIGHT)),
            ),
        ))
    })
}

#[derive(Clone, Copy)]
pub(crate) struct StartupExpansion {
    start: GpuiSize<Pixels>,
    target: GpuiSize<Pixels>,
    started_at: Instant,
}

impl StartupExpansion {
    pub(crate) fn new(
        start: GpuiSize<Pixels>,
        target: GpuiSize<Pixels>,
        started_at: Instant,
    ) -> Self {
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
            size: size(
                px(lerp(
                    self.start.width.as_f32(),
                    self.target.width.as_f32(),
                    eased,
                )),
                px(lerp(
                    self.start.height.as_f32(),
                    self.target.height.as_f32(),
                    eased,
                )),
            ),
            complete: progress >= 1.,
        }
    }
}

pub(crate) struct StartupExpansionFrame {
    pub(crate) size: GpuiSize<Pixels>,
    pub(crate) complete: bool,
}

fn lerp(start: f32, target: f32, progress: f32) -> f32 {
    start + (target - start) * progress
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use gpui::{Bounds, WindowBounds, point, px, size};

    use super::{StartupExpansion, startup_window_bounds};

    #[test]
    fn startup_window_uses_compact_size_and_preserves_restore_origin() {
        let target = Bounds::new(point(px(120.), px(80.)), size(px(1200.), px(760.)));

        for target_state in [
            WindowBounds::Windowed(target),
            WindowBounds::Maximized(target),
            WindowBounds::Fullscreen(target),
        ] {
            assert_eq!(
                startup_window_bounds(Some(target_state)),
                Some(WindowBounds::Windowed(Bounds::new(
                    target.origin,
                    size(px(320.), px(184.)),
                )))
            );
        }
    }

    #[test]
    fn startup_window_never_grows_beyond_a_small_target() {
        let target = Bounds::new(point(px(12.), px(16.)), size(px(280.), px(160.)));

        assert_eq!(
            startup_window_bounds(Some(WindowBounds::Windowed(target))),
            Some(WindowBounds::Windowed(target))
        );
        assert_eq!(startup_window_bounds(None), None);
    }

    #[test]
    fn startup_expansion_reaches_target_with_ease_out_motion() {
        let started_at = Instant::now();
        let expansion = StartupExpansion::new(
            size(px(320.), px(184.)),
            size(px(1200.), px(760.)),
            started_at,
        );

        let first = expansion.frame_at(started_at);
        assert_eq!(first.size, size(px(320.), px(184.)));
        assert!(!first.complete);

        let midpoint = expansion.frame_at(started_at + Duration::from_millis(110));
        assert!(midpoint.size.width > px(760.));
        assert!(midpoint.size.width < px(1200.));
        assert!(!midpoint.complete);

        let final_frame = expansion.frame_at(started_at + Duration::from_millis(220));
        assert_eq!(final_frame.size, size(px(1200.), px(760.)));
        assert!(final_frame.complete);
    }
}
