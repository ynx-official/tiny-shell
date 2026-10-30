use gpui::{
    Anchor, App, Entity, InteractiveElement as _, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
    tooltip::Tooltip,
    v_flex,
};
use rust_i18n::t;

use super::page::{SettingField, SettingGroup, SettingItem};
use crate::{
    TinyShell,
    app::{
        confirmation_dialog::ConfirmationDialog,
        updater::{
            route_ui,
            routes::{RouteMode, RoutePreferences},
        },
    },
};

// Only acknowledged writes appear as saved entries, including in another settings window.
fn saved_preferences(view: &Entity<TinyShell>, cx: &App) -> RoutePreferences {
    let owner = view.read(cx);
    owner
        .update_runtime
        .pending_route_save
        .as_ref()
        .map(|pending| &pending.previous)
        .unwrap_or_else(|| owner.config.update_routes())
        .clone()
}

pub(super) fn group(view: &Entity<TinyShell>, custom_input: Entity<InputState>) -> SettingGroup {
    SettingGroup::new()
        .title(t!("update_routes").to_string())
        .item(
            SettingItem::new(
                t!("update_route_mode").to_string(),
                SettingField::render({
                    let view = view.clone();
                    move |_, _, cx| {
                        let prefs = saved_preferences(&view, cx);
                        let label = if prefs.mode == RouteMode::Custom {
                            prefs.selected_custom_prefix.clone().unwrap_or_default()
                        } else {
                            t!(prefs.mode.label_key()).to_string()
                        };
                        Button::new("update-route-mode")
                            .small()
                            .max_w(px(240.))
                            .disabled(route_ui::routes_locked(&view.read(cx).update_runtime))
                            .tooltip(label.clone())
                            .child(div().max_w(px(200.)).min_w_0().truncate().child(label))
                            .dropdown_menu_with_anchor(Anchor::BottomRight, {
                                let view = view.clone();
                                move |mut menu, window, cx| {
                                    let current = saved_preferences(&view, cx);
                                    for mode in RouteMode::BUILTIN {
                                        menu = menu.item(
                                            PopupMenuItem::new(t!(mode.label_key()).to_string())
                                                .checked(current.mode == mode)
                                                .on_click(window.listener_for(
                                                    &view,
                                                    move |this, _, window, cx| {
                                                        this.set_update_route_mode(mode, window, cx)
                                                    },
                                                )),
                                        );
                                    }
                                    for prefix in current.custom_prefixes {
                                        let selected = current.mode == RouteMode::Custom
                                            && current.selected_custom_prefix.as_deref()
                                                == Some(prefix.as_str());
                                        let label = prefix.clone();
                                        menu = menu.item(
                                            PopupMenuItem::element(move |_, _| {
                                                div().w(px(280.)).truncate().child(label.clone())
                                            })
                                            .checked(selected)
                                            .on_click(window.listener_for(
                                                &view,
                                                move |this, _, window, cx| {
                                                    this.select_custom_update_route(
                                                        &prefix, window, cx,
                                                    )
                                                },
                                            )),
                                        );
                                    }
                                    menu
                                }
                            })
                    }
                }),
            )
            .description(t!("update_route_mode_desc").to_string()),
        )
        .item(SettingItem::render({
            let view = view.clone();
            move |_, _, cx| {
                let prefs = saved_preferences(&view, cx);
                let runtime = &view.read(cx).update_runtime;
                let locked = route_ui::routes_locked(runtime);
                let saving = runtime.pending_route_save.is_some();
                let error = runtime.route_form_error.clone();
                let empty = custom_input.read(cx).value().trim().is_empty();
                v_flex()
                    .w_full()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .child(t!("update_route_custom_list").to_string()),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("update_route_custom_list_desc").to_string()),
                    )
                    .when(prefs.custom_prefixes.is_empty(), |this| {
                        this.child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(t!("update_route_custom_empty").to_string()),
                        )
                    })
                    .children(
                        prefs
                            .custom_prefixes
                            .iter()
                            .enumerate()
                            .map(|(index, prefix)| {
                                let selected = prefs.mode == RouteMode::Custom
                                    && prefs.selected_custom_prefix.as_deref()
                                        == Some(prefix.as_str());
                                custom_route_row(&view, index, prefix, selected, locked, cx)
                            }),
                    )
                    .child(Input::new(&custom_input).small().disabled(locked).w_full())
                    .child(
                        h_flex()
                            .w_full()
                            .gap_2()
                            .flex_wrap()
                            .child(
                                Button::new("add-update-route")
                                    .small()
                                    .primary()
                                    .disabled(locked || empty)
                                    .label(t!("update_route_add").to_string())
                                    .on_click({
                                        let view = view.clone();
                                        let input = custom_input.clone();
                                        move |_, window, cx| {
                                            view.update(cx, |this, cx| {
                                                this.add_custom_update_route(
                                                    input.clone(),
                                                    window,
                                                    cx,
                                                )
                                            });
                                        }
                                    }),
                            )
                            .when(saving, |this| {
                                this.child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(t!("update_route_saving").to_string()),
                                )
                            }),
                    )
                    .when_some(error, |this, error| {
                        this.child(div().text_sm().text_color(cx.theme().danger).child(error))
                    })
            }
        }))
        .item(SettingItem::render({
            let view = view.clone();
            move |_, window, cx| {
                let runtime = &view.read(cx).update_runtime;
                let locked = route_ui::routes_locked(runtime);
                let time = runtime
                    .route_checked_at
                    .and_then(|timestamp| chrono::DateTime::from_timestamp(timestamp, 0))
                    .map(|time| {
                        time.with_timezone(&chrono::Local)
                            .format("%m-%d %H:%M:%S")
                            .to_string()
                    });
                let report = runtime.route_report.clone();
                v_flex()
                    .w_full()
                    .gap_2()
                    .child(
                        h_flex()
                            .w_full()
                            .justify_between()
                            .items_center()
                            .gap_2()
                            .child(
                                div().text_sm().child(
                                    time.map(|time| {
                                        t!("update_route_last_probe", time = time).to_string()
                                    })
                                    .unwrap_or_else(|| t!("update_route_not_tested").to_string()),
                                ),
                            )
                            .child(
                                Button::new("probe-update-routes")
                                    .small()
                                    .disabled(locked)
                                    .label(
                                        t!(if runtime.route_probing {
                                            "update_route_probing"
                                        } else {
                                            "update_route_retest"
                                        })
                                        .to_string(),
                                    )
                                    .on_click(window.listener_for(&view, |this, _, _, cx| {
                                        this.start_update_route_probe(false, None, cx)
                                    })),
                            ),
                    )
                    .when_some(report, |this, report| {
                        this.children(report.entries.iter().map(|entry| {
                            let recommended = report.routes.first() == Some(&entry.route);
                            h_flex()
                                .w_full()
                                .gap_2()
                                .items_center()
                                .text_sm()
                                .child(
                                    div()
                                        .min_w_0()
                                        .flex_1()
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .child(entry.route.label()),
                                )
                                .child(
                                    div().flex_shrink_0().child(
                                        entry
                                            .elapsed_ms
                                            .map(|ms| format!("{ms} ms"))
                                            .unwrap_or_else(|| "—".into()),
                                    ),
                                )
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .text_color(if recommended {
                                            cx.theme().primary
                                        } else {
                                            cx.theme().muted_foreground
                                        })
                                        .child(
                                            t!(if recommended {
                                                "update_route_recommended"
                                            } else {
                                                entry.state.label_key()
                                            })
                                            .to_string(),
                                        ),
                                )
                        }))
                    })
                    .child(route_ui::route_summary(runtime, cx))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("update_route_privacy").to_string()),
                    )
            }
        }))
}

