//! Shared flat surfaces and semantic states for every audit tab and inspector.
use gpui::{App, Hsla, IntoElement as _, ParentElement as _, Styled as _, div, px};
use gpui_component::{ActiveTheme as _, Icon, IconName, h_flex, v_flex};
use rust_i18n::t;

pub(super) fn status_color(state: &str, cx: &App) -> Hsla {
    match state {
        "approved" | "succeeded" | "completed" | "recorded" => cx.theme().success,
        "failed" | "rejected" => cx.theme().danger,
        "awaiting_user" | "pending" | "outcome_unknown" | "timed_out" | "interrupted" => {
            cx.theme().warning
        }
        "running" | "received" | "reported" | "queued" => cx.theme().primary,
        _ => cx.theme().muted_foreground,
    }
}

pub(super) fn status_badge(state: &str, cx: &App) -> gpui::AnyElement {
    let color = status_color(state, cx);
    let mut tint = color;
    tint.a *= 0.10;
    let mut border = color;
    border.a *= 0.24;
    div()
        .px_2()
        .py_1()
        .rounded(px(4.))
        .border_1()
        .border_color(border)
        .bg(tint)
        .text_xs()
        .text_color(color)
        .child(super::presentation::state(state))
        .into_any_element()
}

pub(super) fn hint(text: String, color: Hsla) -> gpui::AnyElement {
    h_flex()
        .items_start()
        .gap_2()
        .text_xs()
        .text_color(color)
        .child(Icon::new(IconName::Info).size(px(14.)))
        .child(div().min_w_0().flex_1().child(text))
        .into_any_element()
}

pub(super) fn metric(label: &str, value: String, cx: &App) -> gpui::AnyElement {
    v_flex()
        .min_w(px(88.))
        .gap_1()
        .px_4()
        .border_l_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .text_size(px(24.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(value),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(t!(label).to_string()),
        )
        .into_any_element()
}

pub(super) fn metadata(label: &str, value: String, cx: &App) -> gpui::AnyElement {
    h_flex()
        .items_start()
        .gap_3()
        .text_sm()
        .child(
            div()
                .w(px(72.))
                .flex_shrink_0()
                .text_color(cx.theme().muted_foreground)
                .child(t!(label).to_string()),
        )
        .child(div().flex_1().min_w_0().child(value))
        .into_any_element()
}

pub(super) fn record_icon(tool: &str) -> IconName {
    match tool {
        "model" => IconName::Bot,
        "run_command" | "open_session" | "close_session" => IconName::SquareTerminal,
        "read_file" | "write_file" => IconName::File,
        "list_directory" => IconName::Folder,
        "budget" => IconName::CircleCheck,
        _ => IconName::Network,
    }
}
