use super::*;
use tiny_shell_mcp::audit::{Category, Event};

pub(crate) fn usage_label(c: &crate::ai::Conversation) -> String {
    let summary = c.usage_summary();
    let legacy = c
        .turns
        .first()
        .map_or(!c.messages.is_empty(), |t| t.user_message > 0);
    if c.requests.is_empty() && legacy {
        return t!("audit_legacy_usage").to_string();
    }
    let mut parts = vec![
        t!("audit_total_tokens", count = summary.total).to_string(),
        t!("audit_request_count", count = summary.requests).to_string(),
    ];
    parts.extend(usage_status_labels(&summary));
    let label = parts.join(" · ");
    if legacy {
        format!("{label} · {}", t!("audit_legacy_also"))
    } else {
        label
    }
}
pub(crate) fn compact_usage_label(c: &crate::ai::Conversation) -> String {
    if c.requests.is_empty() && !c.messages.is_empty() {
        return t!("audit_usage_unreported").to_string();
    }
    let sum = c.usage_summary();
    format!(
        "{}{}",
        t!("audit_compact_tokens", count = sum.total),
        if sum.unknown > 0 || sum.pending > 0 {
            " *"
        } else {
            ""
        }
    )
}
pub(super) fn usage_status_labels(summary: &tiny_shell_mcp::audit::Summary) -> Vec<String> {
    let mut labels = Vec::new();
    if summary.pending > 0 {
        labels.push(t!("audit_pending_requests", count = summary.pending).to_string());
    }
    if summary.unknown > 0 {
        labels.push(t!("audit_unknown_requests", count = summary.unknown).to_string());
    }
    labels
}

pub(super) fn source(source: Source) -> String {
    t!(match source {
        Source::Ai => "audit_source_ai",
        Source::External => "audit_source_external",
        Source::Local => "audit_source_local",
        Source::System => "audit_source_system",
    })
    .to_string()
}
pub(super) fn category(category: Category) -> String {
    t!(match category {
        Category::Create => "audit_create",
        Category::Delete => "audit_delete",
        Category::Update => "audit_update",
        Category::Read => "audit_read",
        Category::Control => "audit_control",
        Category::Command => "audit_command",
        Category::Model => "audit_model",
        Category::Unknown => "audit_unknown",
    })
    .to_string()
}
pub(super) fn state(state: &str) -> String {
    t!(match state {
        "approved" => "audit_approved",
        "rejected" => "audit_rejected",
        "expired" => "audit_expired",
        "invalidated" => "audit_invalidated",
        "awaiting_user" | "pending" => "audit_pending",
        "queued" => "audit_queued",
        "running" | "received" | "reported" => "audit_running",
        "succeeded" | "completed" | "recorded" => "audit_completed",
        "failed" => "audit_failed",
        "cached" => "audit_cached",
        "idempotent" => "audit_idempotent",
        "not_executed" => "audit_not_executed",
        "cancelled" | "cancel_requested" => "audit_cancelled",
        "outcome_unknown" | "timed_out" => "audit_uncertain",
        "interrupted" => "audit_interrupted",
        _ => "audit_unknown",
    })
    .to_string()
}
pub(super) fn time(timestamp: i64) -> String {
    chrono::DateTime::from_timestamp_millis(timestamp)
        .map(|d| {
            d.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_default()
}
pub(super) struct RecordText {
    pub heading: String,
    pub technical: Option<String>,
}

pub(super) fn record_text(event: &Event) -> RecordText {
    if event.tool == "model" && event.details["request"].is_object() {
        let request = &event.details["request"];
        let heading = if event.details["purpose"] == "provider_connection_test"
            || request["purpose"] == "provider_test"
        {
            t!("audit_provider_test")
        } else if request["purpose"] == "title" {
            t!("audit_title_generation")
        } else if request["purpose"] == "followup" {
            t!("audit_followup_generation")
        } else {
            t!("audit_chat_request")
        }
        .to_string();
        let technical = [request["provider_name"].as_str(), request["model"].as_str()]
            .into_iter()
            .flatten()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        return RecordText {
            heading,
            technical: (!technical.is_empty()).then_some(technical),
        };
    }
    let name = t!(match event.tool.as_str() {
        "run_command" => "audit_tool_command",
        "read_file" => "audit_tool_read",
        "write_file" => "audit_tool_write",
        "list_directory" => "audit_tool_directory",
        "get_status" => "audit_tool_status",
        "list_connections" => "audit_tool_connections",
        "get_task" => "audit_tool_task",
        "cancel_task" => "audit_tool_cancel",
        "open_session" => "audit_tool_open",
        "close_session" => "audit_tool_close",
        "model" => "audit_tool_model",
        "budget" => "audit_tool_budget",
        _ => "audit_tool_other",
    });
    // Only a recorded purpose can replace the tool label; never infer intent from a shell command.
    let purpose = event
        .context
        .purpose
        .as_deref()
        .and_then(|p| p.lines().next())
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_owned);
    let heading = if event.tool == "run_command" {
        purpose.clone().unwrap_or_else(|| name.to_string())
    } else {
        name.to_string()
    };
    let args = event
        .details
        .get("arguments")
        .or_else(|| event.details["call"].get("arguments"));
    let technical = args
        .and_then(|v| v["command"].as_str().or_else(|| v["path"].as_str()))
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .or(purpose);
    RecordText { heading, technical }
}

pub(super) fn target(event: &Event) -> String {
    event
        .context
        .target_name
        .as_deref()
        .or_else(|| event.details["target"]["name"].as_str())
        .or_else(|| event.details["request"]["conversation_title"].as_str())
        .or(event.context.target_id.as_deref())
        .unwrap_or_default()
        .to_owned()
}

pub(super) fn format_count(count: u64) -> String {
    let digits = count.to_string();
    let mut result = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            result.push(',');
        }
        result.push(digit);
    }
    result
}

