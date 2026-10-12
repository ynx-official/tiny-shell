//! Shared presentation for short, two-action confirmations. Business callbacks remain at the call site.

use std::{cell::Cell, rc::Rc};

use gpui::{
    App, ClickEvent, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _, Pixels,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px, rgb,
};
use gpui_component::{
    ActiveTheme as _, Colorize as _, Sizable as _, WindowExt as _,
    dialog::{ConfirmDialog, Dialog},
    h_flex,
    scroll::ScrollableElement as _,
    spinner::Spinner,
};
use rust_i18n::t;

type Action = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) -> bool>;
type OnClose = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(Clone)]
struct PendingConfirmation {
    title: SharedString,
    description: SharedString,
    label: SharedString,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConfirmationPhase {
    Ready,
    Acting,
    Pending,
    Completed,
}

/// Use `open` for standalone windows, or `build` inside the existing modal queue.
/// Return false from an action to keep the dialog open (for example after validation fails).
#[derive(Clone)]
pub(crate) struct ConfirmationDialog {
    title: SharedString,
    description: SharedString,
    confirm_label: SharedString,
    cancel_label: SharedString,
    danger: bool,
    keyboard: bool,
    on_ok: Action,
    on_cancel: Action,
    on_close: OnClose,
    phase: Rc<Cell<ConfirmationPhase>>,
    pending: Option<PendingConfirmation>,
    layer_ix: usize,
}

impl ConfirmationDialog {
    pub(crate) fn new(
        title: impl Into<SharedString>,
        description: impl Into<SharedString>,
    ) -> Self {
        Self {
            title: title.into(),
            description: description.into(),
            confirm_label: t!("confirm").to_string().into(),
            cancel_label: t!("cancel").to_string().into(),
            danger: false,
            keyboard: true,
            on_ok: Rc::new(|_, _, _| true),
            on_cancel: Rc::new(|_, _, _| true),
            on_close: Rc::new(|_, _, _| {}),
            phase: Rc::new(Cell::new(ConfirmationPhase::Ready)),
            pending: None,
            layer_ix: 0,
        }
    }

    pub(crate) fn confirm_label(mut self, label: impl Into<SharedString>) -> Self {
        self.confirm_label = label.into();
        self
    }

    pub(crate) fn cancel_label(mut self, label: impl Into<SharedString>) -> Self {
        self.cancel_label = label.into();
        self
    }

    /// An accepted confirmation stays busy until its owner removes the window.
    /// A rejected action remains available for retry or cancellation.
    pub(crate) fn pending_on_confirm(
        mut self,
        title: impl Into<SharedString>,
        description: impl Into<SharedString>,
        label: impl Into<SharedString>,
    ) -> Self {
        self.pending = Some(PendingConfirmation {
            title: title.into(),
            description: description.into(),
            label: label.into(),
        });
        self
    }

    pub(crate) fn danger(mut self, danger: bool) -> Self {
        self.danger = danger;
        self
    }

    pub(crate) fn keyboard(mut self, enabled: bool) -> Self {
        self.keyboard = enabled;
        self
    }

    pub(crate) fn on_ok(
        mut self,
        action: impl Fn(&ClickEvent, &mut Window, &mut App) -> bool + 'static,
    ) -> Self {
        self.on_ok = Rc::new(action);
        self
    }

    pub(crate) fn on_cancel(
        mut self,
        action: impl Fn(&ClickEvent, &mut Window, &mut App) -> bool + 'static,
    ) -> Self {
        self.on_cancel = Rc::new(action);
        self
    }

    pub(crate) fn on_close(
        mut self,
        action: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_close = Rc::new(action);
        self
    }

    pub(crate) fn open(mut self, window: &mut Window, cx: &mut App) {
        // The app's modal queues allow one active dialog; a direct confirmation can cover it.
        self.layer_ix = usize::from(window.has_active_dialog(cx));
        window.open_dialog(cx, move |dialog, window, cx| {
            self.clone().build(dialog, window, cx)
        });
    }

