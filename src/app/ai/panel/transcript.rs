use super::*;
use crate::ai::Snapshot;
use crate::app::ai::{
    composer, markdown, review,
    tool_steps::{self, ChatItem},
};
use gpui::{FollowMode, ListAlignment, ListState, SharedString, list};
use gpui_component::text::TextView;
use std::collections::{HashMap, VecDeque};

const MARKDOWN_CACHE_ITEMS: usize = 64;
const MARKDOWN_CACHE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Default)]
struct MarkdownCache {
    entries: VecDeque<(usize, SharedString, SharedString)>,
    bytes: usize,
}

impl MarkdownCache {
    fn get(&mut self, index: usize, source: &str) -> (SharedString, SharedString) {
        if let Some(position) = self.entries.iter().position(|(key, _, _)| *key == index)
            && let Some(entry) = self.entries.remove(position)
        {
            if entry.1.as_ref() == source {
                let result = (entry.1.clone(), entry.2.clone());
                self.entries.push_back(entry);
                return result;
            }
            self.bytes -= entry.1.len() + entry.2.len();
        }
        // Keep the same resource-blocking sanitizer for every changed message.
        let text: SharedString = source.to_owned().into();
        let sanitized: SharedString = markdown::renderable(source).into();
        self.bytes += text.len() + sanitized.len();
        self.entries
            .push_back((index, text.clone(), sanitized.clone()));
        while self.entries.len() > MARKDOWN_CACHE_ITEMS || self.bytes > MARKDOWN_CACHE_BYTES {
            if let Some((_, source, rendered)) = self.entries.pop_front() {
                self.bytes -= source.len() + rendered.len();
            }
        }
        (text, sanitized)
    }
}

pub(super) struct Transcript {
    pub(super) list: ListState,
    source: Option<Rc<Snapshot>>,
    conversation_index: usize,
    items: Vec<ChatItem>,
    markdown: MarkdownCache,
    tools: HashMap<usize, (Entity<tool_steps::view::ToolGroupView>, Subscription)>,
}

impl Default for Transcript {
    fn default() -> Self {
        Self {
            // Measure only the viewport and a small margin; never measure_all().
            list: ListState::new(0, ListAlignment::Top, px(200.)),
            source: None,
            conversation_index: 0,
            items: Vec::new(),
            markdown: MarkdownCache::default(),
            tools: HashMap::new(),
        }
    }
}

impl Transcript {
    fn prepare(&mut self, snapshot: Rc<Snapshot>, index: usize) {
        let conversation = &snapshot.conversations[index];
        let previous = self
            .source
            .as_ref()
            .map(|s| &s.conversations[self.conversation_index]);
        let switched = previous.is_none_or(|old| old.id != conversation.id);
        let changed = switched
            || self
                .source
                .as_ref()
                .is_none_or(|old| !Rc::ptr_eq(old, &snapshot));
        if changed {
            let items = tool_steps::project(conversation);
            // The final row holds errors, continuation controls, feedback and suggestions.
            let count = items.len() + 1;
            if switched {
                self.markdown = MarkdownCache::default();
                self.tools.clear();
                self.list.reset(count);
                self.list.set_follow_mode(FollowMode::Tail);
            } else if let Some(previous) = previous {
                let first_changed = self
                    .items
                    .iter()
                    .zip(&items)
                    .position(|(old, new)| !same_row(old, new, previous, conversation))
                    .unwrap_or(self.items.len().min(items.len()));
                // The last reply may gain/lose typing feedback without a text change.
                let first_changed = first_changed.min(self.items.len().saturating_sub(1));
                if count == self.list.item_count() {
                    self.list.remeasure_items(first_changed..count);
                } else {
                    self.list
                        .splice(first_changed..self.list.item_count(), count - first_changed);
                }
            }
            self.items = items;
            self.source = Some(snapshot.clone());
            self.conversation_index = index;
        }
        // Preserve the established active-turn policy, including tool/approval phases.
        // Tail mode also follows late Markdown layout; any idle upward scroll disengages it.
        crate::app::ai::scroll::follow(&self.list, conversation);
    }
}

fn same_row(old: &ChatItem, new: &ChatItem, before: &Conversation, after: &Conversation) -> bool {
    match (old, new) {
        (ChatItem::Message(a), ChatItem::Message(b)) if a == b => {
            before.messages[*a].role == after.messages[*b].role
                && before.messages[*a].content == after.messages[*b].content
        }
        (ChatItem::Tools(a), ChatItem::Tools(b)) => a == b,
        _ => false,
    }
}

impl AiPanel {
    #[inline(never)]
    pub(super) fn render_transcript(
        &mut self,
        snapshot: Rc<Snapshot>,
        index: usize,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        self.transcript.prepare(snapshot.clone(), index);
        // Local errors and suggestion expansion can change independently of service data.
        let footer = self.transcript.items.len();
        self.transcript.list.remeasure_items(footer..footer + 1);
        let panel = cx.entity().downgrade();
        let rows = list(self.transcript.list.clone(), move |row, window, cx| {
            let Some(panel) = panel.upgrade() else {
                return div().into_any_element();
            };
            panel.update(cx, |this, cx| {
                let c = &snapshot.conversations[index];
                let content = this.render_transcript_row(row, c, &snapshot, window, cx);
                div()
                    .w_full()
                    .min_w_0()
                    .px_3()
                    .pb_3()
                    .when(row == 0, |row| row.pt_3())
                    .child(content)
                    .into_any_element()
            })
        })
        .size_full();
        div()
            .id("ai-messages")
            .debug_selector(|| "ai-messages".to_owned())
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .child(rows)
            .into_any_element()
    }

