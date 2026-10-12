use super::{
    ConfirmationDialog, Feedback, McpPanel, Snapshot, clients::code_block, heading, hint, mcp,
    report,
};
use gpui::{
    ClipboardItem, Context, InteractiveElement as _, IntoElement as _, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use rust_i18n::t;
use tiny_shell_mcp::{contracts::Call, engine::Review, tasks::State};

impl McpPanel {
    pub(super) fn render_tasks(
        &self,
        snapshot: &Snapshot,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let mut content = v_flex().w_full().min_w_0().gap_3();
        if snapshot.reviews.is_empty() {
            content = content.child(hint(t!("mcp_no_tasks").to_string(), cx));
        }
        for (category, key) in [
            (0, "mcp_tasks_pending"),
            (1, "mcp_tasks_active"),
            (2, "mcp_tasks_finished"),
        ] {
            let tasks = snapshot
                .reviews
                .iter()
                .filter(|r| task_category(r.state) == category)
                .collect::<Vec<_>>();
            if tasks.is_empty() {
                continue;
            }
            let mut list = v_flex()
                .id(format!("mcp-task-group-{category}"))
                .max_h(px(if category == 0 { 420. } else { 240. }))
                .overflow_y_scroll()
                .gap_2();
            for review in tasks {
                list = list.child(self.task_row(review, snapshot, cx));
            }
            content = content.child(heading(t!(key).to_string())).child(list);
        }
        let history_empty = snapshot.history.is_empty();
        content = content.child(
            h_flex()
                .flex_wrap()
                .justify_between()
                .gap_2()
                .border_t_1()
                .border_color(cx.theme().border)
                .pt_3()
                .child(
                    Button::new("mcp-history-toggle")
                        .small()
                        .ghost()
                        .label(t!("mcp_history_count", count = snapshot.history.len()).to_string())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_history = !this.show_history;
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("mcp-clear-history")
                        .small()
                        .ghost()
                        .disabled(history_empty || snapshot.policy.is_none())
                        .label(t!("mcp_clear_history").to_string())
                        .on_click(|_, window, cx| {
                            ConfirmationDialog::new(
                                t!("mcp_clear_history").to_string(),
                                t!("mcp_clear_history_confirm").to_string(),
                            )
                            .danger(true)
                            .on_ok(|_, window, cx| {
                                let result = mcp::service().and_then(|service| {
                                    service
                                        .engine
                                        .store
                                        .clear_audit()
                                        .map_err(|_| "STORAGE_ERROR".to_string())
                                });
                                let success = report(result, "mcp_history_cleared", window, cx);
                                cx.refresh_windows();
                                success
                            })
                            .open(window, cx);
                        }),
                ),
        );
        if self.show_history {
            content = content.child(hint(t!("mcp_history").to_string(), cx));
            let mut history = v_flex()
                .id("mcp-history-list")
                .max_h(px(220.))
                .overflow_y_scroll()
                .gap_2();
            if history_empty {
                history = history.child(hint(t!("mcp_no_history").to_string(), cx));
            }
            for record in snapshot.history.iter().rev() {
                let client_id = record["client_id"].as_str().unwrap_or("");
                let client = snapshot
                    .clients
                    .iter()
                    .find(|c| c.id == client_id)
                    .map(|c| c.name().to_owned())
                    .unwrap_or_else(|| client_id.chars().take(8).collect());
                let connection_id = record["connection_id"].as_str().unwrap_or("");
                let target = snapshot
                    .targets
                    .iter()
                    .find(|t| t.id == connection_id)
                    .map(|t| t.name.clone())
                    .unwrap_or_else(|| t!("mcp_unknown_connection").to_string());
                let time = record["timestamp"]
                    .as_i64()
                    .and_then(|v| chrono::DateTime::from_timestamp(v, 0))
                    .map(|time| {
                        time.with_timezone(&chrono::Local)
                            .format("%m-%d %H:%M:%S")
                            .to_string()
                    })
                    .unwrap_or_default();
                let state = record["state"]
                    .as_str()
                    .and_then(|s| {
                        serde_json::from_value::<State>(serde_json::Value::String(s.into())).ok()
                    })
                    .map(|s| t!(state_key(s)).to_string())
                    .unwrap_or_default();
                history = history.child(
                    v_flex()
                        .gap_1()
                        .py_2()
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .child(div().text_sm().child(format!("{client} · {target}")))
                        .child(hint(
                            format!(
                                "{time} · {} · {state}",
                                t!(operation_name(record["operation"].as_str().unwrap_or("")))
                            ),
                            cx,
                        )),
                );
            }
            content = content.child(history);
        }
        content.into_any_element()
    }

    fn task_row(&self, review: &Review, snapshot: &Snapshot, cx: &mut Context<Self>) -> gpui::Div {
        let awaiting = review.state == State::AwaitingUser;
        let expanded = awaiting || self.expanded_tasks.contains(&review.id);
        let client = snapshot
            .clients
            .iter()
            .find(|c| c.id == review.client)
            .map(|c| c.name().to_owned())
            .unwrap_or_else(|| review.client.chars().take(8).collect());
        let id = review.id.clone();
        let title = format!(
            "{} · {} · {}",
            client,
            review.target.name,
            t!(operation_name(call_name(&review.call)))
        );
        let mut row = v_flex()
            .w_full()
            .min_w_0()
            .gap_2()
            .p_3()
            .border_1()
            .border_color(cx.theme().border)
            .rounded(px(6.))
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .justify_between()
                    .child(heading(title))
                    .child(hint(t!(state_key(review.state)).to_string(), cx)),
            )
            .when(!awaiting, |row| {
                row.child(
                    Button::new(format!("mcp-task-expand-{id}"))
                        .small()
                        .ghost()
                        .label(
                            t!(if expanded {
                                "mcp_collapse"
                            } else {
                                "mcp_details"
                            })
                            .to_string(),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !this.expanded_tasks.remove(&id) {
                                this.expanded_tasks.insert(id.clone());
                            }
                            cx.notify();
                        })),
                )
            });
        if let Some(error) = &review.error {
            row = row.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().warning)
                    .child(format!("{} ({})", t!(error_key(&error.code)), error.code)),
            );
        }
        if expanded {
            row = row.child(hint(
                format!(
                    "{}@{}:{}",
                    review.target.user, review.target.host, review.target.port
                ),
                cx,
            ));
            match &review.call {
                Call::RunCommand(command) => {
                    let command_text = command.command.clone();
                    row = row.child(
                        Button::new(format!("mcp-copy-command-{}", review.id))
                            .small()
                            .ghost()
                            .label(t!("mcp_copy_command").to_string())
                            .on_click(move |_, window, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    command_text.clone(),
                                ));
                                Feedback::success(window, cx, t!("mcp_command_copied").to_string());
                            }),
                    );
                    if let Some(cwd) = &command.cwd {
                        row = row.child(hint(
                            t!("mcp_working_directory", path = cwd).to_string(),
                            cx,
                        ));
                    }
                    row = row.child(code_block(
                        format!("mcp-command-{}", review.id),
                        &command.command,
                        220.,
                        cx,
                    ));
                }
                Call::WriteFile(file) => {
                    row = row
                        .child(hint(file.path.clone(), cx))
                        .child(heading(t!("mcp_old_content").to_string()))
                        .child(code_block(
                            format!("mcp-old-{}", review.id),
                            review.preview["old_content"].as_str().unwrap_or(""),
                            160.,
                            cx,
                        ))
                        .child(heading(t!("mcp_new_content").to_string()))
                        .child(code_block(
                            format!("mcp-new-{}", review.id),
                            &file.content,
                            160.,
                            cx,
                        ));
                }
                _ => {}
            }
        }
        if awaiting {
            let id = review.id.clone();
            if review
                .error
                .as_ref()
                .is_some_and(|e| e.code == "AUTH_REQUIRED")
            {
                let owner = self.owner.clone();
                let connection = review.target.id.clone();
                row = row.child(Button::new(format!("mcp-auth-{id}")).small().label(t!("mcp_authenticate").to_string()).on_click(move |_, _, cx| {
                    if let Some(session) = owner.read(cx).config.get(&connection).cloned() {
                        crate::app::connection_manager::ssh_editor_window::open(owner.clone(), crate::app::connection_manager::ssh_editor_window::SshEditorRequest::McpCredentials { session, task_id: id.clone() }, cx);
                    } else { Feedback::error_for_owner(&owner, cx, t!("mcp_connection_missing").to_string()); }
                }));
            } else {
                let approve_id = id.clone();
                row = row.child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new(format!("mcp-approve-{id}"))
                                .small()
                                .label(t!("mcp_approve").to_string())
                                .on_click(move |_, window, cx| {
                                    let id = approve_id.clone();
                                    ConfirmationDialog::new(
                                        t!("mcp_approve").to_string(),
                                        t!("mcp_approve_confirm").to_string(),
                                    )
                                    .danger(true)
                                    .on_ok(move |_, window, cx| {
                                        let result = mcp::service().and_then(|service| {
                                            let _enter = service.runtime.enter();
                                            service.engine.approve(&id, true).map_err(|e| e.code)
                                        });
                                        let success =
                                            report(result, "mcp_approval_sent", window, cx);
                                        cx.refresh_windows();
                                        success
                                    })
                                    .open(window, cx);
                                }),
                        )
                        .child(
                            Button::new(format!("mcp-deny-{id}"))
                                .small()
                                .outline()
                                .label(t!("mcp_deny").to_string())
                                .on_click(move |_, window, cx| {
                                    let result = mcp::service().and_then(|service| {
                                        let _enter = service.runtime.enter();
                                        service.engine.approve(&id, false).map_err(|e| e.code)
                                    });
                                    report(result, "mcp_rejected", window, cx);
                                    cx.refresh_windows();
                                }),
                        ),
                );
            }
        }
        row
    }
}

