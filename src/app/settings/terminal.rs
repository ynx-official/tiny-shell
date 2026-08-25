use gpui::{Anchor, Entity, IntoElement as _, ParentElement as _, Styled as _, div, px};
use gpui_component::{
    ActiveTheme as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::{DropdownMenu as _, PopupMenuItem},
    setting::SettingPage,
    switch::Switch,
};
use rust_i18n::t;

use crate::{
    TinyShell,
    app::font_preferences::{self, SYSTEM_MONO_FONT, SYSTEM_UI_FONT},
    session::config::TerminalDisplayStyle,
};

pub(crate) fn page(settings_view: &Entity<TinyShell>) -> SettingPage {
    use gpui_component::setting::{SettingField, SettingGroup, SettingItem};

    SettingPage::new(t!("settings_terminal").to_string())
        .icon(IconName::SquareTerminal)
        .group(
            SettingGroup::new()
                .title(t!("settings_group_font").to_string())
                .item(SettingItem::new(
                    t!("ui_font_size").to_string(),
                    SettingField::render({
                        let view = settings_view.clone();
                        move |_, window, cx| {
                            h_flex()
                                .items_center()
                                .gap_3()
                                .child(
                                    Button::new("ui-font-size-down")
                                        .small()
                                        .label("-")
                                        .on_click(window.listener_for(&view, |this, _, _, cx| {
                                            this.change_ui_font_size(-1.0, cx)
                                        })),
                                )
                                .child(
                                    div()
                                        .min_w(px(64.))
                                        .text_center()
                                        .child(format!("{:.0}px", view.read(cx).ui_font_size)),
                                )
                                .child(Button::new("ui-font-size-up").small().label("+").on_click(
                                    window.listener_for(&view, |this, _, _, cx| {
                                        this.change_ui_font_size(1.0, cx)
                                    }),
                                ))
                                .into_any_element()
                        }
                    }),
                ))
                .item(SettingItem::new(
                    t!("terminal_font_size").to_string(),
                    SettingField::render({
                        let view = settings_view.clone();
                        move |_, window, cx| {
                            h_flex()
                                .items_center()
                                .gap_3()
                                .child(
                                    Button::new("terminal-font-size-down")
                                        .small()
                                        .label("-")
                                        .on_click(window.listener_for(&view, |this, _, _, cx| {
                                            this.change_terminal_font_size(-1.0, cx)
                                        })),
                                )
                                .child(
                                    div().min_w(px(64.)).text_center().child(format!(
                                        "{:.0}px",
                                        view.read(cx).terminal_font_size
                                    )),
                                )
                                .child(
                                    Button::new("terminal-font-size-up")
                                        .small()
                                        .label("+")
                                        .on_click(window.listener_for(&view, |this, _, _, cx| {
                                            this.change_terminal_font_size(1.0, cx)
                                        })),
                                )
                                .into_any_element()
                        }
                    }),
                ))
                .item(SettingItem::new(
                    t!("terminal_display_style").to_string(),
                    SettingField::render({
                        let view = settings_view.clone();
                        move |_, _window, cx| {
                            let current = view.read(cx).terminal_display_style;
                            Button::new("terminal-display-style-dropdown")
                                .small()
                                .icon(IconName::ChevronsUpDown)
                                .label({
                                    let key =
                                        crate::app::settings::terminal_display_style_key(current);
                                    t!(key).to_string()
                                })
                                .dropdown_menu_with_anchor(Anchor::BottomRight, {
                                    let view = view.clone();
                                    move |menu, window, _cx| {
                                        menu.min_w(160.)
                                            .item(
                                                PopupMenuItem::new(
                                                    t!("terminal_display_style_standard")
                                                        .to_string(),
                                                )
                                                .checked(current == TerminalDisplayStyle::Standard)
                                                .on_click(window.listener_for(
                                                    &view,
                                                    |this, _, _, cx| {
                                                        this.change_terminal_display_style(
                                                            TerminalDisplayStyle::Standard,
                                                            cx,
                                                        );
                                                    },
                                                )),
                                            )
                                            .item(
                                                PopupMenuItem::new(
                                                    t!("terminal_display_style_compact")
                                                        .to_string(),
                                                )
                                                .checked(current == TerminalDisplayStyle::Compact)
                                                .on_click(window.listener_for(
                                                    &view,
                                                    |this, _, _, cx| {
                                                        this.change_terminal_display_style(
                                                            TerminalDisplayStyle::Compact,
                                                            cx,
                                                        );
                                                    },
                                                )),
                                            )
                                    }
                                })
                                .into_any_element()
                        }
                    }),
                ))
                .item(SettingItem::new(
                    t!("ui_font_family").to_string(),
                    SettingField::render({
                        let view = settings_view.clone();
                        move |_, _window, cx| {
                            Button::new("ui-font-dropdown")
                                .small()
                                .icon(IconName::ChevronsUpDown)
                                .label({
                                    let this = view.read(cx);
                                    let preference = this.config.ui_font_family();
                                    if preference.is_empty()
                                        || preference.eq_ignore_ascii_case(SYSTEM_UI_FONT)
                                    {
                                        t!("system_default").to_string()
                                    } else if !this.ui_font_preference_available {
                                        t!(
                                            "font_unavailable_using",
                                            font = preference,
                                            fallback = t!("system_default")
                                        )
                                        .to_string()
                                    } else {
                                        this.ui_font_family.to_string()
                                    }
                                })
                                .dropdown_menu_with_anchor(Anchor::BottomRight, {
                                    let view = view.clone();
                                    move |mut menu, window, cx| {
                                        let preference =
                                            view.read(cx).config.ui_font_family().to_string();
                                        let mut names = cx.text_system().all_font_names();
                                        names.retain(|name| {
                                            !name.eq_ignore_ascii_case(SYSTEM_UI_FONT)
                                                && !name.eq_ignore_ascii_case(SYSTEM_MONO_FONT)
                                        });
                                        menu = menu.min_w(200.).max_h(px(320.)).scrollable(true);
                                        menu = menu.item(
                                            PopupMenuItem::new(t!("system_default").to_string())
                                                .checked(
                                                    preference.is_empty()
                                                        || preference
                                                            .eq_ignore_ascii_case(SYSTEM_UI_FONT),
                                                )
                                                .on_click(window.listener_for(
                                                    &view,
                                                    move |this, _, window, cx| {
                                                        this.change_ui_font_family(
                                                            SYSTEM_UI_FONT,
                                                            window,
                                                            cx,
                                                        );
                                                    },
                                                )),
                                        );
                                        for name in names {
                                            let checked = name.eq_ignore_ascii_case(&preference);
                                            menu = menu.item(
                                                PopupMenuItem::new(name.clone())
                                                    .checked(checked)
                                                    .on_click(window.listener_for(
                                                        &view,
                                                        move |this, _, window, cx| {
                                                            this.change_ui_font_family(
                                                                &name, window, cx,
                                                            );
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
                .item(
                    SettingItem::new(
                        t!("terminal_font_family").to_string(),
                        SettingField::render({
                            let view = settings_view.clone();
                            move |_, _window, cx| {
                                Button::new("terminal-font-dropdown")
                                    .small()
                                    .icon(IconName::ChevronsUpDown)
                                    .label({
                                        let recommended = font_preferences::system_mono_family(
                                            &cx.text_system().all_font_names(),
                                        );
                                        let this = view.read(cx);
                                        let preference = this.config.terminal_font_family();
                                        if preference.is_empty()
                                            || preference.eq_ignore_ascii_case(SYSTEM_MONO_FONT)
                                            || (this.terminal_font_preference_available
                                                && this
                                                    .terminal_font_family
                                                    .as_ref()
                                                    .eq_ignore_ascii_case(recommended.as_ref()))
                                        {
                                            t!(
                                                "system_recommended_font",
                                                font = recommended.as_ref()
                                            )
                                            .to_string()
                                        } else if !this.terminal_font_preference_available {
                                            t!(
                                                "font_unavailable_using",
                                                font = preference,
                                                fallback = this.terminal_font_family.as_ref()
                                            )
                                            .to_string()
                                        } else {
                                            this.terminal_font_family.to_string()
                                        }
                                    })
                                    .dropdown_menu_with_anchor(Anchor::BottomRight, {
                                        let view = view.clone();
                                        move |mut menu, window, cx| {
                                            let (preference, preference_available) = {
                                                let this = view.read(cx);
                                                (
                                                    this.config.terminal_font_family().to_string(),
                                                    this.terminal_font_preference_available,
                                                )
                                            };
                                            let mut names = cx.text_system().all_font_names();
                                            let recommended =
                                                font_preferences::system_mono_family(&names)
                                                    .to_string();
                                            names.retain(|name| {
                                                !name.eq_ignore_ascii_case(SYSTEM_UI_FONT)
                                                    && !name.eq_ignore_ascii_case(SYSTEM_MONO_FONT)
                                                    && !name.eq_ignore_ascii_case(&recommended)
                                            });
                                            menu =
                                                menu.min_w(200.).max_h(px(320.)).scrollable(true);
                                            menu = menu.item(
                                                PopupMenuItem::new(
                                                    t!(
                                                        "system_recommended_font",
                                                        font = recommended.clone()
                                                    )
                                                    .to_string(),
                                                )
                                                .checked(
                                                    preference.is_empty()
                                                        || preference
                                                            .eq_ignore_ascii_case(SYSTEM_MONO_FONT)
                                                        || (preference_available
                                                            && preference.eq_ignore_ascii_case(
                                                                &recommended,
                                                            )),
                                                )
                                                .on_click(window.listener_for(
                                                    &view,
                                                    move |this, _, _window, cx| {
                                                        this.change_terminal_font_family(
                                                            SYSTEM_MONO_FONT,
                                                            cx,
                                                        );
                                                    },
                                                )),
                                            );
                                            for name in names {
                                                let checked =
                                                    name.eq_ignore_ascii_case(&preference);
                                                menu = menu.item(
                                                    PopupMenuItem::new(name.clone())
                                                        .checked(checked)
                                                        .on_click(window.listener_for(
                                                            &view,
                                                            move |this, _, _window, cx| {
                                                                this.change_terminal_font_family(
                                                                    &name, cx,
                                                                );
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
                    )
                    .description(t!("terminal_font_nerd_hint").to_string()),
                )
                .item(SettingItem::new(
                    t!("cursor_style").to_string(),
                    SettingField::render({
                        let view = settings_view.clone();
                        move |_, _window, cx| {
                            let current = view.read(cx).cursor_style;
                            Button::new("cursor-style-dropdown")
                                .small()
                                .icon(IconName::ChevronsUpDown)
                                .label({
                                    let key = crate::app::settings::cursor_style_key(current);
                                    t!(key).to_string()
                                })
                                .dropdown_menu_with_anchor(Anchor::BottomRight, {
                                    let view = view.clone();
                                    move |mut menu, window, cx| {
                                        use crate::session::config::CursorStyle;
                                        let current = view.read(cx).cursor_style;
                                        menu = menu.min_w(160.).max_h(px(320.)).scrollable(true);
                                        for style in [
                                            CursorStyle::Default,
                                            CursorStyle::Blink,
                                            CursorStyle::Beam,
                                            CursorStyle::BeamBlink,
                                        ] {
                                            let checked = style == current;
                                            let key = crate::app::settings::cursor_style_key(style);
                                            let label = t!(key).to_string();
                                            menu = menu.item(
                                                PopupMenuItem::new(label)
                                                    .checked(checked)
                                                    .on_click(window.listener_for(
                                                        &view,
                                                        move |this, _, _window, cx| {
                                                            this.change_cursor_style(style, cx);
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
                )),
        )
        .group(
            SettingGroup::new()
                .title(t!("settings_group_terminal_behavior").to_string())
                .item(
                    SettingItem::new(
                        t!("keyword_highlight").to_string(),
                        SettingField::render({
                            let view = settings_view.clone();
                            move |_, window, cx| {
                                let enabled_count = view
                                    .read(cx)
                                    .config
                                    .highlight_rules()
                                    .iter()
                                    .filter(|rule| rule.enabled)
                                    .count();
                                h_flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(
                                                t!(
                                                    "highlight_rules_enabled_count",
                                                    count = enabled_count
                                                )
                                                .to_string(),
                                            ),
                                    )
                                    .child(
                                        Button::new("manage-highlight-rules")
                                            .small()
                                            .secondary()
                                            .label(t!("highlight_rules_manage").to_string())
                                            .on_click(window.listener_for(
                                                &view,
                                                |this, _, window, cx| {
                                                    this.open_highlight_rules_dialog(window, cx);
                                                },
                                            )),
                                    )
                                    .child(
                                        Switch::new("keyword-highlight")
                                            .small()
                                            .checked(view.read(cx).config.keyword_highlight())
                                            .on_click(window.listener_for(
                                                &view,
                                                |this, checked, _, cx| {
                                                    this.config.set_keyword_highlight(*checked);
                                                    this.mark_config_preferences_dirty();
                                                    cx.notify();
                                                },
                                            )),
                                    )
                                    .into_any_element()
                            }
                        }),
                    )
                    .description(t!("keyword_highlight_hint").to_string()),
                ),
        )
}