pub(super) fn input_fraction(input: u64, output: u64) -> Option<f32> {
    if input == 0 && output == 0 {
        return None;
    }
    // Compute from the reported components, not total (which can have different provider semantics).
    Some((input as f64 / (input as f64 + output as f64)) as f32)
}

pub(super) fn reset_record_filter(filter: &Filter) -> Filter {
    // The conversation owns the lifetime summary; reset only the independent record filters.
    Filter {
        conversation_id: filter.conversation_id.clone(),
        ..Default::default()
    }
}

pub(super) fn inline_inspector(width: gpui::Pixels, rem_size: gpui::Pixels) -> bool {
    width >= px(1000.) * (rem_size / px(14.)).max(1.)
}
pub(super) fn kind(value: &str) -> String {
    t!(match value {
        "request_received" | "external_received" | "tool_requested" => "audit_event_request",
        "request_result" => "audit_event_response",
        "external_rejected" | "authentication_rejected" | "external_transport_failed" =>
            "audit_event_rejection",
        "execution_started" => "audit_event_execution",
        "execution_result" | "tool_completed" => "audit_event_result",
        "approval_requested" => "audit_event_approval_request",
        "approval_approved" | "approval_rejected" | "approval_expired" | "approval_invalidated" =>
            "audit_event_approval_decision",
        "model_started" | "turn_started" => "audit_event_model_start",
        "model_usage" => "audit_event_usage",
        "model_finished" | "turn_finished" => "audit_event_model_finish",
        "budget_requested" | "budget_approved" | "budget_rejected" => "audit_event_budget",
        "tool_cached" => "audit_cached",
        "legacy_summary" => "audit_event_legacy",
        "application_recovered" | "recovery_interrupted" => "audit_event_recovery",
        _ => "audit_event_other",
    })
    .to_string()
}
pub(super) fn code(text: &str) -> String {
    let fence = "`".repeat(
        text.split(|c| c != '`')
            .map(str::len)
            .max()
            .unwrap_or(0)
            .max(2)
            + 1,
    );
    format!("{fence}json\n{text}\n{fence}")
}
#[cfg(test)]
mod usage_tests {
    use super::*;
    use tiny_shell_mcp::audit::Summary;

    #[test]
    fn usage_labels_show_request_count_and_only_nonzero_incomplete_states() {
        assert!(usage_status_labels(&Summary::default()).is_empty());
        let sum = Summary {
            requests: 3,
            confirmed: 1,
            pending: 1,
            unknown: 1,
            ..Default::default()
        };
        assert_eq!(
            usage_status_labels(&sum),
            vec![
                t!("audit_pending_requests", count = 1).to_string(),
                t!("audit_unknown_requests", count = 1).to_string(),
            ]
        );
        let c = crate::ai::Conversation::new(None, &crate::ai::Settings::default());
        assert_eq!(
            usage_label(&c),
            format!(
                "{} · {}",
                t!("audit_total_tokens", count = 0),
                t!("audit_request_count", count = 0)
            )
        );
        assert!(!usage_label(&c).contains(&t!("audit_unknown_requests", count = 0).to_string()));
    }
}

