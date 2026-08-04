use gpui::{FontWeight, ParentElement as _, Styled as _, div, px, rems};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, Size,
    button::{Button, ButtonVariants as _},
    h_flex,
    setting::SettingPage,
    v_flex,
};
use rust_i18n::t;

pub(crate) fn page() -> SettingPage {
    use gpui_component::setting::{SettingField, SettingGroup, SettingItem};

    let version = env!("CARGO_PKG_VERSION");
    let installation_label = match crate::app::updater::installation_kind() {
        crate::app::updater::InstallationKind::WindowsInstaller => {
            t!("installation_setup").to_string()
        }
        crate::app::updater::InstallationKind::Portable => t!("installation_portable").to_string(),
        crate::app::updater::InstallationKind::MacApp => t!("installation_app_bundle").to_string(),
        crate::app::updater::InstallationKind::MacInstaller => {
            t!("installation_macos_pkg").to_string()
        }
        crate::app::updater::InstallationKind::LinuxPackage => {
            t!("installation_system_package").to_string()
        }
    };
    let runtime_environment = crate::app::updater::runtime_environment_label();

    SettingPage::new(t!("settings_about").to_string())
        .icon(IconName::Info)
        .group(
            SettingGroup::new().item(SettingItem::render(move |_, _, cx| {
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_4()
                    .py_2()
                    .child(
                        div()
                            .size(px(56.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_md()
                            .bg(cx.theme().primary.opacity(0.12))
                            .text_color(cx.theme().primary)
                            .child(Icon::new(IconName::SquareTerminal).with_size(Size::Large)),
                    )
                    .child(
                        v_flex()
                            .min_w_0()
                            .flex_1()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(rems(1.2))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(t!("app_name").to_string()),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("about_subtitle").to_string()),
                            )
                            .child(
                                div()
                                    .text_size(rems(0.75))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("v{version}")),
                            ),
                    )
                    .child(
                        Button::new("about-github")
                            .secondary()
                            .icon(IconName::Github)
                            .label(t!("about_view_project").to_string())
                            .on_click(|_, _, _| {
                                let _ = crate::app::platform::open_url(
                                    "https://github.com/ynx-official/tiny-shell",
                                );
                            }),
                    )
            })),
        )
        .group(
            SettingGroup::new()
                .title(t!("about_version_info").to_string())
                .item(SettingItem::new(
                    t!("about_app_version").to_string(),
                    SettingField::render(move |_, _, _| {
                        div().text_sm().child(format!("v{version}"))
                    }),
                ))
                .item(SettingItem::new(
                    t!("about_installation_type").to_string(),
                    SettingField::render(move |_, _, _| {
                        div().text_sm().child(installation_label.clone())
                    }),
                ))
                .item(SettingItem::new(
                    t!("about_runtime").to_string(),
                    SettingField::render(move |_, _, _| {
                        div().text_sm().child(runtime_environment.clone())
                    }),
                )),
        )
        .group(
            SettingGroup::new().item(SettingItem::render(move |_, _, cx| {
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("about_feedback_hint")),
                    )
                    .child(
                        Button::new("github-link")
                            .icon(IconName::ExternalLink)
                            .label(t!("about_open_feedback").to_string())
                            .secondary()
                            .on_click(|_, _, _| {
                                let _ = crate::app::platform::open_url(
                                    "https://github.com/ynx-official/tiny-shell/issues",
                                );
                            }),
                    )
            })),
        )
}
