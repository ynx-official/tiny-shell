use gpui::{
    Anchor, Context, Entity, FontWeight, IntoElement as _, ParentElement as _, PathPromptOptions,
    Styled as _, Window, div, prelude::FluentBuilder as _, rems,
};
use gpui_component::{
    ActiveTheme as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::{DropdownMenu as _, PopupMenuItem},
    setting::SettingPage,
    switch::Switch,
    v_flex,
};
use rust_i18n::t;

use crate::TinyShell;

impl TinyShell {
    fn choose_download_directory(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(t!("select_download_directory").to_string().into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            match prompt.await {
                Ok(Ok(Some(mut paths))) => {
                    if let Some(path) = paths.pop() {
                        this.update(cx, |this, cx| {
                            this.config.set_download_directory(Some(&path));
                            this.mark_config_preferences_dirty();
                            cx.notify();
                        })?;
                    }
                }
                Ok(Err(error)) => {
                    this.update(cx, |this, cx| {
                        this.status = t!(
                            "download_directory_picker_failed",
                            error = error.to_string()
                        )
                        .to_string()
                        .into();
                        cx.notify();
                    })?;
                }
                _ => {}
            }
            Ok::<(), anyhow::Error>(())
        })
        .detach();
    }

    fn clear_download_directory(&mut self, cx: &mut Context<Self>) {
        self.config.set_download_directory(None);
        self.mark_config_preferences_dirty();
        cx.notify();
    }
}

