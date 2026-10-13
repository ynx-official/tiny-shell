//! Derive an ordered tool trace without changing the model's conversation transcript.
use crate::ai::Conversation;
use serde_json::Value;

#[path = "tool_steps/review.rs"]
pub(super) mod review;
#[path = "tool_steps/view.rs"]
pub(super) mod view;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct StepKey {
    pub message: usize,
    pub call: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ToolState {
    Queued,
    Running,
    AwaitingApproval,
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
    Unknown,
    Interrupted,
    CancelRequested,
    Skipped,
}
#[derive(Clone, PartialEq)]
pub(super) struct ToolStep {
    pub key: StepKey,
    pub call_id: Option<String>,
    pub name: String,
    pub arguments: Option<Value>,
    pub result: Option<String>,
    pub state: ToolState,
    pub task_id: Option<String>,
    pub target: Option<String>,
}
#[derive(Clone, PartialEq)]
pub(super) struct ToolGroup {
    pub key: usize,
    pub steps: Vec<ToolStep>,
}
pub(super) enum ChatItem {
    Message(usize),
    Tools(ToolGroup),
}

pub(super) fn project(conversation: &Conversation) -> Vec<ChatItem> {
    let mut items = Vec::new();
    let mut pending: Option<ToolGroup> = None;
    let mut used = std::collections::HashSet::new();
    for (index, message) in conversation.messages.iter().enumerate() {
        if message.role == "tool" {
            let group = pending.get_or_insert_with(|| ToolGroup {
                key: index,
                steps: Vec::new(),
            });
            let row = group
                .steps
                .iter_mut()
                .rev()
                .find(|step| step.result.is_none() && step.call_id == message.tool_call_id);
            if let Some(step) = row {
                step.state = result_state(&step.name, &message.content, step.state);
                step.result = Some(message.content.clone());
            } else {
                // Older or incomplete transcripts must keep their output accessible.
                group.steps.push(ToolStep {
                    key: StepKey {
                        message: index,
                        call: 0,
                    },
                    call_id: message.tool_call_id.clone(),
                    name: String::new(),
                    arguments: None,
                    result: Some(message.content.clone()),
                    state: result_state("", &message.content, ToolState::Unknown),
                    task_id: None,
                    target: None,
                });
            }
            continue;
        }
        if message.role == "user" || !message.content.trim().is_empty() {
            if let Some(group) = pending.take() {
                items.push(ChatItem::Tools(group));
            }
            items.push(ChatItem::Message(index));
        }
        if message.tool_calls.is_empty() {
            continue;
        }
        let group = pending.get_or_insert_with(|| ToolGroup {
            key: index,
            steps: Vec::new(),
        });
        for (ordinal, call) in message.tool_calls.iter().enumerate() {
            let name = call["function"]["name"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            let call_id = call["id"].as_str().map(str::to_owned);
            let exact = conversation
                .activities
                .iter()
                .enumerate()
                .find(|(i, activity)| {
                    !used.contains(i)
                        && activity.message_index == Some(index)
                        && activity.tool_call_id == call_id
                        && activity.name == name
                });
            let activity = exact.or_else(|| {
                conversation
                    .activities
                    .iter()
                    .enumerate()
                    .find(|(i, activity)| {
                        !used.contains(i)
                            && activity.message_index.is_none()
                            && activity.name == name
                    })
            });
            if let Some((i, _)) = activity {
                used.insert(i);
            }
            let activity = activity.map(|(_, a)| a);
            // Calls from an earlier stopped turn must not inherit the latest turn's running state.
            let in_current_turn = !conversation.messages[index + 1..]
                .iter()
                .any(|m| m.role == "user");
            let active = conversation.state.active() && in_current_turn;
            let mut state = activity.map(|a| task_state(&a.state)).unwrap_or_else(|| {
                if active {
                    ToolState::Queued
                } else {
                    ToolState::Skipped
                }
            });
            if !active && state.pending() {
                state = ToolState::Interrupted;
            }
            group.steps.push(ToolStep {
                key: StepKey {
                    message: index,
                    call: ordinal,
                },
                call_id,
                name,
                arguments: call["function"]["arguments"].as_str().map(|args| {
                    serde_json::from_str(args).unwrap_or_else(|_| Value::String(args.into()))
                }),
                result: None,
                state,
                task_id: activity.and_then(|a| a.task_id.clone()),
                target: conversation.target.as_ref().map(|t| t.name.clone()),
            });
        }
    }
    if let Some(group) = pending {
        items.push(ChatItem::Tools(group));
    }
    items
}

impl ToolState {
    pub fn pending(self) -> bool {
        matches!(
            self,
            Self::Queued | Self::Running | Self::AwaitingApproval | Self::CancelRequested
        )
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::Queued => "mcp_state_queued",
            Self::Running => "mcp_state_running",
            Self::AwaitingApproval => "mcp_state_awaiting_user",
            Self::Succeeded => "mcp_state_succeeded",
            Self::Failed => "mcp_state_failed",
            Self::Cancelled => "mcp_state_cancelled",
            Self::TimedOut => "mcp_state_timed_out",
            Self::Unknown => "mcp_state_outcome_unknown",
            Self::Interrupted => "ai_interrupted",
            Self::CancelRequested => "mcp_state_cancel_requested",
            Self::Skipped => "ai_step_skipped",
        }
    }
}
impl ToolGroup {
    pub fn state(&self) -> ToolState {
        let priority = [
            ToolState::AwaitingApproval,
            ToolState::CancelRequested,
            ToolState::Running,
            ToolState::Queued,
            ToolState::Failed,
            ToolState::TimedOut,
            ToolState::Unknown,
            ToolState::Interrupted,
            ToolState::Cancelled,
            ToolState::Skipped,
        ];
        priority
            .into_iter()
            .find(|state| self.steps.iter().any(|s| s.state == *state))
            .unwrap_or(ToolState::Succeeded)
    }
}
impl ToolStep {
    pub fn label(&self) -> String {
        let key = match self.name.as_str() {
            "get_status" => "ai_step_get_status",
            "list_connections" => "ai_step_list_connections",
            "open_session" => "ai_step_open_session",
            "close_session" => "ai_step_close_session",
            "run_command" => "ai_step_run_command",
            "list_dir" | "list_directory" => "ai_step_list_dir",
            "read_file" => "ai_step_read_file",
            "write_file" => "ai_step_write_file",
            "get_task" => "ai_step_get_task",
            "cancel_task" => "ai_step_cancel_task",
            "" => "ai_tool_result",
            _ => return self.name.clone(),
        };
        rust_i18n::t!(key).to_string()
    }
    pub fn preview(&self) -> Option<String> {
        let value = match self.name.as_str() {
            "run_command" => self.arguments.as_ref().and_then(|a| a["command"].as_str()),
            "read_file" | "write_file" | "list_dir" | "list_directory" => {
                self.arguments.as_ref().and_then(|a| a["path"].as_str())
            }
            "open_session" | "close_session" => self.target.as_deref(),
            _ => None,
        }?;
        let line = value.lines().find(|line| !line.trim().is_empty())?.trim();
        Some(if line.chars().count() > 160 {
            format!("{}…", line.chars().take(160).collect::<String>())
        } else {
            line.to_owned()
        })
    }
}
fn task_state(state: &str) -> ToolState {
    match state {
        "queued" => ToolState::Queued,
        "running" => ToolState::Running,
        "awaiting_user" => ToolState::AwaitingApproval,
        "succeeded" => ToolState::Succeeded,
        "failed" => ToolState::Failed,
        "cancelled" => ToolState::Cancelled,
        "timed_out" => ToolState::TimedOut,
        "outcome_unknown" => ToolState::Unknown,
        "interrupted" => ToolState::Interrupted,
        "cancel_requested" => ToolState::CancelRequested,
        "not_executed" => ToolState::Skipped,
        _ => ToolState::Unknown,
    }
}
fn result_state(name: &str, text: &str, fallback: ToolState) -> ToolState {
    let Ok(result) = serde_json::from_str::<Value>(text) else {
        return if fallback.pending() || fallback == ToolState::Skipped {
            ToolState::Unknown
        } else {
            fallback
        };
    };
    // A task lookup/cancellation is itself successful even if the inspected task failed.
    if !matches!(name, "get_task" | "cancel_task")
        && let Some(state) = result["state"].as_str()
    {
        return task_state(state);
    }
    if result.get("error").is_some_and(|error| !error.is_null()) {
        return ToolState::Failed;
    }
    if result.is_object() {
        ToolState::Succeeded
    } else {
        ToolState::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{Activity, Message, Settings, TurnState};
    use serde_json::json;

    fn call(id: &str, name: &str, args: Value, content: &str) -> Message {
        Message {
            role: "assistant".into(),
            content: content.into(),
            tool_call_id: None,
            tool_calls: vec![
                json!({"id":id,"function":{"name":name,"arguments":args.to_string()}}),
            ],
        }
    }
    fn result(id: &str, value: Value) -> Message {
        Message {
            role: "tool".into(),
            content: value.to_string(),
            tool_call_id: Some(id.into()),
            tool_calls: Vec::new(),
        }
    }
    #[test]
    fn consecutive_calls_form_one_trace_before_the_final_answer() {
        let mut c = Conversation::new(None, &Settings::default());
        c.messages = vec![
            Message::text("user", "检查 nginx"),
            call("status", "get_status", json!({}), "开始检查"),
            result("status", json!({"mode":"full"})),
            call(
                "open",
                "open_session",
                json!({"connection_id":"fixture"}),
                "",
            ),
            result(
                "open",
                json!({"state":"succeeded","result":{"session_id":"fixture-session"}}),
            ),
            call(
                "run",
                "run_command",
                json!({"command":"systemctl is-active nginx"}),
                "",
            ),
            result("run", json!({"state":"succeeded","result":{"exit_code":0}})),
            Message::text("assistant", "nginx 已启动"),
        ];
        let items = project(&c);
        assert_eq!(
            items.len(),
            4,
            "tool results and statuses must appear once, in one inline trace"
        );
        assert!(matches!(items[0], ChatItem::Message(0)));
        assert!(matches!(items[1], ChatItem::Message(1)));
        let ChatItem::Tools(group) = &items[2] else {
            panic!("missing tool trace")
        };
        assert_eq!(group.steps.len(), 3);
        assert!(group.steps.iter().all(|s| s.state == ToolState::Succeeded));
        assert!(matches!(items[3], ChatItem::Message(7)));
    }
    #[test]
    fn a_failed_tool_result_is_not_reported_as_success() {
        let mut c = Conversation::new(None, &Settings::default());
        c.messages = vec![
            call("run", "run_command", json!({"command":"false"}), ""),
            result("run", json!({"error":"PERMISSION_DENIED"})),
        ];
        let items = project(&c);
        let ChatItem::Tools(group) = &items[0] else {
            panic!("missing tool trace")
        };
        assert_eq!(group.steps[0].state, ToolState::Failed);
    }
    #[test]
    fn guarded_and_uncertain_operations_keep_their_explicit_state_despite_errors() {
        for (state, error, expected) in [
            ("not_executed", "AI_TOOL_BUDGET", ToolState::Skipped),
            ("outcome_unknown", "OUTCOME_UNKNOWN", ToolState::Unknown),
            ("timed_out", "TIMEOUT", ToolState::TimedOut),
        ] {
            let mut c = Conversation::new(None, &Settings::default());
            c.messages = vec![
                call("run", "run_command", json!({}), ""),
                result("run", json!({"state":state,"error":{"code":error}})),
            ];
            assert_eq!(trace(&c).steps[0].state, expected);
        }
    }

    fn trace(c: &Conversation) -> ToolGroup {
        project(c)
            .into_iter()
            .find_map(|item| match item {
                ChatItem::Tools(group) => Some(group),
                _ => None,
            })
            .unwrap()
    }
    #[test]
    fn reused_call_ids_keep_completed_and_awaiting_steps_distinct() {
        let mut c = Conversation::new(None, &Settings::default());
        c.state = TurnState::AwaitingApproval;
        c.messages = vec![
            call("same", "run_command", json!({"command":"true"}), ""),
            result("same", json!({"state":"succeeded"})),
            call("same", "run_command", json!({"command":"false"}), ""),
        ];
        c.activities = vec![
            Activity {
                name: "run_command".into(),
                task_id: Some("old".into()),
                state: "succeeded".into(),
                message_index: Some(0),
                tool_call_id: Some("same".into()),
            },
            Activity {
                name: "run_command".into(),
                task_id: Some("awaiting".into()),
                state: "awaiting_user".into(),
                message_index: Some(2),
                tool_call_id: Some("same".into()),
            },
        ];
        let group = trace(&c);
        assert_eq!(group.steps.len(), 2);
        assert_eq!(group.steps[0].state, ToolState::Succeeded);
        assert_eq!(group.steps[1].state, ToolState::AwaitingApproval);
        assert_eq!(group.steps[1].task_id.as_deref(), Some("awaiting"));
        assert_eq!(group.state(), ToolState::AwaitingApproval);
    }
    #[test]
    fn older_activities_load_and_results_override_stale_running_states() {
        let mut c = Conversation::new(None, &Settings::default());
        c.activities = vec![
            serde_json::from_value(
                json!({"name":"run_command","task_id":"legacy","state":"running"}),
            )
            .unwrap(),
        ];
        c.messages = vec![
            call("legacy", "run_command", json!({"command":"true"}), ""),
            result("legacy", json!({"state":"succeeded"})),
        ];
        let group = trace(&c);
        assert_eq!(group.steps.len(), 1);
        assert_eq!(group.state(), ToolState::Succeeded);
        assert_eq!(group.steps[0].task_id.as_deref(), Some("legacy"));
    }
    #[test]
    fn interrupted_calls_do_not_become_running_when_a_new_turn_starts() {
        let mut c = Conversation::new(None, &Settings::default());
        c.state = TurnState::Generating;
        c.messages = vec![
            call("old", "run_command", json!({}), ""),
            Message::text("user", "继续解释"),
        ];
        c.activities = vec![Activity {
            name: "run_command".into(),
            task_id: None,
            state: "interrupted".into(),
            message_index: Some(0),
            tool_call_id: Some("old".into()),
        }];
        assert_eq!(trace(&c).steps[0].state, ToolState::Interrupted);
        c.activities.clear();
        assert_eq!(trace(&c).steps[0].state, ToolState::Skipped);
    }
    #[test]
    fn terminal_outcomes_and_unmatched_output_remain_visible() {
        for (state, expected) in [
            ("failed", ToolState::Failed),
            ("timed_out", ToolState::TimedOut),
            ("outcome_unknown", ToolState::Unknown),
            ("cancelled", ToolState::Cancelled),
        ] {
            let mut c = Conversation::new(None, &Settings::default());
            c.messages = vec![
                call("run", "run_command", json!({}), ""),
                result("run", json!({"state":state})),
            ];
            assert_eq!(trace(&c).state(), expected);
        }
        let mut c = Conversation::new(None, &Settings::default());
        c.messages = vec![Message {
            role: "tool".into(),
            content: "retained truncated output".into(),
            ..Message::default()
        }];
        let group = trace(&c);
        assert_eq!(group.steps[0].state, ToolState::Unknown);
        assert_eq!(
            group.steps[0].result.as_deref(),
            Some("retained truncated output")
        );
    }
}
