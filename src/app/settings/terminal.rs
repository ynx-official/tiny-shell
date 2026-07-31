use gpui::{Anchor, Entity, IntoElement as _, ParentElement as _, Styled as _, div, px};
use gpui_component::{
    IconName, Sizable as _,
    button::Button,
    h_flex,
    menu::{DropdownMenu as _, PopupMenuItem},
    setting::SettingPage,
    switch::Switch,
};
use rust_i18n::t;

use crate::{TinyShell, session::config::TerminalDisplayStyle};

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
                                    let current = view.read(cx).ui_font_family.to_string();
                                    let names = cx.text_system().all_font_names();
                                    let using_system_maple = crate::app::theme::USING_SYSTEM_MAPLE
                                        .load(std::sync::atomic::Ordering::Relaxed);
                                    if current == ".SystemUIFont"
                                        || current.is_empty()
                                        || !names.contains(&current)
                                    {
                                        t!("system_default").to_string()
                                    } else if !using_system_maple && current == "Maple Mono NF CN" {
                                        format!("Maple Mono NF CN ({})", t!("software_builtin"))
                                    } else {
                                        current
                                    }
                                })
                                .dropdown_menu_with_anchor(Anchor::BottomRight, {
                                    let view = view.clone();
                                    move |mut menu, window, cx| {
                                        let current = view.read(cx).ui_font_family.to_string();
                                        let mut names = cx.text_system().all_font_names();
                                        menu = menu.min_w(200.).max_h(px(320.)).scrollable(true);
                                        menu = menu.item(
                                            PopupMenuItem::new(t!("system_default").to_string())
                                                .checked(
                                                    current == ".SystemUIFont"
                                                        || current.is_empty(),
                                                )
                                                .on_click(window.listener_for(
                                                    &view,
                                                    move |this, _, window, cx| {
                                                        this.change_ui_font_family(
                                                            ".SystemUIFont",
                                                            window,
                                                            cx,
                                                        );
                                                    },
                                                )),
                                        );
                                        let maple_font = "Maple Mono NF CN".to_string();
                                        let using_system_maple =
                                            crate::app::theme::USING_SYSTEM_MAPLE
                                                .load(std::sync::atomic::Ordering::Relaxed);
                                        if !using_system_maple && names.contains(&maple_font) {
                                            names.retain(|n| n != &maple_font);
                                            menu = menu
                                                .item(
                                                    PopupMenuItem::new(format!(
                                                        "{} ({})",
                                                        maple_font,
                                                        t!("software_builtin")
                                                    ))
                                                    .checked(current == maple_font)
                                                    .on_click(window.listener_for(
                                                        &view,
                                                        move |this, _, window, cx| {
                                                            this.change_ui_font_family(
                                                                "Maple Mono NF CN",
                                                                window,
                                                                cx,
                                                            );
                                                        },
                                                    )),
                                                )
                                                .separator();
                                        }
                                        for name in names {
                                            let checked = name == current;
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
                .item(SettingItem::new(
                    t!("terminal_font_family").to_string(),
                    SettingField::render({
                        let view = settings_view.clone();
                        move |_, _window, cx| {
                            Button::new("terminal-font-dropdown")
                                .small()
                                .icon(IconName::ChevronsUpDown)
                                .label({
                                    let current = view.read(cx).terminal_font_family.to_string();
                                    let using_system_maple = crate::app::theme::USING_SYSTEM_MAPLE
                                        .load(std::sync::atomic::Ordering::Relaxed);
                                    if !using_system_maple && current == "Maple Mono NF CN" {
                                        format!("Maple Mono NF CN ({})", t!("software_builtin"))
                                    } else {
                                        current
                                    }
                                })
                                .dropdown_menu_with_anchor(Anchor::BottomRight, {
                                    let view = view.clone();
                                    move |mut menu, window, cx| {
                                        let current =
                                            view.read(cx).terminal_font_family.to_string();
                                        let mut names = cx.text_system().all_font_names();
                                        menu = menu.min_w(200.).max_h(px(320.)).scrollable(true);
                                        let maple_font = "Maple Mono NF CN".to_string();
                                        let using_system_maple =
                                            crate::app::theme::USING_SYSTEM_MAPLE
                                                .load(std::sync::atomic::Ordering::Relaxed);
                                        if !using_system_maple && names.contains(&maple_font) {
                                            names.retain(|n| n != &maple_font);
                                            menu = menu
                                                .item(
                                                    PopupMenuItem::new(format!(
                                                        "{} ({})",
                                                        maple_font,
                                                        t!("software_builtin")
                                                    ))
                                                    .checked(current == maple_font)
                                                    .on_click(window.listener_for(
                                                        &view,
                                                        move |this, _, _window, cx| {
                                                            this.change_terminal_font_family(
                                                                "Maple Mono NF CN",
                                                                cx,
                                                            );
                                                        },
                                                    )),
                                                )
                                                .separator();
                                        }
                                        for name in names {
                                            let checked = name == current;
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
                ))
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
                .item(SettingItem::new(
                    t!("keyword_highlight").to_string(),
                    SettingField::render({
                        let view = settings_view.clone();
                        move |_, window, cx| {
                            Switch::new("keyword-highlight")
                                .small()
                                .checked(view.read(cx).config.keyword_highlight())
                                .on_click(window.listener_for(&view, |this, checked, _, cx| {
                                    this.config.set_keyword_highlight(*checked);
                                    this.mark_config_preferences_dirty();
                                    cx.notify();
                                }))
                                .into_any_element()
                        }
                    }),
                )),
        )
}