#[cfg(test)]
mod presentation_tests {
    use super::*;
    use serde_json::json;
    use tiny_shell_mcp::audit::Context as AuditContext;

    #[test]
    fn session_control_keeps_actual_action_even_when_it_has_a_user_purpose() {
        let mut context = AuditContext::new(Source::Ai);
        context.purpose = Some("Inspect service status".into());
        let event = Event::new(
            context,
            "execution_result",
            "close_session",
            "succeeded",
            json!({}),
        );
        let text = record_text(&event);
        assert_eq!(text.heading, t!("audit_tool_close").to_string());
        assert_eq!(text.technical.as_deref(), Some("Inspect service status"));
    }

    #[test]
    fn command_title_uses_recorded_purpose_and_keeps_exact_command_separate() {
        let mut context = AuditContext::new(Source::Ai);
        context.purpose = Some("Inspect access logs\nAdditional context".into());
        let event = Event::new(
            context,
            "execution_result",
            "run_command",
            "succeeded",
            json!({"call":{"arguments":{"command":"tail -15 /var/log/nginx/access.log"}}}),
        );
        let text = record_text(&event);
        assert_eq!(text.heading, "Inspect access logs");
        assert_eq!(
            text.technical.as_deref(),
            Some("tail -15 /var/log/nginx/access.log")
        );
        assert!(!text.heading.contains("tail"));
    }

    #[test]
    fn model_purpose_and_historical_model_are_separate() {
        let event = Event::new(
            AuditContext::new(Source::Ai),
            "model_finished",
            "model",
            "completed",
            json!({"request":{"purpose":"followup","provider_name":"Archived provider","model":"historical-model"}}),
        );
        let text = record_text(&event);
        assert_eq!(text.heading, t!("audit_followup_generation").to_string());
        assert_eq!(
            text.technical.as_deref(),
            Some("Archived provider · historical-model")
        );
    }

    #[test]
    fn missing_purpose_does_not_infer_meaning_from_shell_commands() {
        let event = Event::new(
            AuditContext::new(Source::External),
            "execution_result",
            "run_command",
            "succeeded",
            json!({"arguments":{"command":"rm -rf /example"}}),
        );
        assert_eq!(
            record_text(&event).heading,
            t!("audit_tool_command").to_string()
        );
        assert_eq!(
            record_text(&event).technical.as_deref(),
            Some("rm -rf /example")
        );
    }

    #[test]
    fn input_output_ratio_handles_zero_and_large_reported_values() {
        assert_eq!(input_fraction(0, 0), None);
        assert_eq!(input_fraction(40, 40), Some(0.5));
        assert_eq!(input_fraction(u64::MAX, u64::MAX), Some(0.5));
        assert_eq!(input_fraction(1, 0), Some(1.0));
        assert_eq!(format_count(17_701), "17,701");
        assert_eq!(format_count(0), "0");
    }

    #[test]
    fn resetting_detail_filters_preserves_conversation_usage_scope() {
        let filter = Filter {
            conversation_id: Some("synthetic-chat".into()),
            target_id: Some("synthetic-target".into()),
            client_id: Some("synthetic-client".into()),
            source: Some(Source::External),
            category: Some(Category::Command),
            state: Some("failed".into()),
            search: "nginx".into(),
            since_ms: Some(1),
            before_sequence: Some(7),
            ..Default::default()
        };
        let reset = reset_record_filter(&filter);
        assert_eq!(reset.conversation_id, filter.conversation_id);
        assert!(reset.target_id.is_none() && reset.client_id.is_none());
        assert!(reset.source.is_none() && reset.category.is_none() && reset.state.is_none());
        assert!(
            reset.search.is_empty() && reset.since_ms.is_none() && reset.before_sequence.is_none()
        );
    }

    #[test]
    fn inspector_layout_respects_window_width_and_font_scale() {
        assert!(!inline_inspector(px(720.), px(14.)));
        assert!(inline_inspector(px(1040.), px(14.)));
        assert!(!inline_inspector(px(1040.), px(20.)));
        assert!(inline_inspector(px(1440.), px(20.)));
    }
}
