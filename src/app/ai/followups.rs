//! Latest-turn followups are ordinary chat inputs, never a direct tool action.
use gpui::{
    App, Context, InteractiveElement as _, IntoElement, ParentElement as _, Render, Styled as _,
    Window, div, px,
};
use gpui_component::{
    ActiveTheme as _, Colorize as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use rust_i18n::t;
use std::rc::Rc;

const INITIAL_VISIBLE: usize = 2;

#[derive(Clone, Copy)]
pub(super) enum Action {
    Add,
    Send,
}
type Select = Rc<dyn Fn(usize, Action, &mut Window, &mut App)>;

pub(super) struct FollowupView {
    items: Vec<String>,
    expanded: bool,
    show_more: bool,
    select: Select,
}

impl FollowupView {
    pub(super) fn new(
        items: Vec<String>,
        select: impl Fn(usize, Action, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            items,
            expanded: true,
            show_more: false,
            select: Rc::new(select),
        }
    }

    #[inline(never)]
    fn row(&self, index: usize, text: &str, cx: &App) -> gpui::AnyElement {
        let select_text = self.select.clone();
        let select_add = self.select.clone();
        h_flex()
            .id(format!("ai-followup-row-{index}"))
            .debug_selector(move || format!("ai-followup-row-{index}"))
            .w_full()
            .min_w_0()
            .min_h(px(40.))
            .flex_shrink_0()
            .gap_1()
            .px_1()
            .rounded(px(6.))
            .bg(cx.theme().muted)
            .hover(|row| row.bg(cx.theme().muted.mix_oklab(cx.theme().foreground, 0.96)))
            // Keep both button backgrounds transparent so feedback spans the entire row.
            .child(
                Button::new(format!("ai-followup-text-{index}"))
                    .debug_selector(move || format!("ai-followup-text-{index}"))
                    .text()
                    .small()
                    .flex_1()
                    .min_w_0()
                    .h_auto()
                    .px_2()
                    .py_2()
                    .tooltip(t!("ai_followup_send").to_string())
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .text_left()
                            .whitespace_normal()
                            .child(text.to_owned()),
                    )
                    .on_click(move |_, window, cx| select_text(index, Action::Send, window, cx)),
            )
            .child(
                Button::new(format!("ai-followup-add-{index}"))
                    .debug_selector(move || format!("ai-followup-add-{index}"))
                    .text()
                    .xsmall()
                    .w(px(28.))
                    .h(px(32.))
                    .flex_shrink_0()
                    .text_color(cx.theme().muted_foreground)
                    .icon(Icon::default().path("icons/followup-add.svg"))
                    .tooltip(t!("ai_followup_add").to_string())
                    .on_click(move |_, window, cx| select_add(index, Action::Add, window, cx)),
            )
            .into_any_element()
    }
}

impl Render for FollowupView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut content = v_flex()
            .debug_selector(|| "ai-followups".to_owned())
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .mt_2()
            .pt_3()
            .gap_1()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .px_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{}  {}", t!("ai_followup_next"), self.items.len())),
                    )
                    .child(
                        Button::new("ai-followup-toggle")
                            .debug_selector(|| "ai-followup-toggle".to_owned())
                            .ghost()
                            .xsmall()
                            .icon(if self.expanded {
                                IconName::ChevronUp
                            } else {
                                IconName::ChevronDown
                            })
                            .tooltip(
                                t!(if self.expanded {
                                    "ai_followup_collapse"
                                } else {
                                    "ai_followup_expand"
                                })
                                .to_string(),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.expanded = !this.expanded;
                                cx.notify();
                            })),
                    ),
            );
        if self.expanded {
            let visible = if self.show_more {
                self.items.len()
            } else {
                INITIAL_VISIBLE
            };
            for (index, text) in self.items.iter().take(visible).enumerate() {
                content = content.child(self.row(index, text, cx));
            }
            if self.items.len() > INITIAL_VISIBLE {
                content = content.child(
                    Button::new("ai-followup-more")
                        .debug_selector(|| "ai-followup-more".to_owned())
                        .ghost()
                        .xsmall()
                        .self_start()
                        .text_color(cx.theme().muted_foreground)
                        .label(if self.show_more {
                            t!("ai_followup_less").to_string()
                        } else {
                            t!(
                                "ai_followup_more",
                                count = self.items.len() - INITIAL_VISIBLE
                            )
                            .to_string()
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_more = !this.show_more;
                            cx.notify();
                        })),
                );
            }
        }
        content
    }
}

pub(super) fn append(draft: &str, suggestion: &str) -> String {
    let separator = if draft.is_empty() || draft.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    format!("{draft}{separator}{suggestion}")
}

#[cfg(test)]
mod tests;
