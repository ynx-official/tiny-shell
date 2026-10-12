use super::{error_text, service, target};
use crate::{
    TinyShell,
    ai::{BoundTarget, Conversation, TurnState},
    app::confirmation_dialog::ConfirmationDialog,
};
use gpui::{
    AppContext as _, Context, Entity, InteractiveElement as _, IntoElement as _, MouseButton,
    ParentElement as _, Render, ScrollHandle, StatefulInteractiveElement as _, Styled as _,
    Subscription, WeakEntity, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
    text::TextView,
    v_flex,
};
use rust_i18n::t;

pub(crate) struct AiPanel {
    owner: WeakEntity<TinyShell>,
    current: Option<String>,
    history: bool,
    input: Entity<InputState>,
    search: Entity<InputState>,
    rename: Entity<InputState>,
    rename_id: Option<String>,
    scroll: ScrollHandle,
    scroll_stamp: Option<(String, usize, usize)>,
    error: Option<String>,
    pub width: f32,
    drag: Option<(f32, f32)>,
    _subscriptions: Vec<Subscription>,
}
impl AiPanel {
    pub fn new(owner: Entity<TinyShell>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            crate::app::localization::localized_input(window, cx, || {
                t!("ai_input_hint").to_string()
            })
            .auto_grow(3, 7)
            .submit_on_enter(true)
        });
        let search = cx.new(|cx| {
            crate::app::localization::localized_input(window, cx, || t!("ai_search").to_string())
        });
        let rename = cx.new(|cx| InputState::new(window, cx));
        let subscriptions = vec![
            cx.observe(&owner, |_, _, cx| cx.notify()),
            cx.subscribe_in(&input, window, |this, _, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { shift: false, .. }) {
                    window.prevent_default();
                    cx.stop_propagation();
                    this.send(window, cx);
                } else {
                    cx.notify();
                }
            }),
            cx.subscribe(&search, |_, _, _: &InputEvent, cx| cx.notify()),
            cx.subscribe(&rename, |_, _, _: &InputEvent, cx| cx.notify()),
        ];
        Self {
            owner: owner.downgrade(),
            current: None,
            history: false,
            input,
            search,
            rename,
            rename_id: None,
            scroll: ScrollHandle::new(),
            scroll_stamp: None,
            error: None,
            width: 420.,
            drag: None,
            _subscriptions: subscriptions,
        }
    }
    fn selected_target(&self, cx: &gpui::App) -> Option<BoundTarget> {
        let owner = self.owner.upgrade()?;
        let owner = owner.read(cx);
        let tab = owner
            .workspace()
            .active_tab_id()
            .and_then(|id| owner.workspace().terminal_tab(id))?;
        let session = tab.session.as_ref()?;
        super::current_target(session, &owner.config)
    }
    fn create(&mut self, cx: &mut Context<Self>) {
        match service().and_then(|s| {
            s.create(self.selected_target(cx))
                .map_err(|e| e.to_string())
        }) {
            Ok(id) => {
                self.current = Some(id);
                self.history = false;
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
        cx.notify();
    }
    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.current.clone() else {
            return;
        };
        let text = self.input.read(cx).value().to_string();
        match service().and_then(|s| s.send(id, text).map_err(|e| e.to_string())) {
            Ok(()) => {
                self.input.update(cx, |i, cx| i.set_value("", window, cx));
                self.scroll.scroll_to_bottom();
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
        cx.notify();
    }
    fn settings(
        &self,
        index: usize,
        task: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(owner) = self.owner.upgrade() {
            owner.update(cx, |o, cx| {
                o.mcp_requested_task = task;
                o.show_ai_settings(index, window, cx);
            });
        }
    }
    fn confirm_delete(&self, ids: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        ConfirmationDialog::new(
            t!("ai_delete_title").to_string(),
            t!("ai_delete_description").to_string(),
        )
        .danger(true)
        .confirm_label(t!("delete").to_string())
        .on_ok(move |_, _, _| {
            if let Ok(service) = service() {
                service.delete(ids.clone());
                true
            } else {
                false
            }
        })
        .open(window, cx);
    }
    fn render_history(
        &mut self,
        snapshot: &crate::ai::Snapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let query = self.search.read(cx).value().trim().to_lowercase();
        let mut list = v_flex()
            .id("ai-history-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .gap_1();
        let mut conversations = snapshot
            .conversations
            .iter()
            .filter(|c| {
                !c.messages.is_empty()
                    && format!(
                        "{} {}",
                        c.title,
                        c.target
                            .as_ref()
                            .map(|t| t.name.as_str())
                            .unwrap_or_default()
                    )
                    .to_lowercase()
                    .contains(&query)
            })
            .collect::<Vec<_>>();
        conversations.sort_by_key(|c| std::cmp::Reverse(c.updated));
        if conversations.is_empty() {
            list = list.child(note(t!("ai_no_history").to_string(), cx));
        }
        for c in conversations {
            let id = c.id.clone();
            let open = id.clone();
            let delete = id.clone();
            let rename = id.clone();
            let title = c.title.clone();
            list = list.child(
                h_flex()
                    .gap_1()
                    .p_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        Button::new(format!("ai-history-{id}"))
                            .ghost()
                            .small()
                            .flex_1()
                            .min_w_0()
                            .h_auto()
                            .child(
                                v_flex()
                                    .min_w_0()
                                    .w_full()
                                    .items_start()
                                    .gap_1()
                                    .child(
                                        div()
                                            .w_full()
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .child(c.title.clone()),
                                    )
                                    .child(note(
                                        format!(
                                            "{} · {} · {}",
                                            c.target
                                                .as_ref()
                                                .map(|t| t.name.clone())
                                                .unwrap_or_else(|| t!("ai_plain_chat").to_string()),
                                            t!(
                                                "ai_message_count",
                                                count = c
                                                    .messages
                                                    .iter()
                                                    .filter(|m| m.role == "user"
                                                        || m.role == "assistant")
                                                    .count()
                                            ),
                                            chrono::DateTime::from_timestamp(c.updated, 0)
                                                .map(|date| date
                                                    .with_timezone(&chrono::Local)
                                                    .format("%m-%d %H:%M")
                                                    .to_string())
                                                .unwrap_or_default()
                                        ),
                                        cx,
                                    )),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.current = Some(open.clone());
                                this.history = false;
                                this.error = None;
                                cx.notify();
                            })),
                    )
                    .when(c.state.active(), |row| {
                        row.child(note(t!(state_key(c.state)).to_string(), cx))
                    })
                    .child(
                        Button::new(format!("ai-rename-{id}"))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Replace)
                            .tooltip(t!("ai_rename").to_string())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.rename_id = Some(rename.clone());
                                this.rename
                                    .update(cx, |i, cx| i.set_value(title.clone(), window, cx));
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new(format!("ai-delete-{id}"))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Delete)
                            .tooltip(t!("delete").to_string())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.confirm_delete(vec![delete.clone()], window, cx)
                            })),
                    ),
            );
        }
        let mut content = v_flex()
            .size_full()
            .gap_3()
            .p_3()
            .child(
                h_flex()
                    .justify_between()
                    .child(div().child(t!("ai_history").to_string()))
                    .child(
                        Button::new("ai-history-new")
                            .small()
                            .outline()
                            .icon(IconName::Plus)
                            .label(t!("ai_new").to_string())
                            .on_click(cx.listener(|this, _, _, cx| this.create(cx))),
                    ),
            )
            .child(Input::new(&self.search).small())
            .child(list);
        if let Some(id) = self.rename_id.clone() {
            content = content.child(
                h_flex()
                    .gap_2()
                    .child(Input::new(&self.rename).small())
                    .child(
                        Button::new("ai-rename-save")
                            .small()
                            .label(t!("save").to_string())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Ok(s) = service() {
                                    s.rename(id.clone(), this.rename.read(cx).value().to_string());
                                    this.rename_id = None;
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("ai-rename-cancel")
                            .small()
                            .ghost()
                            .icon(IconName::Close)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.rename_id = None;
                                cx.notify();
                            })),
                    ),
            );
        }
        let ids = snapshot
            .conversations
            .iter()
            .map(|c| c.id.clone())
            .collect::<Vec<_>>();
        let _ = window;
        content
            .child(
                h_flex()
                    .justify_between()
                    .child(note(t!("ai_local_history").to_string(), cx))
                    .child(
                        Button::new("ai-clear-history")
                            .ghost()
                            .xsmall()
                            .label(t!("ai_clear_history").to_string())
                            .disabled(ids.is_empty())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.confirm_delete(ids.clone(), window, cx)
                            })),
                    ),
            )
            .into_any_element()
    }
    fn render_conversation(
        &mut self,
        c: &Conversation,
        snapshot: &crate::ai::Snapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let stamp = (
            c.id.clone(),
            c.messages.len(),
            c.messages.last().map_or(0, |m| m.content.len()),
        );
        if self.scroll_stamp.as_ref() != Some(&stamp) {
            let new_conversation = self.scroll_stamp.as_ref().is_none_or(|old| old.0 != c.id);
            if new_conversation || self.scroll.max_offset().y + self.scroll.offset().y <= px(32.) {
                self.scroll.scroll_to_bottom();
            }
            self.scroll_stamp = Some(stamp);
        }
        let mut messages = v_flex()
            .id("ai-messages")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .gap_3()
            .p_3();
        if c.messages.is_empty() {
            messages = messages.child(
                v_flex()
                    .w_full()
                    .items_center()
                    .gap_3()
                    .pt_6()
                    .child(super::icon().size_8())
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
        for item in super::tool_steps::project(c) {
            let ix = match item {
                super::tool_steps::ChatItem::Message(index) => index,
                super::tool_steps::ChatItem::Tools(group) => {
                    let owner = cx.entity().downgrade();
                    let review: super::tool_steps::view::ReviewAction =
                        std::rc::Rc::new(move |task, window, cx| {
                            if let Some(owner) = owner.upgrade() {
                                owner.update(cx, |this, cx| {
                                    this.settings(5, Some(task), window, cx)
                                });
                            }
                        });
                    let initial = group.clone();
                    let trace = window.use_keyed_state(
                        format!("ai-steps-{}-{}", c.id, group.key),
                        cx,
                        |_, _| super::tool_steps::view::ToolGroupView::new(initial, review),
                    );
                    trace.update(cx, |view, cx| view.update_group(group, cx));
                    messages = messages.child(trace);
                    continue;
                }
            };
            let message = &c.messages[ix];
            if message.content.is_empty() {
                continue;
            }
            let text = message.content.clone();
            let copy = text.clone();
            messages = messages.child(
                v_flex()
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
                                    .on_click(move |_, _, cx| {
                                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                            copy.clone(),
                                        ))
                                    }),
                            ),
                    )
                    .child(
                        TextView::markdown(
                            format!("ai-message-{}-{ix}", c.id),
                            super::markdown::renderable(&text),
                        )
                        .selectable(true)
                        .code_block_actions(|block, _, _| {
                            let code = block.code().to_string();
                            Button::new("ai-copy-code")
                                .ghost()
                                .xsmall()
                                .icon(IconName::Copy)
                                .on_click(move |_, _, cx| {
                                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                        code.clone(),
                                    ))
                                })
                        }),
                    ),
            );
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
        if super::composer::turn_in_progress(c) {
            messages = messages.child(super::composer::processing(cx));
        }
        let target_button = Button::new("ai-target").ghost().xsmall().label(
            c.target
                .as_ref()
                .map(|t| t.name.clone())
                .unwrap_or_else(|| t!("ai_plain_chat").to_string()),
        );
        let target_button = if c.messages.is_empty() {
            let view = cx.entity();
            let id = c.id.clone();
            let pid = c.provider_id.clone();
            let model = c.model.clone();
            let targets = self
                .owner
                .upgrade()
                .map(|o| {
                    o.read(cx)
                        .config
                        .sessions()
                        .iter()
                        .filter(|s| {
                            s.connection_type == crate::session::config::ConnectionType::Ssh
                        })
                        .map(|s| target(s, &o.read(cx).config))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            target_button
                .dropdown_menu(move |mut menu, window, cx| {
                    menu = menu.scrollable(true).max_h(px(280.));
                    let id0 = id.clone();
                    let p0 = pid.clone();
                    let m0 = model.clone();
                    menu = menu.item(
                        PopupMenuItem::new(t!("ai_plain_chat").to_string()).on_click(
                            window.listener_for(&view, move |_, _, _, _| {
                                if let Ok(s) = service() {
                                    let _ = s.configure_conversation(
                                        &id0,
                                        p0.clone(),
                                        m0.clone(),
                                        None,
                                    );
                                }
                            }),
                        ),
                    );
                    for target in &targets {
                        let target = target.clone();
                        let cid = id.clone();
                        let pid = pid.clone();
                        let model = model.clone();
                        menu = menu.item(PopupMenuItem::new(target.name.clone()).on_click(
                            window.listener_for(&view, move |_, _, _, _| {
                                if let Ok(s) = service() {
                                    let _ = s.configure_conversation(
                                        &cid,
                                        pid.clone(),
                                        model.clone(),
                                        Some(target.clone()),
                                    );
                                }
                            }),
                        ));
                    }
                    let _ = cx;
                    menu
                })
                .into_any_element()
        } else {
            target_button.into_any_element()
        };
        let view = cx.entity();
        let id = c.id.clone();
        let target = c.target.clone();
        let providers = snapshot.settings.providers.clone();
        let model_label = snapshot
            .settings
            .providers
            .iter()
            .find(|p| Some(&p.id) == c.provider_id.as_ref())
            .map(|p| format!("{} · {}", p.name, c.model))
            .unwrap_or_else(|| t!("ai_choose_model").to_string());
        let model_button = Button::new("ai-model")
            .ghost()
            .xsmall()
            .w_full()
            .min_w_0()
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .text_left()
                    .child(model_label),
            )
            .disabled(c.state.active())
            .dropdown_menu(move |mut menu, window, _| {
                for p in providers.iter().filter(|p| p.enabled) {
                    for m in &p.models {
                        let cid = id.clone();
                        let pid = p.id.clone();
                        let model = m.id.clone();
                        let target = target.clone();
                        menu = menu.item(
                            PopupMenuItem::new(format!("{} · {}", p.name, m.id)).on_click(
                                window.listener_for(&view, move |_, _, _, _| {
                                    if let Ok(s) = service() {
                                        let _ = s.configure_conversation(
                                            &cid,
                                            Some(pid.clone()),
                                            model.clone(),
                                            target.clone(),
                                        );
                                    }
                                }),
                            ),
                        );
                    }
                }
                menu
            });
        let policy = super::super::mcp::service()
            .ok()
            .and_then(|s| s.engine.ai_policy().ok());
        let target_current = c.target.as_ref().is_some_and(|bound| {
            self.owner.upgrade().is_some_and(|owner| {
                let owner = owner.read(cx);
                owner.config.sessions().iter().any(|session| {
                    let current = super::target(session, &owner.config);
                    session.connection_type == crate::session::config::ConnectionType::Ssh
                        && current.id == bound.id
                        && current.digest == bound.digest
                })
            })
        });
        let supports_tools = snapshot
            .settings
            .providers
            .iter()
            .find(|p| Some(&p.id) == c.provider_id.as_ref())
            .is_some_and(|p| p.models.iter().any(|m| m.id == c.model && m.supports_tools));
        let permission_label = t!(super::ssh_status_key(
            policy,
            c.target.is_some(),
            target_current,
            supports_tools
        ))
        .to_string();
        let cid = c.id.clone();
        let active = c.state.active();
        v_flex()
            .size_full()
            .min_h_0()
            .child(
                h_flex()
                    .gap_2()
                    .p_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        Button::new("ai-back")
                            .ghost()
                            .xsmall()
                            .icon(IconName::ArrowLeft)
                            .tooltip(t!("ai_history").to_string())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.history = true;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .child(if c.title.is_empty() {
                                t!("ai_new").to_string()
                            } else {
                                c.title.clone()
                            }),
                    ),
            )
            .child(messages)
            .child(
                v_flex()
                    .gap_2()
                    .p_3()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(super::composer::surface(
                        &self.input,
                        active,
                        h_flex().flex_wrap().gap_1().child(target_button).child(
                            Button::new("ai-permissions")
                                .ghost()
                                .xsmall()
                                .label(permission_label)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.settings(5, None, window, cx)
                                })),
                        ),
                        model_button,
                        Button::new("ai-send")
                            .small()
                            .ghost()
                            .icon(if active {
                                IconName::Pause
                            } else {
                                IconName::ArrowUp
                            })
                            .disabled(!active && self.input.read(cx).value().trim().is_empty())
                            .tooltip(t!(if active { "ai_stop" } else { "ai_send" }).to_string())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if active {
                                    if let Ok(s) = service() {
                                        s.stop(&cid);
                                    }
                                } else {
                                    this.send(window, cx);
                                }
                            })),
                        cx,
                    )),
            )
            .into_any_element()
    }
}
impl Render for AiPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        let mut snapshot = service().map(|s| s.snapshot()).unwrap_or_default();
        if self.current.is_none() && !self.history && snapshot.ready {
            self.create(cx);
            snapshot = service().map(|s| s.snapshot()).unwrap_or_default();
        }
        if self
            .current
            .as_ref()
            .is_some_and(|id| !snapshot.conversations.iter().any(|c| &c.id == id))
            && snapshot.ready
        {
            self.current = None;
            self.history = true;
        }
        let body = if self.history {
            self.render_history(&snapshot, window, cx)
        } else if let Some(c) = snapshot
            .conversations
            .iter()
            .find(|c| Some(&c.id) == self.current.as_ref())
        {
            self.render_conversation(c, &snapshot, window, cx)
        } else {
            note(t!("loading").to_string(), cx).into_any_element()
        };
        let owner = self.owner.clone();
        let mut panel = v_flex()
            .relative()
            .size_full()
            .min_w_0()
            .min_h_0()
            .bg(cx.theme().background)
            .border_l_1()
            .border_color(cx.theme().border)
            .text_sm()
            .child(
                h_flex()
                    .gap_2()
                    .p_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(super::icon())
                    .child(div().flex_1().child(t!("ai_assistant").to_string()))
                    .child(
                        Button::new("ai-new")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Plus)
                            .tooltip(t!("ai_new").to_string())
                            .on_click(cx.listener(|this, _, _, cx| this.create(cx))),
                    )
                    .child(
                        Button::new("ai-settings")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Settings)
                            .tooltip(t!("ai_open_settings").to_string())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.settings(4, None, window, cx)
                            })),
                    )
                    .child(
                        Button::new("ai-close")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Close)
                            .tooltip(t!("ai_close").to_string())
                            .on_click(move |_, _, cx| {
                                if let Some(owner) = owner.upgrade() {
                                    owner.update(cx, |o, cx| {
                                        o.ai_open = false;
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
            );
        if snapshot.temporary {
            panel = panel.child(
                div()
                    .p_2()
                    .text_xs()
                    .text_color(cx.theme().warning)
                    .child(t!("ai_temporary").to_string()),
            );
        }
        if let Some(error) = snapshot.storage_error.as_ref() {
            panel = panel.child(
                div()
                    .p_2()
                    .text_xs()
                    .text_color(cx.theme().danger)
                    .child(error_text(error)),
            );
        }
        if let Some(notice) = snapshot.history_notice.as_ref().filter(|_| self.history) {
            panel = panel.child(
                note(error_text(notice), cx).text_color(if notice == "AI_SAVED" {
                    cx.theme().muted_foreground
                } else {
                    cx.theme().danger
                }),
            );
        }
        panel.child(div().flex_1().min_h_0().child(body)).child(
            div()
                .id("ai-resize")
                .absolute()
                .left_0()
                .top_0()
                .bottom_0()
                .w(px(5.))
                .cursor_col_resize()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                        this.drag = Some((event.position.x.as_f32(), this.width));
                        cx.notify();
                    }),
                ),
        )
    }
}
impl AiPanel {
    pub(crate) fn drag_move(&mut self, event: &gpui::MouseMoveEvent, cx: &mut Context<Self>) {
        if event.pressed_button != Some(MouseButton::Left) {
            self.drag = None;
            return;
        }
        if let Some((origin, width)) = self.drag {
            self.width = (width + origin - event.position.x.as_f32()).clamp(360., 600.);
            if let Some(owner) = self.owner.upgrade() {
                owner.update(cx, |_, cx| cx.notify());
            }
            cx.notify();
        }
    }
    pub(crate) fn end_drag(&mut self) {
        self.drag = None;
    }
}
pub(crate) fn note(text: String, cx: &gpui::App) -> gpui::Div {
    div()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text)
}
fn state_key(state: TurnState) -> &'static str {
    match state {
        TurnState::Idle => "ai_ready",
        TurnState::Generating => "ai_generating",
        TurnState::AwaitingApproval => "ai_waiting_approval",
        TurnState::RunningTool => "ai_running_tool",
        TurnState::Failed => "ai_failed",
        TurnState::Interrupted => "ai_interrupted",
    }
}