fn task_category(state: State) -> usize {
    if state == State::AwaitingUser {
        0
    } else if state.terminal() {
        2
    } else {
        1
    }
}
fn state_key(state: State) -> &'static str {
    match state {
        State::Queued => "mcp_state_queued",
        State::AwaitingUser => "mcp_state_awaiting_user",
        State::Running => "mcp_state_running",
        State::Succeeded => "mcp_state_succeeded",
        State::Failed => "mcp_state_failed",
        State::CancelRequested => "mcp_state_cancel_requested",
        State::Cancelled => "mcp_state_cancelled",
        State::TimedOut => "mcp_state_timed_out",
        State::Interrupted => "mcp_state_interrupted",
        State::OutcomeUnknown => "mcp_state_outcome_unknown",
    }
}
fn call_name(call: &Call) -> &'static str {
    match call {
        Call::GetStatus(_) => "get_status",
        Call::ListConnections(_) => "list_connections",
        Call::OpenSession(_) => "open_session",
        Call::CloseSession(_) => "close_session",
        Call::RunCommand(_) => "run_command",
        Call::GetTask(_) => "get_task",
        Call::CancelTask(_) => "cancel_task",
        Call::ListDirectory(_) => "list_directory",
        Call::ReadFile(_) => "read_file",
        Call::WriteFile(_) => "write_file",
    }
}
fn operation_name(name: &str) -> &'static str {
    match name {
        "open_session" => "mcp_operation_open",
        "close_session" => "mcp_operation_close",
        "run_command" => "mcp_operation_command",
        "write_file" => "mcp_operation_write",
        "read_file" => "mcp_operation_read",
        "list_directory" => "mcp_operation_directory",
        "cancel_task" => "mcp_operation_cancel",
        _ => "mcp_operation_other",
    }
}
fn error_key(code: &str) -> &'static str {
    match code {
        "AUTH_REQUIRED" => "mcp_error_auth",
        "POLICY_CHANGED" | "TARGET_CHANGED" => "mcp_error_policy",
        "FILE_CONFLICT" => "mcp_error_conflict",
        "PERMISSION_DENIED" | "NOT_AUTHORIZED" => "mcp_error_permission",
        "CLIENT_REVOKED" => "mcp_error_revoked",
        "OUTCOME_UNKNOWN" => "mcp_error_unknown",
        _ => "mcp_error_generic",
    }
}
