//! Composer summaries keep exact totals and incomplete-state details in the audit tooltip.
use crate::ai::Conversation;
use rust_i18n::t;
use tiny_shell_mcp::audit::Summary;

fn short_number(value: u64) -> String {
    let (unit, scale) = match value {
        0..=999 => return value.to_string(),
        1_000..=999_949 => ("k", 1_000.),
        999_950..=999_949_999 => ("M", 1_000_000.),
        _ => ("B", 1_000_000_000.),
    };
    format!("{:.1}{unit}", value as f64 / scale)
}

fn summary_label(summary: &Summary, legacy: bool) -> String {
    t!(
        "ai_usage_compact",
        tokens = short_number(summary.total),
        requests = short_number(summary.requests as u64),
        partial = if legacy || summary.pending > 0 || summary.unknown > 0 {
            "*"
        } else {
            ""
        }
    )
    .to_string()
}

pub(super) fn label(c: &Conversation) -> String {
    if c.requests.is_empty() && !c.messages.is_empty() {
        return t!("ai_usage_unrecorded").to_string();
    }
    summary_label(
        &c.usage_summary(),
        c.turns
            .first()
            .map_or(!c.messages.is_empty(), |turn| turn.user_message > 0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abbreviated_totals_keep_rounding_boundaries_and_real_zero_distinct() {
        for (value, expected) in [
            (0, "0"),
            (999, "999"),
            (1_000, "1.0k"),
            (75_257, "75.3k"),
            (999_950, "1.0M"),
            (999_950_000, "1.0B"),
        ] {
            assert_eq!(short_number(value), expected);
        }
    }

    #[test]
    fn pending_unknown_and_legacy_usage_remain_marked_in_a_compact_label() {
        let complete = Summary {
            total: 75_257,
            requests: 13,
            confirmed: 13,
            ..Default::default()
        };
        assert!(!summary_label(&complete, false).contains('*'));
        assert!(summary_label(&complete, true).contains('*'));
        for summary in [
            Summary {
                pending: 1,
                ..complete.clone()
            },
            Summary {
                unknown: 1,
                ..complete.clone()
            },
        ] {
            let label = summary_label(&summary, false);
            assert!(label.contains("75.3k") && label.contains("13") && label.contains('*'));
        }
        let mut conversation = Conversation::new(None, &crate::ai::Settings::default());
        conversation
            .messages
            .push(crate::ai::Message::text("assistant", "old reply"));
        assert_eq!(label(&conversation), t!("ai_usage_unrecorded").to_string());
    }
}