fn custom_route_row(
    view: &Entity<TinyShell>,
    index: usize,
    prefix: &str,
    selected: bool,
    locked: bool,
    cx: &App,
) -> gpui::Div {
    h_flex()
        .w_full()
        .gap_2()
        .py_2()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .id(format!("custom-update-route-{index}"))
                .flex_1()
                .min_w_0()
                .truncate()
                .text_sm()
                .child(prefix.to_owned())
                .tooltip({
                    let prefix = prefix.to_owned();
                    move |window, cx| {
                        let prefix = prefix.clone();
                        Tooltip::element(move |_, _| {
                            div().w(px(360.)).whitespace_normal().child(prefix.clone())
                        })
                        .build(window, cx)
                    }
                }),
        )
        .child(
            Button::new(format!("use-update-route-{index}"))
                .small()
                .disabled(locked || selected)
                .label(
                    t!(if selected {
                        "update_route_selected"
                    } else {
                        "update_route_use"
                    })
                    .to_string(),
                )
                .on_click({
                    let view = view.clone();
                    let prefix = prefix.to_owned();
                    move |_, window, cx| {
                        view.update(cx, |this, cx| {
                            this.select_custom_update_route(&prefix, window, cx)
                        });
                    }
                }),
        )
        .child(
            Button::new(format!("remove-update-route-{index}"))
                .small()
                .ghost()
                .disabled(locked)
                .label(t!("update_route_remove").to_string())
                .on_click({
                    let view = view.clone();
                    let prefix = prefix.to_owned();
                    move |_, window, cx| {
                        let prefs = saved_preferences(&view, cx);
                        let selected = prefs.mode == RouteMode::Custom
                            && prefs.selected_custom_prefix.as_deref() == Some(prefix.as_str());
                        let view = view.clone();
                        let prefix = prefix.clone();
                        ConfirmationDialog::new(
                            t!("update_route_remove_title").to_string(),
                            t!(
                                if selected {
                                    "update_route_remove_selected_desc"
                                } else {
                                    "update_route_remove_desc"
                                },
                                route = prefix.clone()
                            )
                            .to_string(),
                        )
                        .danger(true)
                        .confirm_label(t!("update_route_remove").to_string())
                        .on_ok(move |_, window, cx| {
                            view.update(cx, |this, cx| {
                                this.remove_custom_update_route(&prefix, window, cx)
                            })
                        })
                        .open(window, cx);
                    }
                }),
        )
}
