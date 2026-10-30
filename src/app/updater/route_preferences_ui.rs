//! Route edits use the existing configuration worker and acknowledge its write receipt.
use gpui::{Context, Entity, Window};
use gpui_component::input::{InputEvent, InputState};
use rust_i18n::t;

use super::{
    route_ui::routes_locked,
    routes::{RouteMode, RoutePreferences},
};
use crate::{TinyShell, feedback::Feedback, session::update_preferences::RoutePreferenceError};

pub(crate) struct PendingRouteSave {
    pub previous: RoutePreferences,
    proposed: RoutePreferences,
}

enum RouteEdit {
    Add {
        input: Entity<InputState>,
        draft: String,
    },
    Remove {
        selected: bool,
    },
    Select,
}

impl RouteEdit {
    fn success_key(&self) -> &'static str {
        match self {
            Self::Add { .. } => "update_route_added",
            Self::Remove { selected: true } => "update_route_removed_selected",
            Self::Remove { selected: false } => "update_route_removed",
            Self::Select => "update_route_selection_saved",
        }
    }
}

impl TinyShell {
    pub(crate) fn on_custom_update_route_input(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => self.update_runtime.route_form_error = None,
            InputEvent::PressEnter { .. } => {
                if !input.read(cx).value().trim().is_empty() {
                    self.add_custom_update_route(input.clone(), window, cx);
                }
                window.prevent_default();
                cx.stop_propagation();
            }
            _ => {}
        }
        cx.notify();
    }

    pub(crate) fn set_update_route_mode(
        &mut self,
        mode: RouteMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if mode == RouteMode::Custom {
            return;
        }
        let mut prefs = self.config.update_routes().clone();
        prefs.mode = mode;
        self.persist_update_route_edit(prefs, RouteEdit::Select, window, cx);
    }

    pub(crate) fn select_custom_update_route(
        &mut self,
        prefix: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.config.update_routes().with_selected_custom(prefix) {
            Ok(prefs) => {
                self.persist_update_route_edit(prefs, RouteEdit::Select, window, cx);
            }
            Err(error) => self.show_route_input_error(error, cx),
        }
    }

    pub(crate) fn add_custom_update_route(
        &mut self,
        input: Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if routes_locked(&self.update_runtime) {
            return;
        }
        let draft = input.read(cx).value().to_string();
        match self.config.update_routes().with_added_custom(&draft) {
            Ok(prefs) => {
                self.persist_update_route_edit(prefs, RouteEdit::Add { input, draft }, window, cx);
            }
            Err(error) => self.show_route_input_error(error, cx),
        }
    }

    pub(crate) fn remove_custom_update_route(
        &mut self,
        prefix: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let previous = self.config.update_routes();
        let selected = previous.mode == RouteMode::Custom
            && previous.selected_custom_prefix.as_deref() == Some(prefix);
        let prefs = previous.without_custom(prefix);
        self.persist_update_route_edit(prefs, RouteEdit::Remove { selected }, window, cx)
    }

    fn show_route_input_error(&mut self, error: RoutePreferenceError, cx: &mut Context<Self>) {
        self.update_runtime.route_form_error = Some(
            t!(match error {
                RoutePreferenceError::Invalid => "update_route_custom_invalid",
                RoutePreferenceError::Duplicate => "update_route_duplicate",
                RoutePreferenceError::Missing => "update_route_missing",
            })
            .to_string(),
        );
        cx.notify();
    }

    fn persist_update_route_edit(
        &mut self,
        prefs: RoutePreferences,
        action: RouteEdit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if routes_locked(&self.update_runtime) {
            Feedback::info(window, cx, t!("update_route_busy").to_string());
            return false;
        }
        if *self.config.update_routes() == prefs {
            Feedback::info(window, cx, t!("update_route_unchanged").to_string());
            return true;
        }
        let previous = self.config.update_routes().clone();
        let mut snapshot = self.config.clone();
        snapshot.set_update_routes(prefs.clone());
        let receipt = match self.config_repository.persist_async(snapshot) {
            Ok(receipt) => receipt,
            Err(error) => {
                let message = t!("config_save_failed", error = format!("{error:#}")).to_string();
                self.update_runtime.route_form_error = Some(message.clone());
                Feedback::error(window, cx, message);
                cx.notify();
                return false;
            }
        };
        // Keep later snapshots consistent with this queued edit, but show the previous list
        // until acknowledgement. The preference driver defers new saves while this is pending.
        self.config.set_update_routes(prefs.clone());
        self.mark_config_preferences_dirty();
        let generation = self.config_persistence.generation();
        self.update_runtime.pending_route_save = Some(PendingRouteSave {
            previous,
            proposed: prefs,
        });
        self.update_runtime.route_form_error = None;
        let target = window.window_handle();
        let save = cx
            .background_executor()
            .spawn(async move { receipt.wait() });
        cx.spawn(async move |owner, cx| {
            let result = save.await;
            let message = match &result {
                Ok(()) => t!(action.success_key()).to_string(),
                Err(error) => t!("config_save_failed", error = format!("{error:#}")).to_string(),
            };
            let updated = owner.update(cx, |this, cx| {
                let Some(pending) = this.update_runtime.pending_route_save.take() else {
                    return;
                };
                if result.is_ok() {
                    this.config_persistence.mark_saved(generation, false);
                    this.note_local_config_saved(cx);
                    this.update_runtime.status = None;
                    this.update_runtime.route_report = None;
                    this.update_runtime.route_checked_at = None;
                    this.update_runtime.active_route = None;
                    this.update_runtime.route_error = None;
                } else {
                    if *this.config.update_routes() == pending.proposed {
                        this.config.set_update_routes(pending.previous);
                        // A timed-out write may still finish. Queue the rollback after it.
                        this.mark_config_preferences_dirty();
                    }
                    this.update_runtime.route_form_error = Some(message.clone());
                }
                cx.notify();
            });
            if updated.is_err() {
                return;
            }
            // Never redirect completion feedback to a different settings/workspace window.
            let _ = target.update(cx, |_, window, cx| {
                if result.is_ok() {
                    if let RouteEdit::Add { input, draft } = action
                        && input.read(cx).value().as_ref() == draft.as_str()
                    {
                        input.update(cx, |input, cx| input.set_value("", window, cx));
                    }
                    Feedback::success(window, cx, message);
                } else {
                    Feedback::error(window, cx, message);
                }
            });
        })
        .detach();
        cx.notify();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::runtime_state::UpdateRuntimeState;

    #[test]
    fn pending_save_blocks_route_changes_probes_and_downloads() {
        let previous = RoutePreferences::default();
        let proposed = previous.with_added_custom("https://example.com/").unwrap();
        let mut runtime = UpdateRuntimeState::default();
        assert!(!routes_locked(&runtime));
        runtime.pending_route_save = Some(PendingRouteSave { previous, proposed });
        assert!(routes_locked(&runtime));
        runtime.pending_route_save = None;
        assert!(!routes_locked(&runtime));
    }
}
