use super::*;

impl TinyShell {
    pub(crate) fn confirm_connection_group_dialog(
        &mut self,
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
        if let Some(old_name) = self.editing_connection_group.clone() {
            let full_name = self
                .connection_group_parent
                .as_deref()
                .map(|parent| format!("{parent}/{name}"))
                .unwrap_or(name.clone());
            self.config
                .rename_connection_group(&old_name, full_name.clone());
            if self.connection_group_filter.as_deref() == Some(old_name.as_str()) {
                self.connection_group_filter = Some(full_name);
            }
        } else {
            let full_name = self
                .connection_group_parent
                .as_deref()
                .map(|parent| format!("{parent}/{name}"))
                .unwrap_or(name.clone());
            self.config.add_connection_group(full_name.clone());
            self.connection_group_filter = Some(full_name);
        }
        if let Err(err) = crate::app::config_persistence::save_full(&self.config) {
            tracing::warn!("failed to save connection group: {err:#}");
        }
        self.active_dialog = None;
        self.editing_connection_group = None;
        self.connection_group_parent = None;
        window.close_dialog(cx);
        cx.notify();
    }
}