pub(crate) fn page(settings_view: &Entity<TinyShell>) -> SettingPage {
    use gpui_component::setting::{SettingField, SettingGroup, SettingItem};

    SettingPage::new(t!("settings_workspace").to_string())
        .icon(IconName::FolderOpen)
        .group(
            SettingGroup::new()
                .title(t!("settings_group_download").to_string())
                .item(SettingItem::render({
                    let view = settings_view.clone();
                    move |_, window, cx| {
                        let directory = view.read(cx).config.download_directory();
                        v_flex()
                            .w_full()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(t!("download_directory").to_string()),
                            )
                            .child(
                                div()
                                    .text_size(rems(0.78))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("download_directory_desc").to_string()),
                            )
                            .child(
                                h_flex()
                                    .w_full()
                                    .min_w_0()
                                    .gap_2()
                                    .items_center()
                                    .child(
                                        div()
                                            .min_w_0()
                                            .flex_1()
                                            .truncate()
                                            .rounded_md()
                                            .border_1()
                                            .border_color(cx.theme().border)
                                            .px_3()
                                            .py_2()
                                            .text_sm()
                                            .text_color(if directory.is_some() {
                                                cx.theme().foreground
                                            } else {
                                                cx.theme().muted_foreground
                                            })
                                            .child(
                                                directory
                                                    .as_ref()
                                                    .map(|path| path.to_string_lossy().to_string())
                                                    .unwrap_or_else(|| {
                                                        t!("download_directory_not_set").to_string()
                                                    }),
                                            ),
                                    )
                                    .child(
                                        Button::new("choose-download-directory")
                                            .flex_shrink_0()
                                            .small()
                                            .icon(IconName::FolderOpen)
                                            .label(t!("choose_directory").to_string())
                                            .on_click(window.listener_for(
                                                &view,
                                                |this, _, window, cx| {
                                                    this.choose_download_directory(window, cx)
                                                },
                                            )),
                                    )
                                    .when(directory.is_some(), |this| {
                                        this.child(
                                            Button::new("clear-download-directory")
                                                .flex_shrink_0()
                                                .small()
                                                .ghost()
                                                .icon(IconName::Close)
                                                .label(t!("clear").to_string())
                                                .on_click({
                                                    let view = view.clone();
                                                    move |_, _, cx| {
                                                        view.update(cx, |this, cx| {
                                                            this.clear_download_directory(cx)
                                                        });
                                                    }
                                                }),
                                        )
                                    }),
                            )
                    }
                })),
        )
        .group(
            SettingGroup::new()
                .title(t!("settings_group_other").to_string())
                .item(
                    SettingItem::new(
                        t!("lock_layout").to_string(),
                        SettingField::render({
                            let view = settings_view.clone();
                            move |_, window, cx| {
                                Switch::new("lock-layout")
                                    .small()
                                    .checked(view.read(cx).config.lock_layout())
                                    .on_click(window.listener_for(&view, |this, checked, _, cx| {
                                        this.config.set_lock_layout(*checked);
                                        this.mark_config_preferences_dirty();
                                        cx.notify();
                                    }))
                                    .into_any_element()
                            }
                        }),
                    )
                    .description(t!("lock_layout_hint").to_string()),
                )
                .item(SettingItem::new(
                    t!("monitoring_position").to_string(),
                    SettingField::render({
                        let view = settings_view.clone();
                        move |_, _window, cx| {
                            Button::new("monitoring-position-dropdown")
                                .small()
                                .icon(IconName::PanelLeftOpen)
                                .label({
                                    let position =
                                        crate::app::settings::MonitoringPosition::from_config(
                                            view.read(cx).config.monitoring_position(),
                                        );
                                    t!(position.translation_key()).to_string()
                                })
                                .dropdown_menu_with_anchor(Anchor::BottomRight, {
                                    let view = view.clone();
                                    move |mut menu, window, cx| {
                                        let current =
                                            crate::app::settings::MonitoringPosition::from_config(
                                                view.read(cx).config.monitoring_position(),
                                            );
                                        menu = menu.min_w(160.);
                                        for position in crate::app::settings::MONITORING_POSITIONS {
                                            menu = menu.item(
                                                PopupMenuItem::new(
                                                    t!(position.translation_key()).to_string(),
                                                )
                                                .checked(position == current)
                                                .on_click(window.listener_for(
                                                    &view,
                                                    move |this, _, _window, cx| {
                                                        this.config.set_monitoring_position(
                                                            position.config_value(),
                                                        );
                                                        this.mark_config_preferences_dirty();
                                                        cx.notify();
                                                    },
                                                )),
                                            );
                                        }
                                        menu
                                    }
                                })
                                .into_any_element()
                        }
                    }),
                ))
                .item(SettingItem::new(
                    t!("language").to_string(),
                    SettingField::render({
                        let view = settings_view.clone();
                        move |_, _window, cx| {
                            Button::new("language-dropdown")
                                .small()
                                .icon(IconName::Globe)
                                .label({
                                    let language =
                                        crate::app::settings::DisplayLanguage::from_config(
                                            view.read(cx).config.locale(),
                                        );
                                    t!(language.translation_key()).to_string()
                                })
                                .dropdown_menu_with_anchor(Anchor::BottomRight, {
                                    let view = view.clone();
                                    move |mut menu, window, cx| {
                                        let current =
                                            crate::app::settings::DisplayLanguage::from_config(
                                                view.read(cx).config.locale(),
                                            );
                                        menu = menu.min_w(160.);
                                        for language in crate::app::settings::DISPLAY_LANGUAGES {
                                            menu = menu.item(
                                                PopupMenuItem::new(
                                                    t!(language.translation_key()).to_string(),
                                                )
                                                .checked(language == current)
                                                .on_click(window.listener_for(
                                                    &view,
                                                    move |this, _, window, cx| {
                                                        this.set_display_language(
                                                            language.config_value(),
                                                            window,
                                                            cx,
                                                        )
                                                    },
                                                )),
                                            );
                                            if language
                                                == crate::app::settings::DisplayLanguage::System
                                            {
                                                menu = menu.separator();
                                            }
                                        }
                                        menu
                                    }
                                })
                                .into_any_element()
                        }
                    }),
                ))
                .item(
                    SettingItem::new(
                        t!("reset_layout").to_string(),
                        SettingField::render({
                            let view = settings_view.clone();
                            move |_, window, _cx| {
                                Button::new("reset-layout")
                                    .small()
                                    .label(t!("reset").to_string())
                                    .on_click(window.listener_for(&view, |this, _, window, cx| {
                                        this.reset_layout(window, cx);
                                    }))
                                    .into_any_element()
                            }
                        }),
                    )
                    .description(t!("reset_layout_hint").to_string()),
                ),
        )
}
