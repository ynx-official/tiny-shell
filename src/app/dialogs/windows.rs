use super::*;

impl TinyShell {
    #[allow(dead_code)]
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

        let groups = self.config.connection_groups().to_vec();
        self.connection_manager_state.update(cx, move |state, _| {
            state.query.clear();
            state.expanded = groups.into_iter().collect();
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
        if let Some(handle) = self.auxiliary_windows.settings.handle {
            if handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
            {
                return;
            }
            self.auxiliary_windows.settings.handle = None;
        }
        if self.auxiliary_windows.settings.opening {
            return;
        }

        let owner = cx.entity();
        let config = self.config.clone();
        let main_window = window.window_handle();
        self.auxiliary_windows.settings.opening = true;
        window.defer(cx, move |_window, cx| {
            let settings_window =
                crate::app::settings_window::open(owner.clone(), config, main_window, cx);
            owner.update(cx, |this, cx| {
                this.auxiliary_windows.settings.handle = settings_window;
                this.auxiliary_windows.settings.opening = false;
                cx.notify();
            });
        });
    }
}
