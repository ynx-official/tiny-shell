use super::*;

impl TinyShell {
    pub(crate) fn confirm_connection_group_dialog(
        &mut self,
        token: crate::app::DialogToken,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = self
            .connection_inputs
            .connection_group_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        if name.is_empty() {
            return;
        }
        let mut staged = self.config.clone();
        let next_filter = if let Some(old_name) = self.editing_connection_group.clone() {
            let full_name = self
                .connection_group_parent
                .as_deref()
                .map(|parent| format!("{parent}/{name}"))
                .unwrap_or(name.clone());
            staged.rename_connection_group(&old_name, full_name.clone());
            if self.connection_group_filter.as_deref() == Some(old_name.as_str()) {
                Some(full_name)
            } else {
                self.connection_group_filter.clone()
            }
        } else {
            let full_name = self
                .connection_group_parent
                .as_deref()
                .map(|parent| format!("{parent}/{name}"))
                .unwrap_or(name.clone());
            staged.add_connection_group(full_name.clone());
            Some(full_name)
        };
        self.commit_staged_config_in_window_async(
            staged,
            window,
            move |this, window, cx| {
                this.connection_group_filter = next_filter;
                crate::feedback::Feedback::success(
                    window,
                    cx,
                    format!("{} · {}", t!("connection_group_dialog_title"), t!("save")),
                );
                this.dismiss_modal_dialog(token, window, cx);
                this.editing_connection_group = None;
                this.connection_group_parent = None;
                cx.notify();
            },
            |this, error, window, cx| {
                tracing::warn!("failed to save connection group: {error:#}");
                let message = t!("config_save_failed", error = format!("{error:#}")).to_string();
                this.status = message.clone().into();
                crate::feedback::Feedback::error(window, cx, message);
                cx.notify();
            },
            cx,
        );
    }
}