    pub(crate) fn build(mut self, dialog: Dialog, window: &mut Window, cx: &mut App) -> Dialog {
        let pending = self.phase.get() == ConfirmationPhase::Pending;
        let scale = (window.rem_size() / px(14.)).max(1.);
        let paddings = gpui_component::window_paddings(window);
        let width = confirmation_width(
            window.viewport_size().width - paddings.left - paddings.right,
            scale,
        );
        let inner_width = (width - px(50.) * scale).max(px(1.));
        let measure_content = |title, description| {
            content_height(
                measure_text(
                    title,
                    px(17.) * scale,
                    px(24.) * scale,
                    inner_width,
                    true,
                    window,
                ),
                measure_text(
                    description,
                    px(13.) * scale,
                    px(20.) * scale,
                    (inner_width - px(12.) * scale).max(px(1.)),
                    false,
                    window,
                ),
                scale,
            )
        };
        let mut preferred_height = measure_content(&self.title, &self.description);
        let measure_button = |label: &SharedString| {
            let style = window.text_style();
            window
                .text_system()
                .shape_line(
                    label.clone(),
                    px(14.) * scale,
                    &[style.to_run(label.len())],
                    None,
                )
                .width
                + px(32.) * scale
        };
        let mut button_widths = [&self.cancel_label, &self.confirm_label].map(measure_button);
        if let Some(status) = &self.pending {
            // Reserve both presentations so a confirmed close does not move or resize the modal.
            preferred_height =
                preferred_height.max(measure_content(&status.title, &status.description));
            button_widths[1] =
                button_widths[1].max(measure_button(&status.label) + px(22.) * scale);
            // Leave room for both the native border and keyboard focus border.
            for width in &mut button_widths {
                *width += px(4.);
            }
            if pending {
                self.title = status.title.clone();
                self.description = status.description.clone();
                self.confirm_label = status.label.clone();
            }
        }
        let stacked = button_widths[0] + button_widths[1] + px(8.) * scale > inner_width;
        preferred_height += if stacked { px(40.) * scale } else { px(0.) };
        let layout =
            super::dialog_layout::centered_dialog_layout(window, preferred_height, self.layer_ix);
        let body = self.description.clone();
        let scroll_handle = window
            .use_keyed_state(("confirmation-scroll", self.layer_ix), cx, |_, _| {
                gpui::ScrollHandle::new()
            })
            .read(cx)
            .clone();
        let on_close = self.on_close.clone();
        let reset_scroll = scroll_handle.clone();
        self.on_close = Rc::new(move |event, window, cx| {
            reset_scroll.set_offset(gpui::point(px(0.), px(0.)));
            on_close(event, window, cx);
        });
        let cancel = self.button(false, scale, button_widths[0], window, cx);
        let confirm = self.button(true, scale, button_widths[1], window, cx);
        let ok = self.clone();
        let cancel_action = self.clone();

        dialog
            .w(width)
            .h(layout.height)
            .margin_top(layout.margin_top)
            .p(px(24.) * scale)
            .gap(px(24.) * scale)
            .rounded(px(8.))
            .close_button(false)
            .overlay_closable(false)
            .keyboard(self.keyboard && !pending)
            .title(
                div()
                    .debug_selector(|| "confirmation-title".to_owned())
                    .text_size(px(17.) * scale)
                    .line_height(px(24.) * scale)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.title),
            )
            .content(move |content, _, cx| {
                content.min_h_0().child(
                    div()
                        .id("confirmation-description")
                        .relative()
                        .min_h_0()
                        .overflow_y_scroll()
                        .track_scroll(&scroll_handle)
                        .pr(px(12.) * scale)
                        .text_size(px(13.) * scale)
                        .line_height(px(20.) * scale)
                        .text_color(cx.theme().muted_foreground)
                        .child(body.clone())
                        .vertical_scrollbar(&scroll_handle),
                )
            })
            .footer(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap(px(8.) * scale)
                    .when(stacked, |row| row.flex_col().items_end())
                    .child(cancel)
                    .child(confirm),
            )
            // Keyboard and mouse share the deferred path, including vetoes and modal replacement.
            .on_ok(move |_, window, cx| {
                ok.click(true, window, cx);
                false
            })
            .on_cancel(move |_, window, cx| {
                cancel_action.click(false, window, cx);
                false
            })
    }

    fn accept(&self, confirm: bool, event: &ClickEvent, window: &mut Window, cx: &mut App) -> bool {
        complete_once(&self.phase, confirm && self.pending.is_some(), || {
            if confirm {
                (self.on_ok)(event, window, cx)
            } else {
                (self.on_cancel)(event, window, cx)
            }
        })
    }

    fn click(&self, confirm: bool, window: &mut Window, cx: &mut App) {
        let dialog = self.clone();
        // A confirmation may remove its native window. Run it after input dispatch has finished.
        window.defer(cx, move |window, cx| {
            let event = ClickEvent::default();
            if dialog.accept(confirm, &event, window, cx) {
                window.close_dialog(cx);
                (dialog.on_close)(&event, window, cx);
            } else if dialog.phase.get() == ConfirmationPhase::Pending {
                window.refresh();
            }
        });
    }

