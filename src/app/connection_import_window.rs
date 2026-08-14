use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, InteractiveElement as _, IntoElement,
    ParentElement as _, PathPromptOptions, Render, ScrollHandle, StatefulInteractiveElement as _,
    Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Root,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex,
    scroll::{Scrollbar, ScrollbarAxis, ScrollbarShow},
    v_flex,
};
use rust_i18n::t;

use crate::{
    TinyShell,
    session::connection_import::{
        FinalShellImportError, FinalShellImportErrorKind, FinalShellImportPreview,
        apply_finalshell_import_selected, import_matches_existing, parse_finalshell_zip,
    },
};

fn localized_import_error(error: &FinalShellImportError) -> String {
    match error.kind() {
        FinalShellImportErrorKind::Open => t!("finalshell_import_error_open").to_string(),
        FinalShellImportErrorKind::InvalidArchive => {
            t!("finalshell_import_error_invalid_archive").to_string()
        }
        FinalShellImportErrorKind::TooManyEntries => {
            t!("finalshell_import_error_too_many_entries").to_string()
        }
        FinalShellImportErrorKind::TooLarge => t!("finalshell_import_error_too_large").to_string(),
        FinalShellImportErrorKind::ReadEntry => {
            t!("finalshell_import_error_read_entry").to_string()
        }
        FinalShellImportErrorKind::InvalidFilenameEncoding => {
            t!("finalshell_import_error_filename_encoding").to_string()
        }
        FinalShellImportErrorKind::UnsafePath => {
            t!("finalshell_import_error_unsafe_path").to_string()
        }
        FinalShellImportErrorKind::ReadConnection => {
            t!("finalshell_import_error_read_connection").to_string()
        }
        FinalShellImportErrorKind::NoConnections => {
            t!("finalshell_import_error_no_connections").to_string()
        }
    }
}

pub(crate) struct FinalShellImportWindow {
    owner: Entity<TinyShell>,
    preview: Option<FinalShellImportPreview>,
    selected: Vec<bool>,
    error: Option<String>,
    focus_handle: FocusHandle,
    scroll_handle: ScrollHandle,
    _owner_subscription: gpui::Subscription,
}

impl FinalShellImportWindow {
    fn new(owner: Entity<TinyShell>, cx: &mut Context<Self>) -> Self {
        let owner_subscription = cx.observe(&owner, |_, _, cx| cx.notify());
        Self {
            owner,
            preview: None,
            selected: Vec::new(),
            error: None,
            focus_handle: cx.focus_handle(),
            scroll_handle: ScrollHandle::new(),
            _owner_subscription: owner_subscription,
        }
    }

