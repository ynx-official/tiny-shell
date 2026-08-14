use std::{
    cell::RefCell,
    sync::atomic::{AtomicU64, Ordering},
};

use gpui::{AnyWindowHandle, App, Context, Window};
use gpui_component::{WindowExt as _, dialog::Dialog};

use crate::{
    TinyShell,
    app::{DialogKind, DialogOpenResult, runtime_state::DialogToken},
};

type ModalBuilder = Box<dyn Fn(Dialog, DialogToken, &mut Window, &mut App) -> Dialog>;

#[derive(Clone, Copy)]
struct ActiveModal {
    kind: DialogKind,
    token: DialogToken,
}

struct PendingModal {
    kind: DialogKind,
    token: DialogToken,
    builder: ModalBuilder,
}

struct WindowModalState {
    window: AnyWindowHandle,
    active: Option<ActiveModal>,
    pending: Option<PendingModal>,
}

thread_local! {
    /// GPUI UI work is single-threaded. Keeping builders in thread-local state lets each native
    /// window own an independent modal queue without imposing Send/Sync on UI closures.
    static WINDOW_MODALS: RefCell<Vec<WindowModalState>> = const { RefCell::new(Vec::new()) };
}

static NEXT_MODAL_TOKEN: AtomicU64 = AtomicU64::new(1);

fn next_token() -> DialogToken {
    DialogToken {
        generation: NEXT_MODAL_TOKEN.fetch_add(1, Ordering::Relaxed),
    }
}

fn open_request(request: PendingModal, window: &mut Window, cx: &mut App) {
    let token = request.token;
    let builder = request.builder;
    window.open_dialog(cx, move |dialog, window, cx| {
        builder(dialog, token, window, cx)
    });
}

fn queue_request(
    window: AnyWindowHandle,
    request: PendingModal,
    replace_active: bool,
) -> (DialogOpenResult, Option<PendingModal>, bool) {
    WINDOW_MODALS.with(|registry| {
        let mut registry = registry.borrow_mut();
        let index = registry
            .iter()
            .position(|state| state.window == window)
            .unwrap_or_else(|| {
                registry.push(WindowModalState {
                    window,
                    active: None,
                    pending: None,
                });
                registry.len() - 1
            });
        let state = &mut registry[index];

        if !replace_active
            && state
                .active
                .is_some_and(|active| active.kind == request.kind)
        {
            return (DialogOpenResult::Ignored, None, false);
        }

        state.pending = Some(request);

        if replace_active && state.active.take().is_some() {
            return (DialogOpenResult::Queued, None, true);
        }

        if state.active.is_some() {
            return (DialogOpenResult::Queued, None, false);
        }

        let request = state
            .pending
            .take()
            .expect("modal request must exist before activation");
        state.active = Some(ActiveModal {
            kind: request.kind,
            token: request.token,
        });
        (DialogOpenResult::Opened, Some(request), false)
    })
}

fn close_active(window: AnyWindowHandle, token: DialogToken) -> (bool, Option<PendingModal>) {
    WINDOW_MODALS.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Some(index) = registry.iter().position(|state| state.window == window) else {
            return (false, None);
        };
        let state = &mut registry[index];
        if !state.active.is_some_and(|active| active.token == token) {
            return (false, None);
        }

        state.active = None;
        let next = state.pending.take().map(|request| {
            state.active = Some(ActiveModal {
                kind: request.kind,
                token: request.token,
            });
            request
        });

        if state.active.is_none() && state.pending.is_none() {
            registry.remove(index);
        }
        (true, next)
    })
}

fn activate_pending(window: AnyWindowHandle) -> Option<PendingModal> {
    WINDOW_MODALS.with(|registry| {
        let mut registry = registry.borrow_mut();
        let state = registry.iter_mut().find(|state| state.window == window)?;
        if state.active.is_some() {
            return None;
        }
        let request = state.pending.take()?;
        state.active = Some(ActiveModal {
            kind: request.kind,
            token: request.token,
        });
        Some(request)
    })
}

