//! Compact conversation selector; mode semantics stay shared with MCP execution.
use gpui::{
    Anchor, App, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _, Styled as _,
    Window, div, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Side, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::{DropdownMenu as _, PopupMenuItem},
    v_flex,
};
use rust_i18n::t;
use std::rc::Rc;
use tiny_shell_mcp::policy::Mode;

fn summary_key(mode: Mode) -> &'static str {
    match mode {
        Mode::ReadOnly => "ai_mode_read_only_summary",
        Mode::Controlled => "ai_mode_controlled_summary",
        Mode::Full => "ai_mode_full_summary",
    }
}

fn presentation(
    current: Option<Mode>,
    global: Option<Mode>,
    unavailable: Option<&str>,
) -> (String, String) {
    let source = if current.is_none() {
        t!(
            "ai_mode_global_hint",
            mode = global
                .map(|mode| t!(title_key(Some(mode))).to_string())
                .unwrap_or_else(|| t!("ai_ssh_unavailable").to_string())
        )
        .to_string()
    } else {
        t!("ai_mode_override_hint").to_string()
    };
    if let Some(reason) = unavailable {
        let compact_key = match reason {
            "ai_ssh_disabled" => "ai_access_disabled_compact",
            "ai_no_ssh_target" => "ai_access_text_compact",
            "ai_ssh_target_changed" => "ai_access_target_compact",
            "ai_model_no_tools" => "ai_access_model_compact",
            _ => "ai_access_unavailable_compact",
        };
        return (
            t!(compact_key).to_string(),
            format!("{}\n{source}", t!(reason)),
        );
    }
    let mode = current.or(global);
    let mode_label = mode.map(|mode| t!(title_key(Some(mode))).to_string());
    let label = if current.is_none() {
        mode_label
            .map(|mode| t!("ai_mode_follow_compact", mode = mode).to_string())
            .unwrap_or_else(|| t!("ai_mode_follow_global").to_string())
    } else {
        t!(title_key(current)).to_string()
    };
    let hint = mode
        .map(|mode| format!("{}\n{source}", t!(summary_key(mode))))
        .unwrap_or(source);
    (label, hint)
}

fn title_key(mode: Option<Mode>) -> &'static str {
    match mode {
        None => "ai_mode_follow_global",
        Some(Mode::ReadOnly) => "mcp_read_only",
        Some(Mode::Controlled) => "mcp_controlled",
        Some(Mode::Full) => "mcp_full",
    }
}

fn icon(mode: Option<Mode>) -> IconName {
    match mode {
        None => IconName::Settings2,
        Some(Mode::ReadOnly) => IconName::Eye,
        Some(Mode::Controlled) => IconName::CircleCheck,
        Some(Mode::Full) => IconName::SquareTerminal,
    }
}

