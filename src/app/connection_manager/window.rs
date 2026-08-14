use gpui::{
    AnyWindowHandle, App, AppContext as _, Context, Entity, FocusHandle, Focusable,
    InteractiveElement as _, ParentElement as _, Render, Styled, Window, WindowOptions, px, size,
};
use gpui_component::{
    ActiveTheme as _, Root,
    input::{InputEvent, InputState},
    v_flex,
};
use rust_i18n::t;

use crate::TinyShell;

pub(crate) struct ConnectionManagerWindow {
    owner: Entity<TinyShell>,
    search_input: Entity<InputState>,
    focus_handle: FocusHandle,
    _owner_subscription: gpui::Subscription,
    _search_subscription: gpui::Subscription,
}

impl ConnectionManagerWindow {
    fn new(owner: Entity<TinyShell>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t!("quick_connection_search").to_string())
        });
        let owner_subscription = cx.observe(&owner, |_, _, cx| cx.notify());
        let search_subscription =
            cx.subscribe_in(&search_input, window, |_, _, _: &InputEvent, _, cx| {
                cx.notify()
            });

        Self {
            owner,
            search_input,
            focus_handle: cx.focus_handle(),
            _owner_subscription: owner_subscription,
            _search_subscription: search_subscription,
        }
    }
}

impl Render for ConnectionManagerWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .p_4()
            .track_focus(&self.focus_handle)
            .on_key_down(|event: &gpui::KeyDownEvent, window, cx| {
                if event.keystroke.key.as_str() == "escape" {
                    window.prevent_default();
                    cx.stop_propagation();
                    crate::app::deregister_auxiliary_window(window.window_handle());
                    window.remove_window();
                }
            })
            .child(super::view::render(
                &self.owner,
                &self.search_input,
                window,
                cx,
            ))
    }
}

pub(crate) fn window_options(cx: &mut App) -> WindowOptions {
    crate::app::platform::auxiliary_window_options(
        cx,
        crate::app::platform::AuxiliaryWindowSpec::new(size(px(600.), px(400.)))
            .with_min_size(size(px(560.), px(360.)))
            .with_max_ratio(0.72, 0.62),
    )
}

pub(crate) fn open(owner: Entity<TinyShell>, cx: &mut App) -> Option<AnyWindowHandle> {
    let options = window_options(cx);
    let owner_id = owner.read(cx).session_owner_id;
    let owner_for_window = owner.clone();
    let opened = cx.open_window(options, move |window, cx| {
        window.set_window_title(t!("quick_connection_title").as_ref());
        let window_handle = window.window_handle();
        crate::app::register_auxiliary_window(window_handle, owner_id);

        let manager =
            cx.new(|cx| ConnectionManagerWindow::new(owner_for_window.clone(), window, cx));
        let search_input = manager.read(cx).search_input.clone();
        let owner_for_close = owner_for_window.clone();
        window.on_window_should_close(cx, move |_, cx| {
            owner_for_close.update(cx, |this, cx| {
                this.auxiliary_windows.connection_manager.handle = None;
                this.auxiliary_windows.connection_manager.opening = false;
                cx.notify();
            });
            crate::app::deregister_auxiliary_window(window_handle);
            true
        });
        window.defer(cx, move |window, cx| {
            window.activate_window();
            let focus_handle = search_input.read(cx).focus_handle(cx);
            window.focus(&focus_handle, cx);
        });

        cx.new(|cx| Root::new(manager, window, cx))
    });

    match opened {
        Ok(handle) => Some(handle.into()),
        Err(error) => {
            tracing::error!("failed to open connection manager window: {error:?}");
            crate::feedback::Feedback::show_for_owner(
                &owner,
                cx,
                crate::feedback::FeedbackKind::Error,
                t!(
                    "connection_manager_action_failed",
                    error = format!("{error:?}")
                )
                .to_string(),
            );
            None
        }
    }
}
