//! Shared presentation for short, two-action confirmations. Business callbacks remain at the call site.

use std::{cell::Cell, rc::Rc};

use gpui::{
    App, ClickEvent, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _, Pixels,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px, rgb,
};
use gpui_component::{
    ActiveTheme as _, Colorize as _, WindowExt as _,
    dialog::{ConfirmDialog, Dialog},
    h_flex,
    scroll::ScrollableElement as _,
};
use rust_i18n::t;

type Action = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) -> bool>;
type OnClose = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

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
    completed: Rc<Cell<bool>>,
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
            completed: Rc::new(Cell::new(false)),
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
        let scale = (window.rem_size() / px(14.)).max(1.);
        let paddings = gpui_component::window_paddings(window);
        let width = confirmation_width(
            window.viewport_size().width - paddings.left - paddings.right,
            scale,
        );
        let inner_width = (width - px(50.) * scale).max(px(1.));
        let title_height = measure_text(
            &self.title,
            px(17.) * scale,
            px(24.) * scale,
            inner_width,
            true,
            window,
        );
        let body_height = measure_text(
            &self.description,
            px(13.) * scale,
            px(20.) * scale,
            (inner_width - px(12.) * scale).max(px(1.)),
            false,
            window,
        );
        let button_widths = [&self.cancel_label, &self.confirm_label].map(|label| {
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
        });
        let stacked = button_widths[0] + button_widths[1] + px(8.) * scale > inner_width;
        let preferred_height = content_height(title_height, body_height, scale)
            + if stacked { px(40.) * scale } else { px(0.) };
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
        let cancel = self.button(false, scale, window, cx);
        let confirm = self.button(true, scale, window, cx);
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
            .keyboard(self.keyboard)
            .title(
                div()
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
        complete_once(&self.completed, || {
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
            }
        });
    }

    fn button(
        &self,
        confirm: bool,
        scale: f32,
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
            cx.theme().foreground
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
            .track_focus(&focus.tab_index(if confirm { 1 } else { 0 }).tab_stop(true))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .min_w(px(68.) * scale)
            .h(px(32.) * scale)
            .px(px(16.) * scale)
            .rounded(px(5.))
            .border_1()
            .border_color(if confirm {
                background
            } else {
                cx.theme().border
            })
            .bg(background)
            .text_color(foreground)
            .text_size(px(14.) * scale)
            .line_height(px(20.) * scale)
            .hover(move |style| style.bg(hover))
            .active(move |style| style.bg(hover.darken(0.04)))
            .when(focused, |button| {
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

fn complete_once(completed: &Cell<bool>, action: impl FnOnce() -> bool) -> bool {
    if completed.replace(true) {
        return false;
    }
    let accepted = action();
    if !accepted {
        completed.set(false);
    }
    accepted
}

fn content_height(title: Pixels, description: Pixels, scale: f32) -> Pixels {
    // Padding + title/body gap + footer gap + control, with the native border accounted for.
    title + description + px(48. + 8. + 24. + 32. + 2.) * scale
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let completed = Cell::new(false);
        let count = Cell::new(0);
        let action = || {
            count.set(count.get() + 1);
            true
        };
        assert!(complete_once(&completed, action));
        assert!(!complete_once(&completed, action));
        assert_eq!(count.get(), 1);
    }

    #[test]
    fn veto_keeps_the_dialog_available_for_retry_or_cancel() {
        let completed = Cell::new(false);
        assert!(!complete_once(&completed, || false));
        assert!(!completed.get());
        assert!(complete_once(&completed, || true));
    }
}
