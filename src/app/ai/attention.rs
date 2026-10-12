//! One native window owns each pending choice. Rendering schedules UI work without replaying it.
use super::{AiPanel, notifications, service};
use crate::{
    TinyShell,
    ai::{Continuation, LimitKind},
    app::{confirmation_dialog::ConfirmationDialog, runtime_state::DialogToken},
};
use gpui::{AnyWindowHandle, App, Context, Entity, Window};
use gpui_component::WindowExt as _;
use rust_i18n::t;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
};

#[derive(Default)]
struct Attention {
    owners: HashMap<String, AnyWindowHandle>,
    notified: HashSet<String>,
    scheduled: HashMap<String, AnyWindowHandle>,
    visible: Vec<Visible>,
    preferred: Option<(String, String)>,
}
struct Visible {
    window: AnyWindowHandle,
    pause: String,
    modal: DialogToken,
}
thread_local! { static ATTENTION: RefCell<Attention> = RefCell::new(Attention::default()); }

pub(super) fn bind(id: String, window: AnyWindowHandle) {
    ATTENTION.with(|state| {
        state.borrow_mut().owners.insert(id, window);
    });
}
fn live(id: &str, token: &str) -> bool {
    service().is_ok_and(|s| s.continuation_live(id, token))
}
fn forget(token: &str) {
    ATTENTION.with(|state| {
        let mut state = state.borrow_mut();
        state.scheduled.remove(token);
        state.visible.retain(|v| v.pause != token);
        if state
            .preferred
            .as_ref()
            .is_some_and(|(_, pause)| pause == token)
        {
            state.preferred = None;
        }
    });
}
fn owner(id: &str, cx: &App) -> Option<(AnyWindowHandle, Entity<TinyShell>)> {
    let registry = crate::app::window_registry();
    let windows = registry.lock().ok()?;
    let bound = ATTENTION.with(|state| state.borrow().owners.get(id).copied());
    let entry = windows
        .iter()
        .find(|e| Some(e.window_handle) == bound)
        .or_else(|| {
            windows
                .iter()
                .find(|e| Some(e.window_handle) == cx.active_window())
        })
        .or_else(|| windows.iter().max_by_key(|e| e.activation_seq))?;
    Some((entry.window_handle, entry.entity.clone()))
}
pub(super) fn dispatch_activations(cx: &mut App) {
    for activation in notifications::take_activations() {
        cx.defer(move |cx| {
            // A notification only opens a live choice. It never authorizes remote execution.
            if !live(&activation.conversation, &activation.token) {
                return;
            }
            if let Some((handle, entity)) = owner(&activation.conversation, cx) {
                ATTENTION.with(|state| {
                    state.borrow_mut().preferred =
                        Some((activation.conversation.clone(), activation.token.clone()))
                });
                let _ = handle.update(cx, |_, window, cx| {
                    entity.update(cx, |this, cx| {
                        let previous = ATTENTION.with(|state| {
                            state
                                .borrow()
                                .visible
                                .iter()
                                .find(|v| v.window == handle && v.pause != activation.token)
                                .map(|v| (v.modal, v.pause.clone()))
                        });
                        if let Some((modal, pause)) = previous {
                            forget(&pause);
                            this.dismiss_modal_dialog(modal, window, cx);
                        }
                        this.open_ai_conversation(activation.conversation.clone(), window, cx);
                        window.activate_window();
                    });
                });
            }
        });
    }
}

pub(super) fn description(pause: &Continuation) -> String {
    match pause.kind {
        LimitKind::ToolCalls => t!(
            "ai_continue_tools_description",
            used = pause.used,
            allowance = pause.allowance
        )
        .to_string(),
        LimitKind::ModelRequests => t!(
            "ai_continue_models_description",
            used = pause.used,
            allowance = pause.allowance
        )
        .to_string(),
    }
}
fn label(pause: &Continuation) -> String {
    match pause.kind {
        LimitKind::ToolCalls => t!("ai_continue_tools", count = pause.allowance).to_string(),
        LimitKind::ModelRequests => t!("ai_continue_models", count = pause.allowance).to_string(),
    }
}

impl TinyShell {
    fn open_ai_conversation(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if !self.ai_open {
            self.toggle_ai(window, cx);
        }
        if let Some(panel) = &self.ai_panel {
            panel.update(cx, |p: &mut AiPanel, cx| {
                p.select_conversation(id.clone(), cx)
            });
        }
        cx.notify();
    }