    fn choose_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(t!("finalshell_import_select_file").to_string().into()),
        });
        let window_handle = window.window_handle();
        cx.spawn_in(window, async move |this, cx| {
            match prompt.await {
                Ok(Ok(Some(mut paths))) => {
                    if let Some(path) = paths.pop() {
                        let parsed = cx
                            .background_executor()
                            .spawn(async move { parse_finalshell_zip(&path) })
                            .await;
                        let feedback = this.update(cx, |this, cx| {
                            let feedback = match parsed {
                                Ok(preview) => {
                                    let connection_count = preview.sessions.len();
                                    let skipped = preview.skipped_entries;
                                    this.selected = vec![true; connection_count];
                                    this.preview = Some(preview);
                                    this.error = None;
                                    Ok((connection_count, skipped))
                                }
                                Err(error) => {
                                    tracing::warn!(error = ?error, "failed to parse FinalShell backup");
                                    let message = localized_import_error(&error);
                                    this.preview = None;
                                    this.error = Some(message.clone());
                                    Err(message)
                                }
                            };
                            cx.notify();
                            feedback
                        })?;
                        let _ = window_handle.update(cx, |_, window, cx| {
                            window.activate_window();
                            match feedback {
                                Ok((count, skipped)) => {
                                    crate::feedback::Feedback::info(
                                        window,
                                        cx,
                                        t!("finalshell_import_connections", count = count).to_string(),
                                    );
                                    if skipped > 0 {
                                        crate::feedback::Feedback::warning(
                                            window,
                                            cx,
                                            t!("finalshell_import_skipped", count = skipped).to_string(),
                                        );
                                    }
                                }
                                Err(message) => {
                                    crate::feedback::Feedback::error(window, cx, message);
                                }
                            }
                        });
                    }
                }
                Ok(Err(error)) => {
                    let message =
                        t!("finalshell_import_picker_failed", error = error.to_string()).to_string();
                    this.update(cx, |this, cx| {
                        this.error = Some(message.clone());
                        cx.notify();
                    })?;
                    let _ = window_handle.update(cx, |_, window, cx| {
                        crate::feedback::Feedback::error(window, cx, message);
                    });
                }
                _ => {}
            }
            Ok::<(), anyhow::Error>(())
        })
        .detach();
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(preview) = self.preview.clone() else {
            return;
        };
        let selected = self.selected.clone();
        let import_window = cx.entity();
        let owner_for_credentials = self.owner.clone();
        let owner_for_feedback = self.owner.clone();
        self.owner.update(cx, move |owner, cx| {
            let mut staged = owner.config.clone();
            let summary = apply_finalshell_import_selected(&mut staged, preview.clone(), &selected);
            let pending_credentials = preview
                .sessions
                .iter()
                .enumerate()
                .find(|(index, _)| selected.get(*index).copied().unwrap_or(false))
                .and_then(|(_, imported)| {
                    staged
                        .sessions()
                        .iter()
                        .find(|session| import_matches_existing(session, imported))
                        .filter(|session| session.requires_credential_prompt())
                        .map(|session| session.id.clone())
                });
            owner.commit_staged_config_in_window_async(
                staged,
                window,
                move |owner, window, cx| {
                    let message = t!(
                        "finalshell_imported",
                        count = summary.imported_sessions,
                        skipped = summary.skipped_sessions
                    )
                    .to_string();
                    owner.status = message.clone().into();
                    cx.notify();
                    let owner_for_feedback = owner_for_feedback.clone();
                    let owner_for_credentials = owner_for_credentials.clone();
                    window.defer(cx, move |window, cx| {
                        crate::feedback::Feedback::show_for_owner(
                            &owner_for_feedback,
                            cx,
                            crate::feedback::FeedbackKind::Success,
                            message,
                        );
                        if let Some(session_id) = pending_credentials {
                            if let Some(session) = owner_for_credentials
                                .read(cx)
                                .config
                                .get(&session_id)
                                .cloned()
                            {
                                crate::app::connection_manager::ssh_editor_window::open(
                                    owner_for_credentials,
                                    crate::app::connection_manager::ssh_editor_window::SshEditorRequest::Credentials {
                                        session,
                                    },
                                    cx,
                                );
                            } else {
                                window.activate_window();
                            }
                        }
                        crate::app::deregister_auxiliary_window(window.window_handle());
                        window.remove_window();
                    });
                },
                move |_, error, window, cx| {
                    let message = error.to_string();
                    import_window.update(cx, |this, cx| {
                        this.error = Some(message.clone());
                        cx.notify();
                    });
                    crate::feedback::Feedback::error(window, cx, message);
                },
                cx,
            );
        });
    }
}

