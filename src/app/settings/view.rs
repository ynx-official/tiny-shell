use std::time::Duration;

use crate::{TinyShell, app::settings::form::SettingsInputs};
use gpui::{
    Animation, AnimationExt as _, ElementId, InteractiveElement as _, IntoElement,
    ParentElement as _, Styled as _, div, px,
};
use gpui_component::{ActiveTheme as _, setting::Settings};

impl TinyShell {
    pub(crate) fn render_settings_content(
        &self,
        view: &gpui::Entity<Self>,
        settings_id: &'static str,
        focus_handle: &gpui::FocusHandle,
        inputs: &SettingsInputs,
        cx: &gpui::App,
    ) -> gpui::AnyElement {
        let settings_view = view.clone();
        let focus_handle = focus_handle.clone();

        div()
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .size_full()
                    .min_w_0()
                    .min_h_0()
                    .track_focus(&focus_handle)
                    .on_key_down({
                        let view = view.clone();
                        move |event: &gpui::KeyDownEvent, window, cx| {
                            view.update(cx, |this, cx| {
                                this.handle_settings_keybinding_input(event, window, cx);
                            });
                        }
                    })
                    .on_mouse_down_out({
                        let view = view.clone();
                        move |_, _, cx| {
                            view.update(cx, |this, cx| {
                                this.cancel_settings_keybinding_recording(cx);
                            });
                        }
                    })
                    .child(
                        Settings::new(settings_id)
                            .sidebar_width(px(180.))
                            .sidebar_style(div().bg(cx.theme().background).style())
                            .page(super::appearance::page(&settings_view))
                            .page(super::terminal::page(&settings_view))
                            .page(super::workspace::page(&settings_view))
                            .page(super::sync::page(&settings_view, inputs.sync.clone()))
                            .page(super::proxy::page(&settings_view, inputs.proxy.clone()))
                            .page(super::keybindings::page(self, view, &focus_handle))
                            .page(super::update::page(
                                &settings_view,
                                inputs.update.interval_hours.clone(),
                            ))
                            .page(super::about::page()),
                    ),
            )
            .with_animation(
                ElementId::NamedInteger("settings-content-fade".into(), self.main_view_key()),
                Animation::new(Duration::from_millis(200)).with_easing(gpui::ease_out_quint()),
                |element, delta| element.opacity(delta * delta),
            )
            .into_any_element()
    }
}
