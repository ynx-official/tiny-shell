use gpui::{Anchor, Entity, IntoElement as _, px};
use gpui_component::{
    IconName, Sizable as _,
    button::Button,
    menu::{DropdownMenu as _, PopupMenuItem},
    setting::SettingPage,
};
use rust_i18n::t;

use crate::TinyShell;

pub(crate) fn page(settings_view: &Entity<TinyShell>) -> SettingPage {
    use gpui_component::setting::{SettingField, SettingGroup, SettingItem};

    SettingPage::new(t!("settings_appearance").to_string())
        .icon(IconName::Sun)
        .default_open(true)
        .group(
            SettingGroup::new()
                .title(t!("settings_group_appearance").to_string())
                .item(
                    SettingItem::new(
                        t!("theme_mode").to_string(),
                        SettingField::render({
                            let view = settings_view.clone();
                            move |_, _window, cx| {
                                let (follow_system, is_dark_mode) = {
                                    let state = view.read(cx);
                                    (state.follow_system_theme, state.theme_mode.is_dark())
                                };
                                Button::new("theme-mode-dropdown")
                                    .small()
                                    .icon(if follow_system { IconName::Sun } else if is_dark_mode { IconName::Moon } else { IconName::Sun })
                                    .label(if follow_system { t!("follow_system").to_string() } else if is_dark_mode { t!("use_dark_mode").to_string() } else { t!("use_light_mode").to_string() })
                                    .dropdown_menu_with_anchor(Anchor::BottomRight, {
                                        let view = view.clone();
                                        move |mut menu, window, cx| {
                                            let (follow_system, is_dark_mode) = {
                                                let state = view.read(cx);
                                                (state.follow_system_theme, state.theme_mode.is_dark())
                                            };
                                            menu = menu.min_w(160.)
                                                .item(
                                                    PopupMenuItem::new(t!("follow_system").to_string())
                                                        .checked(follow_system)
                                                        .on_click(window.listener_for(&view, |this, _, window, cx| {
                                                            this.set_follow_system_theme(true, window, cx)
                                                        }))
                                                )
                                                .item(
                                                    PopupMenuItem::new(t!("use_light_mode").to_string())
                                                        .checked(!follow_system && !is_dark_mode)
                                                        .on_click(window.listener_for(&view, |this, _, window, cx| {
                                                            this.switch_theme_mode(crate::app::ThemeMode::Light, window, cx)
                                                        }))
                                                )
                                                .item(
                                                    PopupMenuItem::new(t!("use_dark_mode").to_string())
                                                        .checked(!follow_system && is_dark_mode)
                                                        .on_click(window.listener_for(&view, |this, _, window, cx| {
                                                            this.switch_theme_mode(crate::app::ThemeMode::Dark, window, cx)
                                                        }))
                                                );
                                            menu
                                        }
                                    })
                                    .into_any_element()
                            }
                        })
                    )
                )
                .item(
                    SettingItem::new(
                        t!("light_theme").to_string(),
                        SettingField::render({
                            let view = settings_view.clone();
                            move |_, _window, cx| {
                                let current_theme = view.read(cx).light_theme_name.to_string();
                                Button::new("light-theme-dropdown")
                                    .small()
                                    .icon(IconName::Sun)
                                    .label(current_theme.clone())
                                    .dropdown_menu_with_anchor(Anchor::BottomRight, {
                                        let view = view.clone();
                                        move |mut menu, window, cx| {
                                            let current_theme = view.read(cx).light_theme_name.to_string();
                                            let themes = gpui_component::ThemeRegistry::global(cx).sorted_themes();
                                            let light_themes: Vec<_> = themes.into_iter().filter(|t| !t.mode.is_dark()).map(|t| t.name.clone()).collect();
                                            menu = menu.min_w(160.).max_h(px(320.)).scrollable(true);
                                            for theme_name in light_themes {
                                                let checked = theme_name == current_theme;
                                                menu = menu.item(
                                                    PopupMenuItem::new(theme_name.clone())
                                                        .checked(checked)
                                                        .on_click(window.listener_for(&view, move |this, _, window, cx| {
                                                            this.apply_theme(theme_name.clone(), window, cx)
                                                        }))
                                                );
                                            }
                                            menu
                                        }
                                    })
                                    .into_any_element()
                            }
                        })
                    )
                )
                .item(
                    SettingItem::new(
                        t!("dark_theme").to_string(),
                        SettingField::render({
                            let view = settings_view.clone();
                            move |_, _window, cx| {
                                let current_theme = view.read(cx).dark_theme_name.to_string();
                                Button::new("dark-theme-dropdown")
                                    .small()
                                    .icon(IconName::Moon)
                                    .label(current_theme.clone())
                                    .dropdown_menu_with_anchor(Anchor::BottomRight, {
                                        let view = view.clone();
                                        move |mut menu, window, cx| {
                                            let current_theme = view.read(cx).dark_theme_name.to_string();
                                            let themes = gpui_component::ThemeRegistry::global(cx).sorted_themes();
                                            let dark_themes: Vec<_> = themes.into_iter().filter(|t| t.mode.is_dark()).map(|t| t.name.clone()).collect();
                                            menu = menu.min_w(160.).max_h(px(320.)).scrollable(true);
                                            for theme_name in dark_themes {
                                                let checked = theme_name == current_theme;
                                                menu = menu.item(
                                                    PopupMenuItem::new(theme_name.clone())
                                                        .checked(checked)
                                                        .on_click(window.listener_for(&view, move |this, _, window, cx| {
                                                            this.apply_theme(theme_name.clone(), window, cx)
                                                        }))
                                                );
                                            }
                                            menu
                                        }
                                    })
                                    .into_any_element()
                            }
                        })
                    )
                )
                .item(
                    SettingItem::new(
                        format!("{}{}", t!("title_bar_style"), t!("restart_hint")),
                        SettingField::render({
                            let view = settings_view.clone();
                            move |_, _window, cx| {
                                let current_style = view.read(cx).config.title_bar_style();
                                Button::new("title-bar-style-dropdown")
                                    .small()
                                    .label({
                                        let key = crate::app::settings::title_bar_style_key(
                                            current_style,
                                        );
                                        t!(key).to_string()
                                    })
                                    .dropdown_menu_with_anchor(Anchor::BottomRight, {
                                        let view = view.clone();
                                        move |mut menu, window, cx| {
                                            let current_style = view.read(cx).config.title_bar_style();
                                            menu = menu.min_w(160.)
                                                .item(
                                                    PopupMenuItem::new(t!("title_bar_native").to_string())
                                                        .checked(current_style == crate::session::config::TitleBarStyle::Native)
                                                        .on_click(window.listener_for(&view, |this, _, _, cx| {
                                                            this.config.set_title_bar_style(crate::session::config::TitleBarStyle::Native);
                                                            this.mark_config_preferences_dirty();
                                                            cx.notify();
                                                        }))
                                                )
                                                .item(
                                                    PopupMenuItem::new(t!("title_bar_integrated").to_string())
                                                        .checked(current_style == crate::session::config::TitleBarStyle::Integrated)
                                                        .on_click(window.listener_for(&view, |this, _, _, cx| {
                                                            this.config.set_title_bar_style(crate::session::config::TitleBarStyle::Integrated);
                                                            this.mark_config_preferences_dirty();
                                                            cx.notify();
                                                        }))
                                                );
                                            menu
                                        }
                                    })
                                    .into_any_element()
                            }
                        })
                    )
                ),
        )
}
