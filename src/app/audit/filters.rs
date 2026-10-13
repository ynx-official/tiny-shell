use super::*;
use gpui::{IntoElement as _, ParentElement as _, Styled as _, div};
use gpui_component::{
    Disableable as _, IconName, Sizable as _,
    button::Button,
    h_flex,
    menu::{DropdownMenu as _, PopupMenuItem},
};
use std::collections::BTreeMap;
use tiny_shell_mcp::audit::Event;

fn extend_conversation_choices(choices: &mut BTreeMap<String, String>, events: &[Event]) {
    // Tool events are often newer than the model snapshot. Resolve titles before ID fallbacks.
    // Existing choices are live chat titles, so a saved snapshot must not undo a rename.
    for event in events {
        if let Some(id) = &event.context.conversation_id
            && let Some(title) = event.details["request"]["conversation_title"]
                .as_str()
                .filter(|title| !title.trim().is_empty())
        {
            choices
                .entry(id.clone())
                .or_insert_with(|| title.to_owned());
        }
    }
    for event in events {
        if let Some(id) = &event.context.conversation_id {
            choices.entry(id.clone()).or_insert_with(|| id.clone());
        }
    }
}

fn menu(
    id: &'static str,
    label: String,
    icon: IconName,
    choices: Vec<(Option<String>, String)>,
    panel: Entity<AuditPanel>,
    apply: fn(&mut Filter, Option<String>),
    loading: bool,
) -> impl gpui::IntoElement {
    div().flex_1().min_w(px(112.)).child(
        Button::new(id)
            .small()
            .outline()
            .w_full()
            .icon(icon)
            .tooltip(label.clone())
            .child(div().min_w_0().flex_1().text_ellipsis().child(label))
            .disabled(loading)
            .dropdown_menu(move |mut menu, window, _| {
                for (value, label) in &choices {
                    let value = value.clone();
                    menu = menu.item(PopupMenuItem::new(label.clone()).on_click(
                        window.listener_for(&panel, move |this, _, _, cx| {
                            apply(&mut this.filter, value.clone());
                            this.selection = None;
                            this.filter.before_sequence = None;
                            this.load(cx);
                        }),
                    ));
                }
                menu
            }),
    )
}
impl AuditPanel {
    pub(super) fn extra_filters(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let panel = cx.entity();
        let mut targets: BTreeMap<String, String> = self.targets.iter().cloned().collect();
        let mut clients = BTreeMap::new();
        let mut conversations = BTreeMap::new();
        if let Ok(service) = crate::app::ai::service() {
            for c in service.snapshot().conversations {
                conversations.insert(c.id, c.title);
            }
        }
        for event in &self.page.events {
            if let Some(id) = &event.context.target_id {
                targets.entry(id.clone()).or_insert_with(|| {
                    event
                        .context
                        .target_name
                        .clone()
                        .unwrap_or_else(|| id.clone())
                });
            }
            if let Some(id) = &event.context.client_id {
                clients.entry(id.clone()).or_insert_with(|| {
                    event
                        .context
                        .client_name
                        .clone()
                        .unwrap_or_else(|| id.clone())
                });
            }
        }
        extend_conversation_choices(&mut conversations, &self.page.events);
        let choices = |items: BTreeMap<String, String>, key: &str| {
            std::iter::once((None, t!(key).to_string()))
                .chain(items.into_iter().map(|(id, label)| (Some(id), label)))
                .collect::<Vec<_>>()
        };
        let target_label = self
            .filter
            .target_id
            .as_ref()
            .and_then(|id| targets.get(id))
            .cloned()
            .unwrap_or_else(|| t!("audit_targets_all").to_string());
        let client_label = self
            .filter
            .client_id
            .as_ref()
            .and_then(|id| clients.get(id))
            .cloned()
            .unwrap_or_else(|| t!("audit_clients_all").to_string());
        let conversation_label = self
            .filter
            .conversation_id
            .as_ref()
            .and_then(|id| conversations.get(id))
            .cloned()
            .unwrap_or_else(|| t!("audit_conversations_all").to_string());
        let state_label = self
            .filter
            .state
            .as_deref()
            .map(super::presentation::state)
            .unwrap_or_else(|| t!("audit_states_all").to_string());
        let states = [
            "running",
            "queued",
            "awaiting_user",
            "succeeded",
            "completed",
            "failed",
            "cancelled",
            "not_executed",
            "outcome_unknown",
            "approved",
            "rejected",
            "expired",
            "invalidated",
            "cached",
            "interrupted",
        ];
        let mut row = h_flex()
            .gap_2()
            .flex_wrap()
            .child(menu(
                "audit-target-filter",
                target_label,
                IconName::SquareTerminal,
                choices(targets, "audit_targets_all"),
                panel.clone(),
                |f, id| f.target_id = id,
                self.loading,
            ))
            .child(menu(
                "audit-client-filter",
                client_label,
                IconName::Network,
                choices(clients, "audit_clients_all"),
                panel.clone(),
                |f, id| f.client_id = id,
                self.loading,
            ))
            .child(menu(
                "audit-conversation-filter",
                conversation_label,
                IconName::Bot,
                choices(conversations, "audit_conversations_all"),
                panel.clone(),
                |f, id| f.conversation_id = id,
                self.loading,
            ))
            .child(menu(
                "audit-state-filter",
                state_label,
                IconName::CircleCheck,
                std::iter::once((None, t!("audit_states_all").to_string()))
                    .chain(
                        states
                            .into_iter()
                            .map(|s| (Some(s.to_owned()), super::presentation::state(s))),
                    )
                    .collect(),
                panel.clone(),
                |f, state| f.state = state,
                self.loading,
            ));
        let time_label = t!(if self.filter.since_ms.is_some() {
            "audit_time_recent"
        } else {
            "audit_time_all"
        })
        .to_string();
        row = row.child(
            div().flex_1().min_w(px(112.)).child(
                Button::new("audit-time-filter")
                    .small()
                    .outline()
                    .w_full()
                    .icon(IconName::Calendar)
                    .disabled(self.loading)
                    .tooltip(time_label.clone())
                    .child(div().min_w_0().flex_1().text_ellipsis().child(time_label))
                    .dropdown_menu(move |mut menu, window, _| {
                        for (days, key) in [
                            (None, "audit_time_all"),
                            (Some(1), "audit_time_day"),
                            (Some(7), "audit_time_week"),
                            (Some(30), "audit_time_month"),
                            (Some(90), "audit_time_quarter"),
                        ] {
                            menu = menu.item(PopupMenuItem::new(t!(key).to_string()).on_click(
                                window.listener_for(&panel, move |this, _, _, cx| {
                                    this.filter.since_ms = days.map(|days| {
                                        tiny_shell_mcp::audit::now_ms() - days * 86_400_000
                                    });
                                    this.filter.before_sequence = None;
                                    this.selection = None;
                                    this.load(cx);
                                }),
                            ));
                        }
                        menu
                    }),
            ),
        );
        row.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tiny_shell_mcp::audit::Context as AuditContext;

    #[test]
    fn deleted_conversation_uses_title_snapshot_even_when_tool_event_is_newer() {
        let context = AuditContext::client("ai:synthetic-conversation");
        let events = [
            Event::new(
                context.clone(),
                "execution_result",
                "run_command",
                "succeeded",
                json!({}),
            ),
            Event::new(
                context.clone(),
                "model_finished",
                "model",
                "completed",
                json!({"request":{"conversation_title":"Saved inspection title"}}),
            ),
            Event::new(
                context,
                "model_finished",
                "model",
                "completed",
                json!({"request":{"conversation_title":"Earlier title"}}),
            ),
        ];
        let mut choices = BTreeMap::new();
        extend_conversation_choices(&mut choices, &events);
        assert_eq!(
            choices.get("synthetic-conversation").map(String::as_str),
            Some("Saved inspection title")
        );
        choices.insert(
            "synthetic-conversation".into(),
            "Current renamed title".into(),
        );
        extend_conversation_choices(&mut choices, &events);
        assert_eq!(
            choices.get("synthetic-conversation").map(String::as_str),
            Some("Current renamed title")
        );
    }
}