impl TinyShell {
    fn record_modal_token(&mut self, kind: DialogKind, token: DialogToken) {
        match kind {
            // Managed-key modals record their token explicitly in managed_key_dialogs.rs. Keeping
            // that ownership local prevents unrelated SFTP dialogs that historically reuse these
            // enum values from mutating managed-key state.
            DialogKind::SessionSelector => self.selector_dialog_token = Some(token),
            DialogKind::ConnectionGroup => self.connection_group_dialog_token = Some(token),
            DialogKind::VerifySyncSecretsPassword => {
                if let Some(state) = self.sync_runtime.secrets_password_dialog.as_mut() {
                    state.token = token;
                }
            }
            _ => {}
        }
    }

    fn clear_recorded_modal_token(&mut self, token: DialogToken) {
        if self.selector_dialog_token == Some(token) {
            self.selector_dialog_token = None;
        }
        if self.connection_group_dialog_token == Some(token) {
            self.connection_group_dialog_token = None;
        }
        if self.managed_key_dialog_token == Some(token) {
            self.managed_key_dialog_token = None;
        }
    }

    fn activate_modal_request(
        &mut self,
        request: PendingModal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.record_modal_token(request.kind, request.token);
        open_request(request, window, cx);
    }

    /// Open a modal owned by the exact native window that initiated the action.
    ///
    /// Each native window has an independent active + latest-pending slot, so a modal opened from
    /// Settings, Connection Manager or SSH Editor can never be rendered by another TinyShell
    /// window. Requests on different windows may be active concurrently.
    pub(crate) fn open_modal_dialog<F>(
        &mut self,
        kind: DialogKind,
        window: &mut Window,
        cx: &mut Context<Self>,
        builder: F,
    ) -> DialogOpenResult
    where
        F: Fn(Dialog, DialogToken, &mut Window, &mut App) -> Dialog + 'static,
    {
        let request = PendingModal {
            kind,
            token: next_token(),
            builder: Box::new(builder),
        };
        let (result, request, _) = queue_request(window.window_handle(), request, false);
        if let Some(request) = request {
            self.activate_modal_request(request, window, cx);
        }
        result
    }

    /// Replace the current modal in this native window while preserving modal isolation for every
    /// other window. The replacement opens after GPUI has finished closing the current layer.
    pub(crate) fn replace_modal_dialog<F>(
        &mut self,
        kind: DialogKind,
        window: &mut Window,
        cx: &mut Context<Self>,
        builder: F,
    ) -> DialogOpenResult
    where
        F: Fn(Dialog, DialogToken, &mut Window, &mut App) -> Dialog + 'static,
    {
        let handle = window.window_handle();
        let request = PendingModal {
            kind,
            token: next_token(),
            builder: Box::new(builder),
        };
        let (result, request, should_close) = queue_request(handle, request, true);
        if let Some(request) = request {
            self.activate_modal_request(request, window, cx);
        } else if should_close {
            window.close_dialog(cx);
            if let Some(request) = activate_pending(handle) {
                self.record_modal_token(request.kind, request.token);
                window.defer(cx, move |window, cx| open_request(request, window, cx));
            }
        }
        result
    }

    /// Mark the modal identified by `token` as closed for this exact native window and schedule
    /// that window's pending modal, if any.
    pub(crate) fn modal_dialog_closed(
        &mut self,
        token: DialogToken,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let (closed, next) = close_active(window.window_handle(), token);
        if !closed {
            return false;
        }
        self.clear_recorded_modal_token(token);
        if let Some(request) = next {
            self.record_modal_token(request.kind, request.token);
            window.defer(cx, move |window, cx| open_request(request, window, cx));
        }
        true
    }

    /// Programmatically dismiss a modal without allowing a queued request from another native
    /// window to steal the dialog layer.
    pub(crate) fn dismiss_modal_dialog(
        &mut self,
        token: DialogToken,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.modal_dialog_closed(token, window, cx) {
            return false;
        }
        window.close_dialog(cx);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modal_tokens_are_process_unique() {
        let first = next_token();
        let second = next_token();
        assert_ne!(first, second);
    }
}