    fn button(
        &self,
        confirm: bool,
        scale: f32,
        width: Pixels,
        window: &mut Window,
        cx: &mut App,
    ) -> impl IntoElement + use<> {
        let id = if confirm {
            "confirmation-submit"
        } else {
            "confirmation-cancel"
        };
        let focus = window
            .use_keyed_state(id, cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let focused = focus.is_focused(window);
        let pending = self.phase.get() == ConfirmationPhase::Pending;
        let background = if confirm {
            if self.danger {
                rgb(0xc93737).into()
            } else {
                cx.theme().button_primary
            }
        } else {
            cx.theme().background
        };
        let foreground = if confirm {
            if self.danger {
                rgb(0xffffff).into()
            } else {
                cx.theme().button_primary_foreground
            }
        } else {
            if pending {
                cx.theme().muted_foreground.opacity(0.5)
            } else {
                cx.theme().foreground
            }
        };
        let hover = if confirm {
            background.darken(0.06)
        } else {
            cx.theme().secondary
        };
        let click = self.clone();
        let key = self.clone();
        let enter = self.clone();
        // The pinned component library intentionally tints Danger/Custom buttons. A small native
        // control keeps this destructive action solid in normal, hover and pressed states.
        div()
            .id(id)
            .debug_selector(|| id.to_owned())
            .track_focus(
                &focus
                    .tab_index(if confirm { 1 } else { 0 })
                    .tab_stop(!pending),
            )
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .when(!pending, |button| button.cursor_pointer())
            .min_w(width.max(px(68.) * scale))
            .when(self.pending.is_some(), |button| {
                button.w(width.max(px(68.) * scale))
            })
            .h(px(32.) * scale)
            .px(px(16.) * scale)
            .rounded(px(5.))
            .border_1()
            .border_color(if confirm {
                background
            } else {
                cx.theme().border
            })
            .bg(if pending && !confirm {
                cx.theme().secondary
            } else {
                background
            })
            .text_color(foreground)
            .text_size(px(14.) * scale)
            .line_height(px(20.) * scale)
            .when(!pending, |button| {
                button
                    .hover(move |style| style.bg(hover))
                    .active(move |style| style.bg(hover.darken(0.04)))
            })
            .when(focused && !pending, |button| {
                button.border_2().border_color(cx.theme().ring)
            })
            .on_click(move |_, window, cx| click.click(confirm, window, cx))
            // GPUI resolves the Dialog Enter keybinding before raw key-down handlers. Handle the
            // action on the focused button so Enter on Cancel cannot fall through to confirmation.
            .on_action(move |_: &ConfirmDialog, window, cx| {
                cx.stop_propagation();
                enter.click(confirm, window, cx);
            })
            .on_key_down(move |event, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    cx.stop_propagation();
                    key.click(confirm, window, cx);
                }
            })
            .when(pending && confirm, |button| {
                button.gap(px(8.) * scale).child(
                    div()
                        .debug_selector(|| "confirmation-pending-spinner".to_owned())
                        .child(Spinner::new().with_size(px(14.) * scale).color(foreground)),
                )
            })
            .child(if confirm {
                self.confirm_label.clone()
            } else {
                self.cancel_label.clone()
            })
    }
}

fn measure_text(
    text: &SharedString,
    size: Pixels,
    line_height: Pixels,
    width: Pixels,
    bold: bool,
    window: &Window,
) -> Pixels {
    let mut style = window.text_style();
    if bold {
        style.font_weight = FontWeight::SEMIBOLD;
    }
    match window.text_system().shape_text(
        text.clone(),
        size,
        &[style.to_run(text.len())],
        Some(width),
        None,
    ) {
        Ok(lines) => lines
            .iter()
            .map(|line| line.size(line_height).height)
            .sum::<Pixels>()
            .max(line_height),
        Err(error) => {
            tracing::warn!("failed to measure confirmation text: {error}");
            line_height
                * (text.chars().count() as f32 * f32::from(size) / f32::from(width))
                    .ceil()
                    .max(1.)
        }
    }
}

fn confirmation_width(viewport: Pixels, scale: f32) -> Pixels {
    (px(380.) * scale).min((viewport - px(32.)).max(px(1.)))
}

fn complete_once(
    phase: &Cell<ConfirmationPhase>,
    pending_on_confirm: bool,
    action: impl FnOnce() -> bool,
) -> bool {
    if phase.get() != ConfirmationPhase::Ready {
        return false;
    }
    phase.set(ConfirmationPhase::Acting);
    let accepted = action();
    phase.set(if !accepted {
        ConfirmationPhase::Ready
    } else if pending_on_confirm {
        ConfirmationPhase::Pending
    } else {
        ConfirmationPhase::Completed
    });
    accepted && !pending_on_confirm
}