impl Render for FinalShellImportWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = if let Some(preview) = &self.preview {
            let password_count = preview
                .sessions
                .iter()
                .filter(|session| session.auth == crate::session::config::AuthMethod::Password)
                .count();
            let key_count = preview.sessions.len().saturating_sub(password_count);
            let all_selected = !self.selected.is_empty() && self.selected.iter().all(|item| *item);
            let selected_count = self.selected.iter().filter(|item| **item).count();
            let sessions = preview.sessions.clone();
            let selected = self.selected.clone();
            let existing = self.owner.read(cx).config.clone();
            let rows = v_flex()
                .id("finalshell-import-list")
                .relative()
                .track_scroll(&self.scroll_handle)
                .overflow_y_scroll()
                .flex_1()
                .min_h(px(0.))
                .border_1()
                .border_color(cx.theme().border)
                .children(sessions.into_iter().enumerate().map(|(index, session)| {
                    let checked = selected.get(index).copied().unwrap_or(false);
                    let is_existing = existing
                        .sessions()
                        .iter()
                        .any(|current| import_matches_existing(current, &session));
                    let auth = match session.auth {
                        crate::session::config::AuthMethod::Password => {
                            t!("finalshell_import_auth_password").to_string()
                        }
                        _ => t!("finalshell_import_auth_key").to_string(),
                    };
                    let status = if is_existing {
                        t!("finalshell_import_existing").to_string()
                    } else {
                        t!("finalshell_import_new").to_string()
                    };
                    h_flex()
                        .id(("finalshell-import-row", index))
                        .flex_none()
                        .min_h(px(38.))
                        .px_2()
                        .gap_2()
                        .items_center()
                        .border_b_1()
                        .border_color(cx.theme().border.opacity(0.5))
                        .child(
                            Checkbox::new(("finalshell-import-check", index))
                                .checked(checked)
                                .on_click(cx.listener(move |this, value: &bool, _, cx| {
                                    if let Some(item) = this.selected.get_mut(index) {
                                        *item = *value;
                                    }
                                    cx.notify();
                                })),
                        )
                        .child(div().flex_1().min_w(px(0.)).child(format!(
                            "{}  ({}:{})",
                            session.name, session.host, session.port
                        )))
                        .child(div().w(px(130.)).child(session.group.unwrap_or_default()))
                        .child(div().w(px(90.)).child(auth))
                        .child(
                            div()
                                .w(px(70.))
                                .text_color(if is_existing {
                                    cx.theme().warning
                                } else {
                                    cx.theme().success
                                })
                                .child(status),
                        )
                        .into_any_element()
                }))
                .child(
                    div().absolute().top_0().bottom_0().right_0().child(
                        Scrollbar::new(&self.scroll_handle)
                            .id("finalshell-import-scrollbar")
                            .axis(ScrollbarAxis::Vertical)
                            .scrollbar_show(ScrollbarShow::Scrolling),
                    ),
                );
            v_flex()
                .size_full()
                .gap_3()
                .child(
                    h_flex()
                        .items_center()
                        .child(t!("finalshell_import_preview").to_string())
                        .child(div().flex_1())
                        .child(
                            Checkbox::new("finalshell-import-select-all")
                                .checked(all_selected)
                                .label(t!("finalshell_import_select_all").to_string())
                                .on_click(cx.listener(|this, value: &bool, _, cx| {
                                    for item in &mut this.selected {
                                        *item = *value;
                                    }
                                    cx.notify();
                                })),
                        ),
                )
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            t!(
                                "finalshell_import_connections",
                                count = preview.sessions.len()
                            )
                            .to_string(),
                        )
                        .child(
                            t!("finalshell_import_groups", count = preview.groups.len())
                                .to_string(),
                        )
                        .child(
                            t!("finalshell_import_password_auth", count = password_count)
                                .to_string(),
                        )
                        .child(t!("finalshell_import_key_auth", count = key_count).to_string())
                        .child(t!("finalshell_import_credentials_omitted").to_string())
                        .child(t!("finalshell_import_selected", count = selected_count).to_string())
                        .when(preview.skipped_entries > 0, |this| {
                            this.child(
                                t!("finalshell_import_skipped", count = preview.skipped_entries)
                                    .to_string(),
                            )
                        }),
                )
                .child(
                    h_flex()
                        .flex_none()
                        .h(px(30.))
                        .px_2()
                        .gap_2()
                        .items_center()
                        .text_color(cx.theme().muted_foreground)
                        .child(div().flex_1().child(t!("session").to_string()))
                        .child(
                            div()
                                .w(px(130.))
                                .child(t!("finalshell_import_folder").to_string()),
                        )
                        .child(
                            div()
                                .w(px(90.))
                                .child(t!("ssh_editor_auth_method").to_string()),
                        )
                        .child(
                            div()
                                .w(px(70.))
                                .child(t!("finalshell_import_state").to_string()),
                        ),
                )
                .child(rows)
                .child(
                    h_flex()
                        .mt_auto()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("finalshell-import-cancel")
                                .secondary()
                                .label(t!("cancel").to_string())
                                .on_click(|_, window, _| {
                                    crate::app::deregister_auxiliary_window(window.window_handle());
                                    window.remove_window();
                                }),
                        )
                        .child(
                            Button::new("finalshell-import-confirm")
                                .primary()
                                .label(t!("confirm").to_string())
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.confirm(window, cx)),
                                ),
                        ),
                )
                .into_any_element()
        } else {
            let body = v_flex()
                .size_full()
                .gap_3()
                .child(t!("finalshell_import_description").to_string())
                .child(
                    Button::new("finalshell-import-select")
                        .primary()
                        .label(t!("finalshell_import_select_file").to_string())
                        .on_click(cx.listener(|this, _, window, cx| this.choose_file(window, cx))),
                )
                .child(
                    h_flex().mt_auto().justify_end().child(
                        Button::new("finalshell-import-cancel")
                            .secondary()
                            .label(t!("cancel").to_string())
                            .on_click(|_, window, _| {
                                crate::app::deregister_auxiliary_window(window.window_handle());
                                window.remove_window();
                            }),
                    ),
                );
            body.into_any_element()
        };

        v_flex()
            .size_full()
            .track_focus(&self.focus_handle)
            .bg(cx.theme().background)
            .p_4()
            .child(content)
            .when_some(self.error.clone(), |this, error| {
                this.child(
                    gpui::div()
                        .flex_none()
                        .text_color(cx.theme().danger)
                        .child(error),
                )
            })
    }
}

