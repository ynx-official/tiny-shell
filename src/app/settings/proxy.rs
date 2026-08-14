use gpui::{Entity, IntoElement, ParentElement as _, Styled as _, prelude::FluentBuilder as _};
use gpui_component::{
    Disableable as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    setting::{SettingField, SettingGroup, SettingItem, SettingPage},
    switch::Switch,
    v_flex,
};
use rust_i18n::t;

use crate::{TinyShell, app::settings::form::ProxySettingsInputs};

use super::{
    actions::{ProxyFormValues, ProxyKind},
    controls::labeled_input,
};

pub(crate) fn page(view: &Entity<TinyShell>, inputs: ProxySettingsInputs) -> SettingPage {
    SettingPage::new(t!("settings_proxy").to_string())
        .icon(IconName::Network)
        .group(
            SettingGroup::new()
                .title(t!("settings_proxy").to_string())
                .item(SettingItem::new(
                    t!("enable_proxy").to_string(),
                    SettingField::render({
                        let view = view.clone();
                        move |_, window, cx| {
                            Switch::new("use-proxy")
                                .small()
                                .checked(view.read(cx).config.use_proxy())
                                .on_click(window.listener_for(&view, |this, checked, _, cx| {
                                    this.set_proxy_enabled(*checked, cx);
                                }))
                                .into_any_element()
                        }
                    }),
                ))
                .item(
                    SettingItem::new(
                        t!("read_env_proxy").to_string(),
                        SettingField::render({
                            let view = view.clone();
                            move |_, window, cx| {
                                Switch::new("read-env-proxy")
                                    .small()
                                    .checked(view.read(cx).config.read_env_proxy())
                                    .on_click(window.listener_for(&view, |this, checked, _, cx| {
                                        this.set_environment_proxy_enabled(*checked, cx);
                                    }))
                                    .into_any_element()
                            }
                        }),
                    )
                    .description(t!("read_env_proxy_desc").to_string()),
                )
                .item(SettingItem::render({
                    let view = view.clone();
                    move |_, window, cx| {
                        let kind = ProxyKind::from_config(&view.read(cx).global_proxy_type);
                        let can_save = ProxyFormValues::capture(kind, &inputs, cx).is_some();

                        v_flex()
                            .w_full()
                            .gap_3()
                            .child(
                                gpui::div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .child(t!("global_proxy_settings").to_string()),
                            )
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        Button::new("global-proxy-type-socks5")
                                            .small()
                                            .label("SOCKS5")
                                            .when(kind == ProxyKind::Socks5, |button| {
                                                button.primary()
                                            })
                                            .on_click(window.listener_for(
                                                &view,
                                                |this, _, _, cx| {
                                                    this.set_proxy_kind(ProxyKind::Socks5, cx)
                                                },
                                            )),
                                    )
                                    .child(
                                        Button::new("global-proxy-type-http")
                                            .small()
                                            .label("HTTP")
                                            .when(kind == ProxyKind::Http, |button| {
                                                button.primary()
                                            })
                                            .on_click(window.listener_for(
                                                &view,
                                                |this, _, _, cx| {
                                                    this.set_proxy_kind(ProxyKind::Http, cx)
                                                },
                                            )),
                                    ),
                            )
                            .child(labeled_input(
                                t!("global_proxy_host").to_string(),
                                inputs.host.clone(),
                            ))
                            .child(labeled_input(
                                t!("global_proxy_port").to_string(),
                                inputs.port.clone(),
                            ))
                            .child(labeled_input(
                                t!("global_proxy_user").to_string(),
                                inputs.user.clone(),
                            ))
                            .child(labeled_input(
                                t!("global_proxy_password").to_string(),
                                inputs.password.clone(),
                            ))
                            .child(
                                Button::new("save-global-proxy")
                                    .small()
                                    .primary()
                                    .disabled(!can_save)
                                    .label(t!("save_proxy").to_string())
                                    .on_click({
                                        let view = view.clone();
                                        let inputs = inputs.clone();
                                        move |_, window, cx| {
                                            view.update(cx, |this, cx| {
                                                this.save_proxy_settings(&inputs, window, cx);
                                            });
                                        }
                                    }),
                            )
                    }
                })),
        )
}