fn content_height(title: Pixels, description: Pixels, scale: f32) -> Pixels {
    // Padding + title/body gap + footer gap + control, with the native border accounted for.
    title + description + px(48. + 8. + 24. + 32. + 2.) * scale
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext as _, Context, Render, TestAppContext, VisualTestContext};
    use gpui_component::Root;

    #[derive(Clone, Default)]
    struct Actions {
        confirmed: Rc<Cell<usize>>,
        cancelled: Rc<Cell<usize>>,
        closed: Rc<Cell<usize>>,
    }

    struct ConfirmationHost;

    impl Render for ConfirmationHost {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .children(Root::render_dialog_layer(window, cx))
        }
    }

    fn open_confirmation(cx: &mut TestAppContext, pending: bool) -> (VisualTestContext, Actions) {
        cx.update(gpui_component::init);
        let handle = cx.open_window(gpui::size(px(800.), px(600.)), |window, cx| {
            let host = cx.new(|_| ConfirmationHost);
            Root::new(host, window, cx)
        });
        let actions = Actions::default();
        let callbacks = actions.clone();
        let root = handle.root(cx).unwrap();
        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        visual.update(|window, cx| {
            let mut dialog =
                ConfirmationDialog::new("Close window?", "End this window's sessions.")
                    .danger(true)
                    .confirm_label("Close window")
                    .cancel_label("Cancel");
            if pending {
                dialog = dialog.pending_on_confirm("Closing window", "Please wait.", "Closing…");
            }
            dialog
                .on_ok(move |_, _, _| {
                    callbacks.confirmed.set(callbacks.confirmed.get() + 1);
                    true
                })
                .on_cancel(move |_, _, _| {
                    callbacks.cancelled.set(callbacks.cancelled.get() + 1);
                    true
                })
                .on_close(move |_, _, _| callbacks.closed.set(callbacks.closed.get() + 1))
                .open(window, cx);
        });
        visual.draw(
            gpui::point(px(0.), px(0.)),
            gpui::size(px(800.), px(600.)),
            |_, _| root.into_any_element(),
        );
        // Dialog's entrance animation uses wall-clock time; compare settled geometry.
        std::thread::sleep(std::time::Duration::from_millis(300));
        visual.update(|window, _| window.refresh());
        visual.run_until_parked();
        (visual, actions)
    }

    #[gpui::test]
    fn confirmed_close_keeps_the_modal_until_the_window_is_removed(cx: &mut TestAppContext) {
        let (mut visual, actions) = open_confirmation(cx, true);
        let submit = visual.debug_bounds("confirmation-submit").unwrap();
        let submit_before = submit;
        let cancel_before = visual.debug_bounds("confirmation-cancel").unwrap();
        let title_before = visual.debug_bounds("confirmation-title").unwrap();
        visual.simulate_click(submit.center(), gpui::Modifiers::none());
        visual.run_until_parked();
        assert_eq!(actions.confirmed.get(), 1);
        assert!(
            visual.update(|window, cx| window.has_active_dialog(cx)),
            "a confirmed close must remain visible while shutdown is pending"
        );
        assert_eq!(actions.closed.get(), 0);
        assert!(
            visual
                .debug_bounds("confirmation-pending-spinner")
                .is_some()
        );

        let cancel = visual.debug_bounds("confirmation-cancel").unwrap();
        let submit = visual.debug_bounds("confirmation-submit").unwrap();
        assert_eq!(cancel, cancel_before, "the pending footer must not move");
        assert_eq!(submit, submit_before, "the loading button must not resize");
        assert_eq!(
            visual.debug_bounds("confirmation-title").unwrap(),
            title_before
        );
        visual.simulate_click(cancel.center(), gpui::Modifiers::none());
        visual.simulate_click(submit.center(), gpui::Modifiers::none());
        visual.simulate_keystrokes("escape enter space");
        visual.run_until_parked();
        assert_eq!(actions.confirmed.get(), 1);
        assert_eq!(actions.cancelled.get(), 0);
        assert_eq!(actions.closed.get(), 0);
        assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
        visual.update(|window, _| window.remove_window());
    }

    #[gpui::test]
    fn closing_can_still_be_cancelled_before_confirmation(cx: &mut TestAppContext) {
        let (mut visual, actions) = open_confirmation(cx, true);
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        assert_eq!(actions.confirmed.get(), 0);
        assert_eq!(actions.cancelled.get(), 1);
        assert_eq!(actions.closed.get(), 1);
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
    }

    #[gpui::test]
    fn enter_on_cancel_does_not_start_shutdown(cx: &mut TestAppContext) {
        let (mut visual, actions) = open_confirmation(cx, true);
        visual.simulate_keystrokes("tab enter");
        visual.run_until_parked();
        assert_eq!(actions.confirmed.get(), 0);
        assert_eq!(actions.cancelled.get(), 1);
        assert_eq!(actions.closed.get(), 1);
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
    }

    #[gpui::test]
    fn ordinary_confirmation_still_closes_immediately(cx: &mut TestAppContext) {
        let (mut visual, actions) = open_confirmation(cx, false);
        let submit = visual.debug_bounds("confirmation-submit").unwrap();
        visual.simulate_click(submit.center(), gpui::Modifiers::none());
        visual.run_until_parked();
        assert_eq!(actions.confirmed.get(), 1);
        assert_eq!(actions.closed.get(), 1);
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
    }

    #[gpui::test]
    fn narrow_large_font_closing_keeps_the_spinner_and_buttons_inside_the_window(
        cx: &mut TestAppContext,
    ) {
        let (mut visual, actions) = open_confirmation(cx, true);
        visual.update(|window, cx| {
            gpui_component::Theme::global_mut(cx).font_size = px(21.);
            window.refresh();
        });
        visual.simulate_resize(gpui::size(px(320.), px(600.)));
        visual.run_until_parked();
        let submit = visual.debug_bounds("confirmation-submit").unwrap();
        let cancel = visual.debug_bounds("confirmation-cancel").unwrap();
        assert!(
            submit.origin.y >= cancel.bottom(),
            "narrow controls must stack"
        );
        visual.simulate_click(submit.center(), gpui::Modifiers::none());
        visual.run_until_parked();
        assert_eq!(actions.confirmed.get(), 1);
        let submit = visual.debug_bounds("confirmation-submit").unwrap();
        let spinner = visual.debug_bounds("confirmation-pending-spinner").unwrap();
        assert!(submit.right() <= px(320.) && submit.bottom() <= px(600.));
        assert!(spinner.origin.x >= submit.origin.x && spinner.right() <= submit.right());
        assert!(spinner.origin.y >= submit.origin.y && spinner.bottom() <= submit.bottom());
        assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
    }

    #[test]
    fn short_confirmation_has_no_unused_body_space() {
        assert_eq!(content_height(px(24.), px(20.), 1.), px(158.));
    }

    #[test]
    fn wrapped_description_increases_height_instead_of_clipping() {
        let short = content_height(px(24.), px(20.), 1.);
        let wrapped = content_height(px(24.), px(80.), 1.);
        assert_eq!(wrapped - short, px(60.));
    }

    #[test]
    fn larger_fonts_scale_spacing_and_controls_together() {
        assert_eq!(content_height(px(36.), px(60.), 1.5), px(267.));
    }

    #[test]
    fn narrow_windows_keep_horizontal_gutters_even_with_large_fonts() {
        assert_eq!(confirmation_width(px(320.), 1.5), px(288.));
        assert_eq!(confirmation_width(px(1000.), 1.), px(380.));
    }

    #[test]
    fn rapid_confirmation_only_runs_the_action_once() {
        let completed = Cell::new(ConfirmationPhase::Ready);
        let count = Cell::new(0);
        let action = || {
            count.set(count.get() + 1);
            true
        };
        assert!(complete_once(&completed, false, action));
        assert!(!complete_once(&completed, false, action));
        assert_eq!(count.get(), 1);
    }

    #[test]
    fn veto_keeps_the_dialog_available_for_retry_or_cancel() {
        let completed = Cell::new(ConfirmationPhase::Ready);
        assert!(!complete_once(&completed, false, || false));
        assert_eq!(completed.get(), ConfirmationPhase::Ready);
        assert!(complete_once(&completed, false, || true));
    }

    #[test]
    fn rejected_pending_confirmation_remains_available_for_retry() {
        let phase = Cell::new(ConfirmationPhase::Ready);
        assert!(!complete_once(&phase, true, || false));
        assert_eq!(phase.get(), ConfirmationPhase::Ready);
        assert!(!complete_once(&phase, true, || true));
        assert_eq!(phase.get(), ConfirmationPhase::Pending);
        assert!(!complete_once(&phase, false, || panic!(
            "pending cancellation must be ignored"
        )));
    }
}
