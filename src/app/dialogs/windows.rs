use super::{Context, TinyShell, Window};

fn activate_existing_settings(
    handle: gpui::AnyWindowHandle,
    window: &mut Window,
    cx: &mut gpui::App,
) -> bool {
    if handle == window.window_handle() {
        window.activate_window();
        return true;
    }
    handle
        .update(cx, |_, window, _| window.activate_window())
        .is_ok()
}

impl TinyShell {
    pub(crate) fn show_quick_connection_manager_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(handle) = self.auxiliary_windows.connection_manager.handle {
            if handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
            {
                return;
            }
            self.auxiliary_windows.connection_manager.handle = None;
        }
        if self.auxiliary_windows.connection_manager.opening {
            return;
        }

        self.connection_manager_state.update(cx, move |state, _| {
            state.query.clear();
            state.show_deleted = false;
            state.selected = None;
        });

        let owner = cx.entity();
        self.auxiliary_windows.connection_manager.opening = true;
        window.defer(cx, move |_, cx| {
            let manager_window = crate::app::connection_manager::window::open(owner.clone(), cx);
            owner.update(cx, |this, cx| {
                this.auxiliary_windows.connection_manager.handle = manager_window;
                this.auxiliary_windows.connection_manager.opening = false;
                cx.notify();
            });
        });
    }

    pub(crate) fn show_settings_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.auxiliary_windows.settings.opening {
            return;
        }

        let owner = cx.entity();
        let main_window = window.window_handle();
        self.auxiliary_windows.settings.opening = true;
        window.defer(cx, move |window, cx| {
            // Native activation can synchronously redraw another window. Release the owner
            // and finish the input event before activating a view that reads the same owner.
            let existing = owner.read(cx).auxiliary_windows.settings.handle;
            let settings_window = existing
                .filter(|handle| activate_existing_settings(*handle, window, cx))
                .or_else(|| {
                    let config = owner.read(cx).config.clone();
                    crate::app::settings_window::open(owner.clone(), config, main_window, cx)
                });
            owner.update(cx, |this, cx| {
                this.auxiliary_windows.settings.handle = settings_window;
                this.auxiliary_windows.settings.opening = false;
                cx.notify();
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Render, Styled as _, TestAppContext, VisualTestContext};

    struct EmptyWindow;
    impl Render for EmptyWindow {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui::IntoElement {
            gpui::div().size_full()
        }
    }

    #[gpui::test]
    fn settings_jump_from_its_own_window_reuses_the_current_window(cx: &mut TestAppContext) {
        let target = cx.add_window(|_, _| EmptyWindow);
        let mut visual = VisualTestContext::from_window(target.into(), cx);
        visual.update(|window, cx| {
            assert!(
                activate_existing_settings(target.into(), window, cx),
                "an in-window settings jump must not treat its borrowed native window as closed"
            );
        });
        assert_eq!(cx.windows().len(), 1);
    }

    #[gpui::test]
    fn stale_settings_handle_is_rejected_for_recreation(cx: &mut TestAppContext) {
        let parent = cx.add_window(|_, _| EmptyWindow);
        let target = cx.add_window(|_, _| EmptyWindow);
        target
            .update(cx, |_, window, _| window.remove_window())
            .unwrap();
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(parent.into(), cx);
        visual.update(|window, cx| assert!(!activate_existing_settings(target.into(), window, cx)));
    }
}
