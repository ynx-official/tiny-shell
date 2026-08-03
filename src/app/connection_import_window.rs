use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, InteractiveElement as _, IntoElement,
    ParentElement as _, PathPromptOptions, Render, Styled, Window, prelude::FluentBuilder as _,
};
use gpui_component::{
    ActiveTheme as _, Root,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use rust_i18n::t;

use crate::{
    TinyShell,
    session::connection_import::{
        FinalShellImportError, FinalShellImportErrorKind, FinalShellImportPreview,
        apply_finalshell_import, parse_finalshell_zip,
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
    error: Option<String>,
    focus_handle: FocusHandle,
    _owner_subscription: gpui::Subscription,
}

impl FinalShellImportWindow {
    fn new(owner: Entity<TinyShell>, cx: &mut Context<Self>) -> Self {
        let owner_subscription = cx.observe(&owner, |_, _, cx| cx.notify());
        Self {
            owner,
            preview: None,
            error: None,
            focus_handle: cx.focus_handle(),
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
                        this.update(cx, |this, cx| {
                            match parsed {
                                Ok(preview) => {
                                    this.preview = Some(preview);
                                    this.error = None;
                                }
                                Err(error) => {
                                    tracing::warn!(error = ?error, "failed to parse FinalShell backup");
                                    this.preview = None;
                                    this.error = Some(localized_import_error(&error));
                                }
                            }
                            cx.notify();
                        })?;
                        let _ = window_handle.update(cx, |_, window, _| window.activate_window());
                    }
                }
                Ok(Err(error)) => {
                    this.update(cx, |this, cx| {
                        this.error = Some(
                            t!("finalshell_import_picker_failed", error = error.to_string())
                                .to_string(),
                        );
                        cx.notify();
                    })?;
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
        let result = self.owner.update(cx, |owner, cx| {
            let mut staged = owner.config.clone();
            let summary = apply_finalshell_import(&mut staged, preview);
            crate::app::config_persistence::save_full(&staged)?;
            owner.config = staged;
            owner.status = t!(
                "finalshell_imported",
                count = summary.imported_sessions,
                skipped = summary.skipped_sessions
            )
            .to_string()
            .into();
            cx.notify();
            Ok::<(), anyhow::Error>(())
        });
        match result {
            Ok(()) => window.remove_window(),
            Err(error) => {
                self.error = Some(error.to_string());
                cx.notify();
            }
        }
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
            v_flex()
                .size_full()
                .gap_3()
                .child(t!("finalshell_import_preview").to_string())
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
                        .when(preview.skipped_entries > 0, |this| {
                            this.child(
                                t!("finalshell_import_skipped", count = preview.skipped_entries)
                                    .to_string(),
                            )
                        }),
                )
                .child(
                    h_flex()
                        .mt_auto()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("finalshell-import-cancel")
                                .secondary()
                                .label(t!("cancel").to_string())
                                .on_click(|_, window, _| window.remove_window()),
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
                            .on_click(|_, window, _| window.remove_window()),
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
    let mut options = super::connection_manager::window::window_options(cx);
    options.window_min_size = Some(gpui::size(gpui::px(420.), gpui::px(300.)));
    let opened = cx.open_window(options, move |window, cx| {
        window.set_window_title(t!("finalshell_import_title").as_ref());
        let view = cx.new(|cx| FinalShellImportWindow::new(owner, cx));
        let focus_handle = view.read(cx).focus_handle.clone();
        window.defer(cx, move |window, cx| {
            window.activate_window();
            window.focus(&focus_handle, cx);
        });
        cx.new(|cx| Root::new(view, window, cx))
    });
    if let Err(error) = opened {
        tracing::error!("failed to open FinalShell import window: {error:#}");
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
