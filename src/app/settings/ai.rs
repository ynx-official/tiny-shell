//! AI settings navigation; individual forms own their draft state.
use super::page::{SettingGroup, SettingItem, SettingPage};
use crate::{
    TinyShell,
    app::ai::{error_text, service},
};
use gpui::{Entity, IntoElement as _, ParentElement as _, Styled as _, Window, div};
use gpui_component::ActiveTheme as _;
use rust_i18n::t;
use std::rc::Rc;
mod execution;
mod feedback;
mod followups;
mod notifications;
mod providers;
use providers::ProviderForm;

type PermissionsAction = Rc<dyn Fn(&mut Window, &mut gpui::App)>;

pub(crate) fn page(owner: &Entity<TinyShell>) -> SettingPage {
    let owner = owner.clone();
    let audit_owner = owner.clone();
    SettingPage::new(t!("settings_ai").to_string())
        .icon(crate::app::ai::icon())
        .default_open(true)
        .group(
            SettingGroup::new()
                .title(t!("ai_settings_provider").to_string())
                .item(SettingItem::render(move |_, window, cx| {
                    window
                        .use_keyed_state("ai-provider-form", cx, |window, cx| {
                            let weak = owner.downgrade();
                            ProviderForm::new(
                                Rc::new(move |window, cx| {
                                    if let Some(owner) = weak.upgrade() {
                                        owner.update(cx, |o, cx| {
                                            o.mcp_requested_task = None;
                                            o.show_ai_settings(5, window, cx);
                                        });
                                    }
                                }),
                                window,
                                cx,
                            )
                        })
                        .into_any_element()
                })),
        )
        .group(
            SettingGroup::new()
                .title(t!("ai_settings_budget").to_string())
                .item(SettingItem::render(|_, window, cx| {
                    window
                        .use_keyed_state("ai-budget-settings", cx, execution::BudgetForm::new)
                        .into_any_element()
                })),
        )
        .group(
            SettingGroup::new()
                .title(t!("ai_settings_notifications").to_string())
                .item(SettingItem::render(|_, window, cx| {
                    window
                        .use_keyed_state(
                            "ai-notification-settings",
                            cx,
                            notifications::NotificationForm::new,
                        )
                        .into_any_element()
                })),
        )
        .group(
            SettingGroup::new()
                .title(t!("audit_title").to_string())
                .on_open(move |_, cx| {
                    crate::app::audit::open(
                        tiny_shell_mcp::audit::Filter::default(),
                        audit_owner.clone(),
                        cx,
                    );
                }),
        )
        .group(
            SettingGroup::new()
                .title(t!("ai_settings_followups").to_string())
                .item(SettingItem::render(|_, window, cx| {
                    window
                        .use_keyed_state("ai-followup-settings", cx, followups::FollowupForm::new)
                        .into_any_element()
                })),
        )
}
fn note(text: String, cx: &gpui::App) -> gpui::Div {
    div()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .whitespace_normal()
        .child(text)
}
fn field(label: &str, input: gpui::AnyElement, cx: &gpui::App) -> gpui::Div {
    div()
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_2()
        .child(note(t!(label).to_string(), cx))
        .child(input)
}
