//! Shared production selector and isolated native preview, with no authorization side effects.
use gpui::{
    App, FontWeight, IntoElement as _, ParentElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use rust_i18n::t;
use std::rc::Rc;
use tiny_shell_mcp::policy::Mode;

pub(crate) fn selector(
    current: Option<Mode>,
    stacked: bool,
    window: &Window,
    cx: &App,
    on_change: impl Fn(Mode, &mut Window, &mut App) + 'static,
) -> gpui::AnyElement {
    let on_change = Rc::new(on_change);
    let scale = (window.rem_size() / px(14.)).max(1.);
    // Match the requested blue selection while deriving surfaces and text from the theme.
    let accent: gpui::Hsla = gpui::rgb(0x3b82f6).into();
    let mut cards = if stacked {
        v_flex()
    } else {
        h_flex().items_stretch()
    }
    .gap_2()
    .w_full();
    for mode in [Mode::ReadOnly, Mode::Controlled, Mode::Full] {
        let selected = current == Some(mode);
        let on_change = on_change.clone();
        cards = cards.child(
            Button::new(mode_key(mode))
                .ghost()
                .small()
                .flex_1()
                .min_w_0()
                .w_full()
                .h_auto()
                .min_h(px(80.) * scale)
                .p_3()
                .rounded(px(8.))
                .border_1()
                .border_color(if selected { accent } else { cx.theme().border })
                .bg(if selected {
                    accent.opacity(0.06)
                } else {
                    cx.theme().background
                })
                .disabled(current.is_none())
                .child(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .items_start()
                        .gap_2()
                        .child(
                            div()
                                .flex_shrink_0()
                                .size(px(14.) * scale)
                                .mt(px(2.) * scale)
                                .rounded_full()
                                .border_1()
                                .border_color(if selected { accent } else { cx.theme().input })
                                .flex()
                                .items_center()
                                .justify_center()
                                .when(selected, |indicator| {
                                    indicator
                                        .child(div().size(px(6.) * scale).rounded_full().bg(accent))
                                }),
                        )
                        // Reserve the indicator and gap before measuring wrapped text.
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .child(
                                    div()
                                        .w_full()
                                        .text_sm()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(cx.theme().foreground)
                                        .whitespace_normal()
                                        .child(t!(mode_key(mode)).to_string()),
                                )
                                .child(
                                    div()
                                        .w_full()
                                        .text_xs()
                                        .font_weight(FontWeight::NORMAL)
                                        .line_height(gpui::relative(1.35))
                                        .text_color(cx.theme().muted_foreground)
                                        .whitespace_normal()
                                        .child(t!(mode_hint(mode)).to_string()),
                                ),
                        ),
                )
                .on_click(move |_, window, cx| {
                    if current != Some(mode) {
                        on_change(mode, window, cx);
                    }
                }),
        );
    }
    let full = current == Some(Mode::Full);
    let notice_color: gpui::Hsla = if full {
        gpui::rgb(0xf59e0b).into()
    } else {
        accent
    };
    v_flex()
        .w_full()
        .min_w_0()
        .gap_2()
        .child(cards)
        .child(
            h_flex()
                .items_start()
                .gap_2()
                .p_2()
                .rounded(px(6.))
                .border_1()
                .border_color(notice_color.opacity(0.45))
                .bg(notice_color.opacity(0.06))
                .child(
                    Icon::new(if full {
                        IconName::TriangleAlert
                    } else {
                        IconName::Info
                    })
                    .size(px(14.))
                    .text_color(notice_color),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .line_height(gpui::relative(1.4))
                        .text_color(cx.theme().foreground)
                        .whitespace_normal()
                        .child(
                            t!(if full {
                                "mcp_mode_full_notice"
                            } else {
                                "mcp_mode_notice"
                            })
                            .to_string(),
                        ),
                ),
        )
        .into_any_element()
}

pub(crate) fn mode_key(mode: Mode) -> &'static str {
    match mode {
        Mode::ReadOnly => "mcp_read_only",
        Mode::Controlled => "mcp_controlled",
        Mode::Full => "mcp_full",
    }
}

fn mode_hint(mode: Mode) -> &'static str {
    match mode {
        Mode::ReadOnly => "mcp_read_only_hint",
        Mode::Controlled => "mcp_controlled_hint",
        Mode::Full => "mcp_full_hint",
    }
}
