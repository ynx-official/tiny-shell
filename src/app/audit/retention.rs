use super::components::hint;
use super::*;
use gpui::{
    InteractiveElement as _, IntoElement as _, ParentElement as _, StatefulInteractiveElement as _,
    Styled as _, div,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::Input,
    v_flex,
};

impl AuditPanel {
    pub(super) fn settings_view(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let disabled = self.loading || !self.status.ready;
        let body = v_flex()
            .id("audit-settings")
            .flex_1()
            .min_h_0()
            .min_w_0()
            .overflow_y_scroll()
            .p_6()
            .gap_6()
            .child(div().max_w(px(920.)).child(hint(
                t!("audit_privacy_note").to_string(),
                cx.theme().muted_foreground,
            )))
            .child(
                v_flex()
                    .max_w(px(920.))
                    .gap_4()
                    .child(
                        h_flex()
                            .gap_3()
                            .flex_wrap()
                            .justify_between()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(t!("audit_retention_section").to_string()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(
                                        t!(
                                            "audit_storage_size",
                                            mb = self.status.bytes / (1024 * 1024)
                                        )
                                        .to_string(),
                                    ),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_4()
                            .flex_wrap()
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w(px(240.))
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_sm()
                                            .child(t!("audit_retention_days").to_string()),
                                    )
                                    .child(Input::new(&self.days).small().disabled(disabled)),
                            )
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w(px(240.))
                                    .gap_2()
                                    .child(
                                        div().text_sm().child(t!("audit_capacity_mb").to_string()),
                                    )
                                    .child(Input::new(&self.capacity).small().disabled(disabled)),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .max_w(px(920.))
                    .gap_3()
                    .pt_5()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(t!("audit_redaction_section").to_string()),
                    )
                    .child(
                        div()
                            .text_sm()
                            .child(t!("audit_redact_patterns").to_string()),
                    )
                    .child(Input::new(&self.patterns).small().disabled(disabled))
                    .child(hint(
                        t!("audit_redact_note").to_string(),
                        cx.theme().muted_foreground,
                    )),
            )
            .child(
                v_flex()
                    .max_w(px(920.))
                    .gap_3()
                    .pt_5()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(t!("audit_cleanup_section").to_string()),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("audit_cleanup_note").to_string()),
                    )
                    .child(
                        h_flex().child(
                            Button::new("audit-clear")
                                .danger()
                                .outline()
                                .small()
                                .icon(IconName::Delete)
                                .label(t!("audit_clear").to_string())
                                .disabled(disabled)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.confirm_clear(window, cx)
                                })),
                        ),
                    ),
            );
        v_flex()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .child(body)
            .child(
                h_flex()
                    .px_6()
                    .py_3()
                    .justify_end()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        Button::new("audit-save-settings")
                            .primary()
                            .small()
                            .label(t!("audit_save_settings").to_string())
                            .loading(self.loading)
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, _, cx| this.save_settings(cx))),
                    ),
            )
            .into_any_element()
    }
}