pub(super) fn selector(
    current: Option<Mode>,
    global: Option<Mode>,
    unavailable: Option<&str>,
    active: bool,
    cx: &App,
    on_change: impl Fn(Option<Mode>, &mut Window, &mut App) + 'static,
) -> gpui::AnyElement {
    let on_change = Rc::new(on_change);
    let (label, tooltip) = presentation(current, global, unavailable);
    let global_label = global
        .map(|mode| t!(title_key(Some(mode))).to_string())
        .unwrap_or_else(|| t!("ai_ssh_unavailable").to_string());
    Button::new("ai-permissions")
        .debug_selector(|| "ai-mode-selector".to_owned())
        .ghost()
        .xsmall()
        .flex_shrink_0()
        .max_w(px(164.))
        .min_w_0()
        .tooltip(tooltip)
        .disabled(active)
        .child(
            h_flex()
                .min_w_0()
                .gap_1()
                .child(
                    Icon::new(if unavailable.is_some() {
                        IconName::EyeOff
                    } else {
                        icon(current)
                    })
                    .size(px(12.)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(label),
                )
                .child(Icon::new(IconName::ChevronDown).size(px(12.))),
        )
        .text_color(if current.is_some() && unavailable.is_none() {
            cx.theme().primary
        } else {
            cx.theme().foreground
        })
        .dropdown_menu_with_anchor(Anchor::BottomRight, move |mut menu, _, _| {
            menu = menu.min_w(px(264.)).max_w(px(288.)).check_side(Side::Right);
            for mode in [
                None,
                Some(Mode::ReadOnly),
                Some(Mode::Controlled),
                Some(Mode::Full),
            ] {
                let on_change = on_change.clone();
                let hint = match mode {
                    None => t!("ai_mode_global_hint", mode = global_label.clone()).to_string(),
                    Some(Mode::ReadOnly) => t!("mcp_read_only_hint").to_string(),
                    Some(Mode::Controlled) => t!("mcp_controlled_hint").to_string(),
                    Some(Mode::Full) => t!("mcp_full_hint").to_string(),
                };
                menu = menu.item(
                    PopupMenuItem::element(move |_, cx| {
                        v_flex()
                            .w(px(216.))
                            .min_w_0()
                            .gap_1()
                            .py_1()
                            .debug_selector(move || format!("ai-mode-option-{}", title_key(mode)))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(t!(title_key(mode)).to_string()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::NORMAL)
                                    .text_color(cx.theme().muted_foreground)
                                    .whitespace_normal()
                                    .line_height(gpui::relative(1.35))
                                    .child(hint.clone()),
                            )
                    })
                    .icon(icon(mode))
                    .checked(current == mode)
                    .on_click(move |_, window, cx| {
                        if current != mode {
                            on_change(mode, window, cx);
                        }
                    }),
                );
                if mode.is_none() {
                    menu = menu.separator();
                }
            }
            menu
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext as _, Context, Render, TestAppContext, VisualTestContext};
    use gpui_component::Root;

    struct Host {
        mode: Option<Mode>,
        active: bool,
    }
    impl Render for Host {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let host = cx.entity();
            v_flex().size_full().justify_end().p_3().child(selector(
                self.mode,
                Some(Mode::Full),
                None,
                self.active,
                cx,
                move |mode, _, cx| {
                    host.update(cx, |this, cx| {
                        this.mode = mode;
                        cx.notify();
                    })
                },
            ))
        }
    }

    #[test]
    fn compact_selector_shows_effective_mode_and_unavailable_reason() {
        let (label, hint) = presentation(None, Some(Mode::Full), None);
        assert_eq!(
            label,
            t!("ai_mode_follow_compact", mode = t!("mcp_full")).to_string()
        );
        assert!(hint.contains(t!("ai_mode_full_summary").as_ref()));
        let (label, hint) = presentation(Some(Mode::Controlled), Some(Mode::ReadOnly), None);
        assert_eq!(label, t!("mcp_controlled").to_string());
        assert!(hint.contains(t!("ai_mode_override_hint").as_ref()));
        for (reason, expected) in [
            ("ai_ssh_disabled", "ai_access_disabled_compact"),
            ("ai_no_ssh_target", "ai_access_text_compact"),
            ("ai_ssh_target_changed", "ai_access_target_compact"),
            ("ai_model_no_tools", "ai_access_model_compact"),
            ("ai_ssh_unavailable", "ai_access_unavailable_compact"),
        ] {
            let (label, hint) = presentation(Some(Mode::Full), Some(Mode::Full), Some(reason));
            assert_eq!(label, t!(expected).to_string());
            assert!(hint.contains(t!(reason).as_ref()));
        }
    }

    #[gpui::test]
    fn menu_selects_an_override_and_can_restore_inheritance(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let handle = cx.open_window(gpui::size(px(360.), px(600.)), |window, cx| {
            let view = cx.new(|_| Host {
                mode: None,
                active: false,
            });
            Root::new(view, window, cx)
        });
        let root = handle.root(cx).unwrap();
        let host = root.read_with(cx, |root, _| {
            root.view().clone().downcast::<Host>().unwrap()
        });
        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        visual.draw(
            gpui::point(px(0.), px(0.)),
            gpui::size(px(360.), px(600.)),
            |_, _| root.into_any_element(),
        );
        for (key, expected) in [
            ("ai-mode-option-mcp_read_only", Some(Mode::ReadOnly)),
            ("ai-mode-option-ai_mode_follow_global", None),
        ] {
            let trigger = visual.debug_bounds("ai-mode-selector").unwrap();
            visual.simulate_click(trigger.center(), gpui::Modifiers::none());
            visual.run_until_parked();
            let bounds = visual.debug_bounds(key).unwrap();
            assert!(bounds.origin.x >= px(0.) && bounds.right() <= px(360.));
            visual.simulate_click(bounds.center(), gpui::Modifiers::none());
            visual.run_until_parked();
            assert_eq!(host.read_with(&visual, |host, _| host.mode), expected);
        }
        host.update_in(&mut visual, |host, _, cx| {
            host.active = true;
            cx.notify();
        });
        visual.run_until_parked();
        let trigger = visual.debug_bounds("ai-mode-selector").unwrap();
        visual.simulate_click(trigger.center(), gpui::Modifiers::none());
        visual.run_until_parked();
        assert!(
            visual
                .debug_bounds("ai-mode-option-mcp_read_only")
                .is_none()
        );
    }
}
