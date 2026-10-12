use super::*;
use gpui::{
    InteractiveElement as _, IntoElement as _, ParentElement as _, StatefulInteractiveElement as _,
    Styled as _, div,
};
use gpui_component::{ActiveTheme as _, text::TextView, v_flex};
use serde_json::Value;
use tiny_shell_mcp::audit::Event;

impl AuditPanel {
    pub(super) fn human_details(event: &Event, cx: &mut Context<Self>) -> gpui::AnyElement {
        let details = &event.details;
        let empty = Value::Null;
        let result = details
            .get("result")
            .or_else(|| details["extra"].get("result"))
            .unwrap_or(&empty);
        let arguments = details
            .get("arguments")
            .or_else(|| details["call"].get("arguments"))
            .unwrap_or(&empty);
        let preview = &details["preview"];
        let diff = preview["old_content"]
            .as_str()
            .zip(preview["new_content"].as_str())
            .filter(|(old, new)| old != new)
            .map(|(old, new)| Value::String(unified_diff(old, new)))
            .unwrap_or(Value::Null);
        let sections = [
            ("audit_params", arguments),
            ("audit_stdout", &result["stdout"]),
            ("audit_stderr", &result["stderr"]),
            ("audit_before", &preview["old_content"]),
            ("audit_after", &preview["new_content"]),
            ("audit_diff", &diff),
            (
                "audit_result",
                if result.is_null() {
                    details.get("extra").unwrap_or(details)
                } else {
                    result
                },
            ),
        ];
        let mut body = v_flex().gap_2();
        if let Some(purpose) = &event.context.purpose {
            body = body.child(div().text_sm().child(purpose.clone()));
        }
        for (key, value) in sections {
            if value.is_null() || value.as_str() == Some("") {
                continue;
            }
            let text = value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| serde_json::to_string_pretty(value).unwrap_or_default());
            body = body
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(t!(key).to_string()),
                )
                .child(
                    div()
                        .id(format!("audit-section-{}-{key}", event.id))
                        .max_h(px(240.))
                        .overflow_y_scroll()
                        .child(
                            TextView::markdown(
                                format!("audit-section-text-{}-{key}", event.id),
                                super::view::code(&text),
                            )
                            .selectable(true),
                        ),
                );
        }
        body.child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(t!("audit_record_json").to_string()),
        )
        .into_any_element()
    }
}
fn unified_diff(before: &str, after: &str) -> String {
    let old = before.split_inclusive('\n').collect::<Vec<_>>();
    let new = after.split_inclusive('\n').collect::<Vec<_>>();
    let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let start = prefix.saturating_sub(3);
    let old_end = (old.len() - suffix + 3).min(old.len());
    let new_end = (new.len() - suffix + 3).min(new.len());
    let mut result = format!(
        "--- before\n+++ after\n@@ -{},{} +{},{} @@\n",
        start + 1,
        old_end - start,
        start + 1,
        new_end - start
    );
    let mut line = |marker: char, text: &str| {
        result.push(marker);
        result.push_str(text);
        if !text.ends_with('\n') {
            result.push_str("\n\\ No newline at end of file\n");
        }
    };
    for text in &old[start..prefix] {
        line(' ', text);
    }
    for text in &old[prefix..old.len() - suffix] {
        line('-', text);
    }
    for text in &new[prefix..new.len() - suffix] {
        line('+', text);
    }
    for text in &old[old.len() - suffix..old_end] {
        line(' ', text);
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diff_preserves_newline_changes_and_surrounding_context() {
        let diff = unified_diff("same\nold\nlast\n", "same\nnew\nlast\n");
        assert!(diff.contains(" same\n-old\n+new\n last\n"));
        assert!(unified_diff("value\n", "value").contains("No newline at end of file"));
    }
}
