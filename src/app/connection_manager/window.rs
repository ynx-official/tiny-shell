use gpui::{
    AnyWindowHandle, App, AppContext as _, Bounds, Context, Entity, FocusHandle, Focusable,
    InteractiveElement as _, ParentElement as _, Render, Styled, Window, WindowOptions, point, px,
    size,
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

pub(crate) fn window_options(cx: &App) -> WindowOptions {
    let mut options = WindowOptions {
        is_movable: true,
        is_resizable: true,
        is_minimizable: true,
        window_min_size: Some(size(px(560.), px(360.))),
        ..Default::default()
    };

    if let Some(display) = cx.displays().first().cloned() {
        let display_bounds = display.bounds();
        let window_size = size(
            px(600.).min(display_bounds.size.width * 0.72),
            px(400.).min(display_bounds.size.height * 0.62),
        );
        let origin = point(
            display_bounds.origin.x + (display_bounds.size.width - window_size.width) / 2.,
            display_bounds.origin.y + (display_bounds.size.height - window_size.height) / 2.,
        );
        options.window_bounds = Some(gpui::WindowBounds::Windowed(Bounds::new(
            origin,
            window_size,
        )));
    }

    #[cfg(not(target_os = "macos"))]
    if let Ok(image) =
        image::load_from_memory(include_bytes!("../../../assets/icons/tiny-shell.png"))
    {
        options.icon = Some(std::sync::Arc::new(image.into_rgba8()));
    }

    options
}

pub(crate) fn open(owner: Entity<TinyShell>, cx: &mut App) -> Option<AnyWindowHandle> {
    let options = window_options(cx);
    let owner_for_window = owner.clone();
    let opened = cx.open_window(options, move |window, cx| {
        window.set_window_title(t!("quick_connection_title").as_ref());

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
            None
        }
    }
}
