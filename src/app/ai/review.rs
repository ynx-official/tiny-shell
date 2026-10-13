//! Connect chat review controls to the existing MCP approval and credential flows.
use super::tool_steps::review::{LoadReview, ReviewActions, ReviewScope, same_operation};
use crate::{
    TinyShell,
    ai::Conversation,
    app::{
        connection_manager::ssh_editor_window::{self, SshEditorRequest},
        mcp,
    },
    feedback::Feedback,
};
use gpui::WeakEntity;
use rust_i18n::t;
use std::rc::Rc;
use tiny_shell_mcp::contracts::Call;

pub(super) fn actions(conversation: &Conversation, owner: WeakEntity<TinyShell>) -> ReviewActions {
    let scope = conversation
        .target
        .as_ref()
        .map(|target| ReviewScope::new(&conversation.id, &target.id, &target.digest));
    let load: LoadReview = Rc::new(move |task| {
        let review = mcp::service()?.engine.review(task).map_err(|e| e.code)?;
        if !scope
            .as_ref()
            .is_some_and(|scope| scope.matches(task, &review))
        {
            return Err("TASK_NOT_PENDING".into());
        }
        Ok(review)
    });
    let reload = load.clone();
    let decide = Rc::new(
        move |displayed: &tiny_shell_mcp::engine::Review,
              approved,
              _: &mut gpui::Window,
              _: &mut gpui::App| {
            let current = reload(&displayed.id)?;
            if !same_operation(displayed, &current) {
                return Err("APPROVAL_CHANGED".into());
            }
            // Credential completion is separate; never approve an unseen file write.
            if approved
                && (current.error.is_some()
                    || !match &current.call {
                        Call::RunCommand(_) => true,
                        Call::WriteFile(_) => current.preview["old_content"].is_string(),
                        _ => false,
                    })
            {
                return Err("PREVIEW_UNAVAILABLE".into());
            }
            let service = mcp::service()?;
            let _enter = service.runtime.enter();
            service
                .engine
                .approve(&current.id, approved)
                .map_err(|e| e.code)
        },
    );
    let reload = load.clone();
    let authenticate = Rc::new(
        move |displayed: &tiny_shell_mcp::engine::Review,
              window: &mut gpui::Window,
              cx: &mut gpui::App| {
            let result = reload(&displayed.id).and_then(|current| {
                if !same_operation(displayed, &current)
                    || !current
                        .error
                        .as_ref()
                        .is_some_and(|e| e.code == "AUTH_REQUIRED")
                {
                    return Err("APPROVAL_CHANGED".into());
                }
                let owner = owner.upgrade().ok_or("TASK_NOT_PENDING")?;
                let session = owner
                    .read(cx)
                    .config
                    .get(&current.target.id)
                    .cloned()
                    .ok_or("CONNECTION_NOT_ALLOWED")?;
                Ok((owner, session, current.id))
            });
            match result {
                Ok((owner, session, task_id)) => {
                    ssh_editor_window::open(
                        owner,
                        SshEditorRequest::McpCredentials { session, task_id },
                        cx,
                    );
                }
                Err(code) => {
                    Feedback::error(window, cx, t!("mcp_action_failed", code = code).to_string())
                }
            }
        },
    );
    ReviewActions {
        load,
        decide,
        authenticate,
    }
}