pub(crate) fn open(owner: Entity<TinyShell>, cx: &mut App) {
    let owner_id = owner.read(cx).session_owner_id;
    let options = crate::app::platform::auxiliary_window_options(
        cx,
        crate::app::platform::AuxiliaryWindowSpec::new(gpui::size(gpui::px(600.), gpui::px(400.)))
            .with_min_size(gpui::size(gpui::px(420.), gpui::px(300.)))
            .with_max_ratio(0.72, 0.62),
    );
    let owner_for_window = owner.clone();
    let opened = cx.open_window(options, move |window, cx| {
        window.set_window_title(t!("finalshell_import_title").as_ref());
        let window_handle = window.window_handle();
        crate::app::register_auxiliary_window(window_handle, owner_id);
        let view = cx.new(|cx| FinalShellImportWindow::new(owner_for_window, cx));
        let focus_handle = view.read(cx).focus_handle.clone();
        window.defer(cx, move |window, cx| {
            window.activate_window();
            window.focus(&focus_handle, cx);
        });
        window.on_window_should_close(cx, move |_, _| {
            crate::app::deregister_auxiliary_window(window_handle);
            true
        });
        cx.new(|cx| Root::new(view, window, cx))
    });
    if let Err(error) = opened {
        tracing::error!("failed to open FinalShell import window: {error:#}");
        crate::feedback::Feedback::show_for_owner(
            &owner,
            cx,
            crate::feedback::FeedbackKind::Error,
            t!(
                "finalshell_import_picker_failed",
                error = format!("{error:#}")
            )
            .to_string(),
        );
    }
}

impl TinyShell {
    pub(crate) fn open_finalshell_import_window(
        &mut self,
        window: &mut Window,
        cx: &mut Context<TinyShell>,
    ) {
        let owner = cx.entity();
        window.defer(cx, move |_, cx| open(owner, cx));
    }
}