    pub(crate) fn present_ai_attention(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Ok(service) = service() else {
            return;
        };
        let snapshot = service.attention();
        let tokens: HashSet<_> = snapshot
            .pending
            .iter()
            .map(|c| c.choice.token.clone())
            .collect();
        let handles = crate::app::window_registry()
            .lock()
            .map(|windows| {
                windows
                    .iter()
                    .map(|entry| entry.window_handle)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let stale = ATTENTION.with(|state| {
            let mut state = state.borrow_mut();
            state.notified.retain(|token| tokens.contains(token));
            state.owners.retain(|id, _| snapshot.active.contains(id));
            // A paused runner survives closing one of several windows. Release its old
            // presentation claim so the remaining window can own the same live choice.
            state
                .scheduled
                .retain(|token, handle| tokens.contains(token) && handles.contains(handle));
            state
                .visible
                .retain(|visible| handles.contains(&visible.window));
            if state
                .preferred
                .as_ref()
                .is_some_and(|(_, token)| !tokens.contains(token))
            {
                state.preferred = None;
            }
            state
                .visible
                .iter()
                .find(|v| v.window == window.window_handle() && !tokens.contains(&v.pause))
                .map(|v| (v.modal, v.pause.clone()))
        });
        if let Some((modal, pause)) = stale {
            forget(&pause);
            let view = cx.entity();
            window.defer(cx, move |window, cx| {
                view.update(cx, |this, cx| {
                    this.dismiss_modal_dialog(modal, window, cx);
                    cx.notify();
                });
            });
            return;
        }
        let mut pending: Vec<_> = snapshot.pending.iter().collect();
        pending.sort_by_key(|c| c.updated);
        let preferred = ATTENTION.with(|state| state.borrow().preferred.clone());
        pending.sort_by_key(|c| {
            !preferred
                .as_ref()
                .is_some_and(|(id, token)| id == &c.conversation && token == &c.choice.token)
        });
        for c in &pending {
            if owner(&c.conversation, cx).is_none_or(|(handle, _)| handle != window.window_handle())
            {
                continue;
            }
            let pause = &c.choice;
            bind(c.conversation.clone(), window.window_handle());
            if snapshot.background_notifications && cx.active_window().is_none() {
                let first =
                    ATTENTION.with(|state| state.borrow_mut().notified.insert(pause.token.clone()));
                if first {
                    notifications::paused(c.conversation.clone(), pause.token.clone());
                }
            }
        }
        if cx.active_window() != Some(window.window_handle()) || window.has_active_dialog(cx) {
            return;
        }
        let Some(c) = pending.into_iter().find(|c| {
            owner(&c.conversation, cx).is_some_and(|(handle, _)| handle == window.window_handle())
        }) else {
            return;
        };
        let pause = c.choice.clone();
        if !ATTENTION.with(|state| {
            state
                .borrow_mut()
                .scheduled
                .insert(pause.token.clone(), window.window_handle())
                .is_none()
        }) {
            return;
        }
        let id = c.conversation.clone();
        let title = c.title.clone();
        let view = cx.entity();
        window.defer(cx, move |window, cx| {
            if !live(&id, &pause.token) || window.has_active_dialog(cx) {
                forget(&pause.token);
                return;
            }
            view.update(cx, |this, cx| {
                this.show_ai_continuation(id.clone(), title.clone(), pause.clone(), window, cx)
            });
        });
    }

    fn show_ai_continuation(
        &mut self,
        id: String,
        title: String,
        pause: Continuation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity();
        let pending_token = pause.token.clone();
        let result = self.open_modal_dialog(
            crate::app::DialogKind::AiContinuation,
            window,
            cx,
            move |dialog, token, window, cx| {
                ATTENTION.with(|state| {
                    let mut state = state.borrow_mut();
                    if !state.visible.iter().any(|v| v.pause == pause.token) {
                        state.visible.push(Visible {
                            window: window.window_handle(),
                            pause: pause.token.clone(),
                            modal: token,
                        });
                    }
                });
                let confirm_id = id.clone();
                let confirm_token = pause.token.clone();
                let cancel_id = id.clone();
                let cancel_token = pause.token.clone();
                let close_token = pause.token.clone();
                let close_view = view.clone();
                ConfirmationDialog::new(
                    t!("ai_continuation_title").to_string(),
                    format!("{}\n\n{}", title, description(&pause)),
                )
                .confirm_label(label(&pause))
                .cancel_label(t!("ai_terminate_turn").to_string())
                // Escape/Enter outside a focused button must not silently decide a paused turn.
                .keyboard(false)
                .on_ok(move |_, _, _| {
                    service().is_ok_and(|s| s.resume(&confirm_id, &confirm_token).is_ok())
                        || !live(&confirm_id, &confirm_token)
                })
                .on_cancel(move |_, _, _| {
                    service()
                        .is_ok_and(|s| s.terminate_continuation(&cancel_id, &cancel_token).is_ok())
                        || !live(&cancel_id, &cancel_token)
                })
                .on_close(move |_, window, cx| {
                    forget(&close_token);
                    close_view.update(cx, |this, cx| {
                        this.modal_dialog_closed(token, window, cx);
                        cx.notify();
                    });
                })
                .build(dialog, window, cx)
            },
        );
        if result != crate::app::DialogOpenResult::Opened {
            ATTENTION.with(|state| {
                state.borrow_mut().scheduled.remove(&pending_token);
            });
        }
    }
}