    #[inline(never)]
    fn render_transcript_row(
        &mut self,
        row: usize,
        c: &Conversation,
        snapshot: &Snapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let feedback = composer::feedback_state(c);
        let dots_message = if feedback == composer::TurnFeedback::Dots
            && c.error.as_ref().or(self.error.as_ref()).is_none()
        {
            self.transcript.items.last().and_then(|item| match item {
                ChatItem::Message(index) if c.messages[*index].role == "assistant" => Some(*index),
                _ => None,
            })
        } else {
            None
        };
        let ix = match self.transcript.items.get(row) {
            Some(ChatItem::Message(index)) => *index,
            Some(ChatItem::Tools(group)) => {
                // Keep expansion state while a tool row is outside the virtual viewport.
                let (trace, _) = self.transcript.tools.entry(group.key).or_insert_with(|| {
                    let review = review::actions(c, self.owner.clone());
                    let trace =
                        cx.new(|_| tool_steps::view::ToolGroupView::new(group.clone(), review));
                    let subscription = cx.observe(&trace, |_, _, cx| cx.notify());
                    (trace, subscription)
                });
                trace.update(cx, |view, cx| view.update_group(group.clone(), cx));
                return trace.clone().into_any_element();
            }
            None => return self.render_transcript_footer(c, snapshot, dots_message, window, cx),
        };
        let message = &c.messages[ix];
        if message.content.is_empty() {
            return div().into_any_element();
        }
        let (copy, text) = self.transcript.markdown.get(ix, &message.content);
        let row = v_flex()
            .id(format!("ai-message-row-{ix}"))
            .debug_selector(move || format!("ai-message-row-{ix}"))
            .w_full()
            .min_w_0()
            .gap_1()
            .when(message.role == "user", |row| {
                row.p_2()
                    .rounded(px(6.))
                    .bg(cx.theme().primary.opacity(0.08))
            })
            .child(
                h_flex()
                    .justify_between()
                    .child(note(
                        t!(if message.role == "user" {
                            "ai_you"
                        } else if message.role == "tool" {
                            "ai_tool_result"
                        } else {
                            "ai_assistant"
                        })
                        .to_string(),
                        cx,
                    ))
                    .child(
                        Button::new(format!("ai-copy-{ix}"))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Copy)
                            .tooltip(t!("ai_copy").to_string())
                            .on_click(move |_, window, cx| {
                                crate::feedback::Feedback::copy(window, cx, copy.to_string());
                            }),
                    ),
            )
            .child(
                TextView::markdown(format!("ai-message-{}-{ix}", c.id), text)
                    .selectable(true)
                    .code_block_actions(|block, _, _| {
                        let code = block.code().to_string();
                        Button::new("ai-copy-code")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Copy)
                            .on_click(move |_, window, cx| {
                                crate::feedback::Feedback::copy(window, cx, code.clone());
                            })
                    }),
            )
            .when(dots_message == Some(ix), |reply| {
                reply.child(composer::animated_dots(cx))
            });
        row.into_any_element()
    }

    #[inline(never)]
    fn render_transcript_footer(
        &mut self,
        c: &Conversation,
        snapshot: &Snapshot,
        dots_message: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let feedback = composer::feedback_state(c);
        let mut messages = v_flex().w_full().min_w_0().gap_3();
        if c.messages.is_empty() {
            messages = messages.child(
                v_flex()
                    .w_full()
                    .items_center()
                    .gap_3()
                    .pt_6()
                    .child(crate::app::ai::icon().size_8())
                    .child(div().child(t!("ai_start").to_string()))
                    .child(note(t!("ai_privacy_empty").to_string(), cx)),
            );
            if snapshot.settings.providers.is_empty() {
                messages = messages.child(
                    Button::new("ai-configure")
                        .small()
                        .label(t!("ai_open_settings").to_string())
                        .on_click(
                            cx.listener(|this, _, window, cx| this.settings(4, None, window, cx)),
                        ),
                );
            }
        }
        if let Some(error) = c.error.as_ref().or(self.error.as_ref()) {
            messages = messages.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(error_text(error)),
            );
        } else if matches!(c.state, TurnState::Failed | TurnState::Interrupted) {
            messages = messages.child(note(t!(state_key(c.state)).to_string(), cx));
        }
        if let Some(pause) = &c.continuation {
            let id = c.id.clone();
            let token = pause.token.clone();
            let stop_id = id.clone();
            let stop_token = token.clone();
            messages = messages.child(
                v_flex()
                    .gap_2()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .pt_3()
                    .child(div().child(t!("ai_waiting_continuation").to_string()))
                    .child(note(crate::app::ai::attention::description(pause), cx))
                    .child(
                        h_flex()
                            .gap_2()
                            .flex_wrap()
                            .child(
                                Button::new("ai-resume-paused")
                                    .small()
                                    .primary()
                                    .label(t!("ai_continue_turn").to_string())
                                    .on_click(move |_, _, _| {
                                        if let Ok(s) = service() {
                                            let _ = s.resume(&id, &token);
                                        }
                                    }),
                            )
                            .child(
                                Button::new("ai-stop-paused")
                                    .small()
                                    .outline()
                                    .label(t!("ai_terminate_turn").to_string())
                                    .on_click(move |_, _, _| {
                                        if let Ok(s) = service() {
                                            let _ = s.terminate_continuation(&stop_id, &stop_token);
                                        }
                                    }),
                            ),
                    ),
            );
        }
        if dots_message.is_none()
            && let Some(indicator) = composer::render_feedback(feedback, cx)
        {
            messages = messages.child(indicator);
        }
        if snapshot.settings.execution.followup_suggestions
            && let Some(suggestions) = self.render_followups(c, window, cx)
        {
            messages = messages.child(suggestions);
        }
        messages.into_any_element()
    }
}

#[cfg(test)]
#[path = "transcript_tests.rs"]
mod tests;
