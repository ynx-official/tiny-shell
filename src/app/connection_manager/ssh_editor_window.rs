use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled,
    Window, WindowOptions, prelude::FluentBuilder as _, px, rems, size,
};
use gpui_component::{
    ActiveTheme as _, Root,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
    v_flex,
};
use rust_i18n::t;

use crate::{
    TinyShell,
    session::{
        config::{AuthMethod, Session},
        ssh_config::SshConfigEntry,
    },
};

#[derive(Clone)]
pub(crate) enum SshEditorRequest {
    New {
        group: Option<String>,
        prefill: Option<Session>,
    },
    Edit {
        session: Session,
    },
    Credentials {
        session: Session,
    },
    Clone {
        session: Session,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SshEditorPage {
    Connection,
    Proxy,
}

struct SshEditorInputs {
    name: Entity<InputState>,
    host: Entity<InputState>,
    port: Entity<InputState>,
    user: Entity<InputState>,
    password: Entity<InputState>,
    proxy_host: Entity<InputState>,
    proxy_port: Entity<InputState>,
    proxy_user: Entity<InputState>,
    proxy_password: Entity<InputState>,
}

pub(crate) struct SshEditorWindow {
    owner: Entity<TinyShell>,
    editing_id: Option<String>,
    baseline: Option<Session>,
    page: SshEditorPage,
    auth: AuthMethod,
    group: Option<String>,
    proxy_type: String,
    managed_key_id: Option<String>,
    ssh_config_entries: Vec<SshConfigEntry>,
    ssh_config_selected: Option<usize>,
    config_key_path: String,
    connect_after_save: bool,
    inputs: SshEditorInputs,
    error: Option<SharedString>,
    focus_handle: FocusHandle,
    _owner_subscription: gpui::Subscription,
    _input_subscriptions: Vec<gpui::Subscription>,
}

fn new_input(
    window: &mut Window,
    placeholder: String,
    value: String,
    masked: bool,
    cx: &mut Context<SshEditorWindow>,
) -> Entity<InputState> {
    cx.new(|cx| {
        let state = InputState::new(window, cx)
            .placeholder(placeholder)
            .default_value(value);
        if masked { state.masked(true) } else { state }
    })
}

impl SshEditorWindow {
    fn new(
        owner: Entity<TinyShell>,
        request: SshEditorRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let connect_after_save = matches!(&request, SshEditorRequest::Credentials { .. });
        let (editing_id, baseline, session, group) = match request {
            SshEditorRequest::New { group, prefill } => (None, None, prefill, group),
            SshEditorRequest::Edit { session } => (
                Some(session.id.clone()),
                Some(session.clone()),
                Some(session.clone()),
                session.group.clone(),
            ),
            SshEditorRequest::Credentials { session } => (
                Some(session.id.clone()),
                Some(session.clone()),
                Some(session.clone()),
                session.group.clone(),
            ),
            SshEditorRequest::Clone { mut session } => {
                session.name = format!("{}-copy", session.name);
                (None, None, Some(session.clone()), session.group.clone())
            }
        };
        let auth = session
            .as_ref()
            .map_or(AuthMethod::Password, |item| match item.auth {
                AuthMethod::Password => AuthMethod::Password,
                AuthMethod::Config => AuthMethod::Config,
                AuthMethod::Key | AuthMethod::KeyPending => AuthMethod::Key,
            });
        let proxy_type = session
            .as_ref()
            .map(|item| item.proxy_type.clone())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "none".to_string());
        let managed_key_id = session
            .as_ref()
            .and_then(|item| item.managed_key_id.clone());
        let config_key_path = session
            .as_ref()
            .map(|item| item.private_key_path.clone())
            .unwrap_or_default();
        let ssh_config_entries =
            crate::session::ssh_config::parse_ssh_config().unwrap_or_else(|error| {
                tracing::warn!(%error, "failed to read OpenSSH config for connection editor");
                Vec::new()
            });
        let ssh_config_selected = session.as_ref().and_then(|session| {
            (auth == AuthMethod::Config)
                .then(|| {
                    ssh_config_entries.iter().position(|entry| {
                        entry.hostname == session.host
                            && entry.port == session.port
                            && (entry.user.is_empty() || entry.user == session.user)
                    })
                })
                .flatten()
        });

        let inputs = SshEditorInputs {
            name: new_input(
                window,
                t!("name").to_string(),
                session
                    .as_ref()
                    .map(|item| item.name.clone())
                    .unwrap_or_default(),
                false,
                cx,
            ),
            host: new_input(
                window,
                t!("host").to_string(),
                session
                    .as_ref()
                    .map(|item| item.host.clone())
                    .unwrap_or_default(),
                false,
                cx,
            ),
            port: new_input(
                window,
                t!("port").to_string(),
                session
                    .as_ref()
                    .map_or_else(|| "22".to_string(), |item| item.port.to_string()),
                false,
                cx,
            ),
            user: new_input(
                window,
                t!("user").to_string(),
                session
                    .as_ref()
                    .map_or_else(|| "root".to_string(), |item| item.user.clone()),
                false,
                cx,
            ),
            password: new_input(
                window,
                t!("password").to_string(),
                session
                    .as_ref()
                    .map(|item| item.password.clone())
                    .unwrap_or_default(),
                true,
                cx,
            ),
            proxy_host: new_input(
                window,
                t!("proxy_host").to_string(),
                session
                    .as_ref()
                    .map(|item| item.proxy_host.clone())
                    .unwrap_or_default(),
                false,
                cx,
            ),
            proxy_port: new_input(
                window,
                t!("proxy_port").to_string(),
                session
                    .as_ref()
                    .and_then(|item| item.proxy_port)
                    .map(|port| port.to_string())
                    .unwrap_or_default(),
                false,
                cx,
            ),
            proxy_user: new_input(
                window,
                t!("proxy_user").to_string(),
                session
                    .as_ref()
                    .map(|item| item.proxy_user.clone())
                    .unwrap_or_default(),
                false,
                cx,
            ),
            proxy_password: new_input(
                window,
                t!("proxy_password").to_string(),
                session
                    .as_ref()
                    .map(|item| item.proxy_password.clone())
                    .unwrap_or_default(),
                true,
                cx,
            ),
        };
        let input_subscriptions = [
            &inputs.name,
            &inputs.host,
            &inputs.port,
            &inputs.user,
            &inputs.password,
            &inputs.proxy_host,
            &inputs.proxy_port,
            &inputs.proxy_user,
            &inputs.proxy_password,
        ]
        .into_iter()
        .map(|input| cx.subscribe_in(input, window, |_, _, _: &InputEvent, _, cx| cx.notify()))
        .collect();
        let owner_subscription = cx.observe(&owner, |_, _, cx| cx.notify());

        Self {
            owner,
            editing_id,
            baseline,
            page: SshEditorPage::Connection,
            auth,
            group,
            proxy_type,
            managed_key_id,
            ssh_config_entries,
            ssh_config_selected,
            config_key_path,
            connect_after_save,
            inputs,
            error: None,
            focus_handle: cx.focus_handle(),
            _owner_subscription: owner_subscription,
            _input_subscriptions: input_subscriptions,
        }
    }

    fn input_value(input: &Entity<InputState>, cx: &Context<Self>) -> String {
        input.read(cx).value().to_string()
    }

    fn build_session(&self, cx: &Context<Self>) -> anyhow::Result<Session> {
        let host = Self::input_value(&self.inputs.host, cx).trim().to_string();
        let user = Self::input_value(&self.inputs.user, cx).trim().to_string();
        if host.is_empty() || user.is_empty() {
            anyhow::bail!(t!("host_and_user_required").to_string());
        }
        let port = Self::input_value(&self.inputs.port, cx)
            .trim()
            .parse::<u16>()
            .map_err(|_| anyhow::anyhow!(t!("ssh_editor_invalid_port").to_string()))?;
        let proxy_port = if self.proxy_type == "none" {
            None
        } else {
            let proxy_host = Self::input_value(&self.inputs.proxy_host, cx)
                .trim()
                .to_string();
            let proxy_port = Self::input_value(&self.inputs.proxy_port, cx)
                .trim()
                .parse::<u16>()
                .map_err(|_| anyhow::anyhow!(t!("ssh_editor_proxy_required").to_string()))?;
            if proxy_host.is_empty() {
                anyhow::bail!(t!("ssh_editor_proxy_required").to_string());
            }
            Some(proxy_port)
        };
        let managed_key_available = self.managed_key_id.as_ref().is_some_and(|selected| {
            self.owner
                .read(cx)
                .config
                .managed_keys()
                .iter()
                .any(|key| &key.id == selected)
        });
        if matches!(self.auth, AuthMethod::Key | AuthMethod::KeyPending) && !managed_key_available {
            anyhow::bail!(t!("select_managed_key_hint").to_string());
        }

        let password = Self::input_value(&self.inputs.password, cx);
        let mut session = match self.auth {
            AuthMethod::Password => Session::password(host.clone(), port, user.clone(), password),
            AuthMethod::Key | AuthMethod::KeyPending => {
                let mut session = Session::key(
                    host.clone(),
                    port,
                    user.clone(),
                    String::new(),
                    String::new(),
                    String::new(),
                );
                session.managed_key_id = self.managed_key_id.clone();
                session
            }
            AuthMethod::Config => {
                let mut session = Session::key(
                    host.clone(),
                    port,
                    user.clone(),
                    self.config_key_path.clone(),
                    String::new(),
                    String::new(),
                );
                session.auth = AuthMethod::Config;
                session
            }
        };
        let name = Self::input_value(&self.inputs.name, cx).trim().to_string();
        session.name = if name.is_empty() { host } else { name };
        if let Some(id) = &self.editing_id {
            session.id = id.clone();
        }
        session.group = self.group.clone();
        session.proxy_type = self.proxy_type.clone();
        session.proxy_host = Self::input_value(&self.inputs.proxy_host, cx)
            .trim()
            .to_string();
        session.proxy_port = proxy_port;
        session.proxy_user = Self::input_value(&self.inputs.proxy_user, cx)
            .trim()
            .to_string();
        session.proxy_password = Self::input_value(&self.inputs.proxy_password, cx);
        Ok(session)
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut session = match self.build_session(cx) {
            Ok(session) => session,
            Err(error) => {
                let message = error.to_string();
                self.error = Some(message.clone().into());
                crate::feedback::Feedback::warning(window, cx, message);
                cx.notify();
                return;
            }
        };
        let editing_id = self.editing_id.clone();
        let baseline = self.baseline.clone();
        let connect_after_save = self.connect_after_save;
        let editor = cx.entity();
        let feedback_owner = self.owner.clone();
        let result = self.owner.update(cx, |owner, cx| {
            let mut staged = owner.config.clone();
            if let Some(id) = &editing_id {
                let Some(latest) = staged.get(id) else {
                    return Err(anyhow::anyhow!(t!("ssh_editor_target_deleted").to_string()));
                };
                let Some(baseline) = &baseline else {
                    return Err(anyhow::anyhow!(t!("ssh_editor_conflict").to_string()));
                };
                if !same_session_revision(latest, baseline) {
                    return Err(anyhow::anyhow!(t!("ssh_editor_conflict").to_string()));
                }
                session.last_used = latest.last_used.clone();
            }
            staged.upsert(session.clone());
            owner.commit_staged_config_in_window_async(
                staged,
                window,
                move |owner, window, cx| {
                    if editing_id.is_none() || connect_after_save {
                        owner.open_ssh_session(session, cx);
                    }
                    cx.notify();
                    crate::feedback::Feedback::show_for_owner(
                        &feedback_owner,
                        cx,
                        crate::feedback::FeedbackKind::Success,
                        t!("saved"),
                    );
                    crate::app::deregister_auxiliary_window(window.window_handle());
                    window.remove_window();
                },
                move |_, error, window, cx| {
                    let message = error.to_string();
                    editor.update(cx, |editor, cx| {
                        editor.error = Some(message.clone().into());
                        cx.notify();
                    });
                    crate::feedback::Feedback::error(window, cx, message);
                },
                cx,
            );
            Ok(())
        });
        match result {
            Ok(()) => {}
            Err(error) => {
                let message = error.to_string();
                self.error = Some(message.clone().into());
                crate::feedback::Feedback::error(window, cx, message);
                cx.notify();
            }
        }
    }

    pub(crate) fn apply_managed_key_selection(
        &mut self,
        key_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.managed_key_id = key_id;
        self.error = None;
        cx.notify();
    }

    fn apply_ssh_config_entry(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self.ssh_config_entries.get(index).cloned() else {
            return;
        };
        let user = if entry.user.is_empty() {
            std::env::var("USER")
                .or_else(|_| std::env::var("USERNAME"))
                .unwrap_or_else(|_| "root".to_string())
        } else {
            entry.user.clone()
        };
        self.inputs.name.update(cx, |input, cx| {
            input.set_value(entry.host_alias.clone(), window, cx)
        });
        self.inputs.host.update(cx, |input, cx| {
            input.set_value(entry.hostname.clone(), window, cx)
        });
        self.inputs.port.update(cx, |input, cx| {
            input.set_value(entry.port.to_string(), window, cx)
        });
        self.inputs
            .user
            .update(cx, |input, cx| input.set_value(user, window, cx));
        self.config_key_path = entry.identity_files.first().cloned().unwrap_or_default();
        self.ssh_config_selected = Some(index);
        self.auth = AuthMethod::Config;
        self.error = None;
        cx.notify();
    }

    fn render_key_fields(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let managed_keys = self.owner.read(cx).config.managed_keys().to_vec();
        let selected_label = self
            .managed_key_id
            .as_ref()
            .and_then(|id| managed_keys.iter().find(|key| &key.id == id))
            .map(|key| format!("{} ({})", key.name, key.key_type))
            .unwrap_or_else(|| t!("select_managed_key").to_string());

        Button::new("ssh-editor-managed-key")
            .w_full()
            .label(selected_label)
            .on_click(cx.listener(|this, _, window, cx| {
                let owner = this.owner.clone();
                let editor = cx.entity();
                let selected = this.managed_key_id.clone();
                owner.update(cx, |owner, cx| {
                    owner.open_managed_key_selector_for_editor(editor, selected, window, cx);
                });
            }))
            .into_any_element()
    }
}

impl Render for SshEditorWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = cx.entity();
        let is_editing = self.editing_id.is_some();
        let groups = self.owner.read(cx).config.connection_groups().to_vec();
        let group_label = self
            .group
            .clone()
            .unwrap_or_else(|| t!("ssh_editor_group_unselected").to_string());
        let auth_label = match self.auth {
            AuthMethod::Password => t!("ssh_editor_password_label").to_string(),
            AuthMethod::Config => t!("ssh_config").to_string(),
            AuthMethod::Key | AuthMethod::KeyPending => t!("ssh_editor_key_label").to_string(),
        };
        let ssh_config_label = self
            .ssh_config_selected
            .and_then(|index| self.ssh_config_entries.get(index))
            .map(|entry| entry.host_alias.clone())
            .unwrap_or_else(|| t!("ssh_config").to_string());
        let proxy_label = match self.proxy_type.as_str() {
            "socks5" => "SOCKS5".to_string(),
            "http" => "HTTP".to_string(),
            _ => t!("proxy_none").to_string(),
        };

        let general_section = v_flex()
            .relative()
            .mt_2()
            .gap_2()
            .px_3()
            .pb_3()
            .pt_4()
            .border_l_1()
            .border_r_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .rounded_md()
            .child(
                h_flex()
                    .absolute()
                    .top(px(-10.))
                    .left_0()
                    .right_0()
                    .items_center()
                    .child(gpui::div().w(px(12.)).h(px(1.)).bg(cx.theme().border))
                    .child(
                        gpui::div()
                            .px_2()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(t!("ssh_editor_general").to_string()),
                    )
                    .child(gpui::div().flex_1().h(px(1.)).bg(cx.theme().border)),
            )
            .child(
                h_flex()
                    .gap_4()
                    .child(
                        h_flex()
                            .flex_1()
                            .gap_3()
                            .child(
                                gpui::div()
                                    .w(px(64.))
                                    .whitespace_nowrap()
                                    .child(t!("ssh_editor_connection_type").to_string()),
                            )
                            .child(
                                Button::new("ssh-editor-type")
                                    .flex_1()
                                    .label("SSH / SFTP")
                                    .dropdown_caret(true)
                                    .dropdown_menu(|menu, _, _| {
                                        menu.item(PopupMenuItem::new("SSH / SFTP").checked(true))
                                    }),
                            ),
                    )
                    .child(
                        h_flex()
                            .w(px(230.))
                            .gap_3()
                            .child(gpui::div().child(t!("connection_group").to_string()))
                            .child(
                                Button::new("ssh-editor-group")
                                    .flex_1()
                                    .label(group_label)
                                    .dropdown_caret(true)
                                    .dropdown_menu({
                                        let groups = groups.clone();
                                        let editor = editor.clone();
                                        move |mut menu, window, _| {
                                            menu = menu.item(
                                                PopupMenuItem::new(
                                                    t!("ssh_editor_group_unselected").to_string(),
                                                )
                                                .on_click(window.listener_for(
                                                    &editor,
                                                    |this, _, _, cx| {
                                                        this.group = None;
                                                        cx.notify();
                                                    },
                                                )),
                                            );
                                            for group in &groups {
                                                let selected = group.clone();
                                                menu = menu.item(
                                                    PopupMenuItem::new(group.clone()).on_click(
                                                        window.listener_for(
                                                            &editor,
                                                            move |this, _, _, cx| {
                                                                this.group = Some(selected.clone());
                                                                cx.notify();
                                                            },
                                                        ),
                                                    ),
                                                );
                                            }
                                            menu
                                        }
                                    }),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .gap_3()
                    .child(
                        gpui::div()
                            .w(px(64.))
                            .whitespace_nowrap()
                            .child(t!("name").to_string()),
                    )
                    .child(Input::new(&self.inputs.name).flex_1()),
            )
            .child(
                h_flex()
                    .gap_3()
                    .child(
                        gpui::div()
                            .w(px(64.))
                            .whitespace_nowrap()
                            .child(t!("ssh_editor_host_label").to_string()),
                    )
                    .child(Input::new(&self.inputs.host).flex_1()),
            )
            .child(
                gpui::div()
                    .ml(px(76.))
                    .text_size(rems(0.76))
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("ssh_editor_host_hint").to_string()),
            )
            .child(
                h_flex()
                    .gap_3()
                    .child(
                        gpui::div()
                            .w(px(64.))
                            .whitespace_nowrap()
                            .child(t!("ssh_editor_port_label").to_string()),
                    )
                    .child(Input::new(&self.inputs.port).w(px(160.))),
            );

        let auth_method = Button::new("ssh-editor-auth-method")
            .w(px(190.))
            .label(auth_label)
            .dropdown_caret(true)
            .dropdown_menu({
                let editor = editor.clone();
                move |menu, window, _| {
                    menu.item(
                        PopupMenuItem::new(t!("ssh_editor_password_label").to_string()).on_click(
                            window.listener_for(&editor, |this, _, _, cx| {
                                this.auth = AuthMethod::Password;
                                cx.notify();
                            }),
                        ),
                    )
                    .item(
                        PopupMenuItem::new(t!("ssh_editor_key_label").to_string()).on_click(
                            window.listener_for(&editor, |this, _, _, cx| {
                                this.auth = AuthMethod::Key;
                                cx.notify();
                            }),
                        ),
                    )
                    .item(
                        PopupMenuItem::new(t!("ssh_config").to_string()).on_click(
                            window.listener_for(&editor, |this, _, _, cx| {
                                this.auth = AuthMethod::Config;
                                cx.notify();
                            }),
                        ),
                    )
                }
            });

        let ssh_config_entries = self.ssh_config_entries.clone();
        let config_selector = Button::new("ssh-editor-openssh-config")
            .w_full()
            .label(ssh_config_label)
            .dropdown_caret(true)
            .dropdown_menu({
                let editor = editor.clone();
                move |mut menu, window, _| {
                    if ssh_config_entries.is_empty() {
                        return menu.item(
                            PopupMenuItem::new(t!("ssh_config_empty").to_string()).disabled(true),
                        );
                    }
                    for (index, entry) in ssh_config_entries.iter().enumerate() {
                        let label = if entry.user.is_empty() {
                            format!("{} — {}:{}", entry.host_alias, entry.hostname, entry.port)
                        } else {
                            format!(
                                "{} — {}@{}:{}",
                                entry.host_alias, entry.user, entry.hostname, entry.port
                            )
                        };
                        menu = menu.item(PopupMenuItem::new(label).on_click(window.listener_for(
                            &editor,
                            move |this, _, window, cx| {
                                this.apply_ssh_config_entry(index, window, cx);
                            },
                        )));
                    }
                    menu
                }
            });

        let authentication_section = v_flex()
            .relative()
            .mt_2()
            .gap_2()
            .px_3()
            .pb_3()
            .pt_4()
            .border_l_1()
            .border_r_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .rounded_md()
            .child(
                h_flex()
                    .absolute()
                    .top(px(-10.))
                    .left_0()
                    .right_0()
                    .items_center()
                    .child(gpui::div().w(px(12.)).h(px(1.)).bg(cx.theme().border))
                    .child(
                        gpui::div()
                            .px_2()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(t!("ssh_editor_authentication").to_string()),
                    )
                    .child(gpui::div().flex_1().h(px(1.)).bg(cx.theme().border)),
            )
            .child(
                h_flex()
                    .gap_4()
                    .child(
                        h_flex()
                            .flex_1()
                            .gap_3()
                            .child(
                                gpui::div()
                                    .w(px(64.))
                                    .whitespace_nowrap()
                                    .child(t!("ssh_editor_auth_method").to_string()),
                            )
                            .child(auth_method),
                    )
                    .child(
                        h_flex()
                            .flex_1()
                            .gap_3()
                            .child(
                                gpui::div()
                                    .w(px(56.))
                                    .whitespace_nowrap()
                                    .child(t!("ssh_editor_user_label").to_string()),
                            )
                            .child(Input::new(&self.inputs.user).flex_1()),
                    ),
            )
            .when(self.auth == AuthMethod::Password, |this| {
                this.child(
                    h_flex()
                        .gap_3()
                        .child(
                            gpui::div()
                                .w(px(64.))
                                .whitespace_nowrap()
                                .child(t!("ssh_editor_password_label").to_string()),
                        )
                        .child(Input::new(&self.inputs.password).flex_1().mask_toggle()),
                )
            })
            .when(self.auth == AuthMethod::Key, |this| {
                this.child(
                    h_flex()
                        .items_start()
                        .gap_3()
                        .child(
                            gpui::div()
                                .w(px(64.))
                                .pt_2()
                                .whitespace_nowrap()
                                .child(t!("ssh_editor_key_label").to_string()),
                        )
                        .child(gpui::div().flex_1().child(self.render_key_fields(cx))),
                )
            })
            .when(self.auth == AuthMethod::Config, |this| {
                this.child(
                    h_flex()
                        .items_start()
                        .gap_3()
                        .child(
                            gpui::div()
                                .w(px(64.))
                                .pt_2()
                                .whitespace_nowrap()
                                .child(t!("ssh_config").to_string()),
                        )
                        .child(gpui::div().flex_1().child(config_selector)),
                )
            });

        let connection_page = v_flex()
            .id("ssh-editor-connection-scroll")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .gap_3()
            .p_4()
            .when(!self.connect_after_save, |this| this.child(general_section))
            .child(authentication_section);

        let proxy_page = v_flex()
            .id("ssh-editor-proxy-scroll")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .p_4()
            .child(
                v_flex()
                    .gap_2()
                    .p_3()
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded_md()
                    .child(
                        gpui::div()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(t!("ssh_editor_proxy_server").to_string()),
                    )
                    .child(
                        h_flex()
                            .gap_3()
                            .child(
                                gpui::div()
                                    .w(px(88.))
                                    .child(t!("ssh_editor_proxy_type").to_string()),
                            )
                            .child(
                                Button::new("ssh-editor-proxy-type")
                                    .w(px(220.))
                                    .label(proxy_label)
                                    .dropdown_caret(true)
                                    .dropdown_menu({
                                        let editor = editor.clone();
                                        move |menu, window, _| {
                                            menu.item(
                                                PopupMenuItem::new(t!("proxy_none").to_string())
                                                    .on_click(window.listener_for(
                                                        &editor,
                                                        |this, _, _, cx| {
                                                            this.proxy_type = "none".to_string();
                                                            cx.notify();
                                                        },
                                                    )),
                                            )
                                            .item(PopupMenuItem::new("SOCKS5").on_click(
                                                window.listener_for(&editor, |this, _, _, cx| {
                                                    this.proxy_type = "socks5".to_string();
                                                    cx.notify();
                                                }),
                                            ))
                                            .item(
                                                PopupMenuItem::new("HTTP").on_click(
                                                    window.listener_for(
                                                        &editor,
                                                        |this, _, _, cx| {
                                                            this.proxy_type = "http".to_string();
                                                            cx.notify();
                                                        },
                                                    ),
                                                ),
                                            )
                                        }
                                    }),
                            ),
                    )
                    .when(self.proxy_type != "none", |this| {
                        this.child(
                            h_flex()
                                .gap_3()
                                .child(gpui::div().w(px(88.)).child(t!("proxy_host").to_string()))
                                .child(Input::new(&self.inputs.proxy_host).flex_1())
                                .child(Input::new(&self.inputs.proxy_port).w(px(130.))),
                        )
                        .child(
                            h_flex()
                                .gap_3()
                                .child(gpui::div().w(px(88.)).child(t!("proxy_user").to_string()))
                                .child(Input::new(&self.inputs.proxy_user).flex_1())
                                .child(
                                    Input::new(&self.inputs.proxy_password)
                                        .flex_1()
                                        .mask_toggle(),
                                ),
                        )
                    }),
            );

        let sidebar = v_flex()
            .w(px(164.))
            .h_full()
            .flex_none()
            .gap_1()
            .p_2()
            .border_r_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().muted.opacity(0.25))
            .child(
                gpui::div()
                    .id("ssh-editor-nav-connection")
                    .cursor_pointer()
                    .whitespace_nowrap()
                    .rounded_md()
                    .px_3()
                    .py_2()
                    .when(self.page == SshEditorPage::Connection, |item| {
                        item.bg(cx.theme().primary.opacity(0.10))
                            .text_color(cx.theme().primary)
                    })
                    .when(self.page != SshEditorPage::Connection, |item| {
                        item.hover(|item| item.bg(cx.theme().muted.opacity(0.45)))
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.page = SshEditorPage::Connection;
                        cx.notify();
                    }))
                    .child(t!("ssh_editor_connection").to_string()),
            )
            .child(
                gpui::div()
                    .id("ssh-editor-nav-proxy")
                    .cursor_pointer()
                    .whitespace_nowrap()
                    .rounded_md()
                    .px_3()
                    .py_2()
                    .when(self.page == SshEditorPage::Proxy, |item| {
                        item.bg(cx.theme().primary.opacity(0.10))
                            .text_color(cx.theme().primary)
                    })
                    .when(self.page != SshEditorPage::Proxy, |item| {
                        item.hover(|item| item.bg(cx.theme().muted.opacity(0.45)))
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.page = SshEditorPage::Proxy;
                        cx.notify();
                    }))
                    .child(t!("ssh_editor_proxy_server").to_string()),
            );

        v_flex()
            .size_full()
            .track_focus(&self.focus_handle)
            .on_key_down(|event: &gpui::KeyDownEvent, window, cx| {
                if event.keystroke.key.as_str() == "escape" {
                    window.prevent_default();
                    cx.stop_propagation();
                    crate::app::deregister_auxiliary_window(window.window_handle());
                    window.remove_window();
                }
            })
            .bg(cx.theme().background)
            .when(self.connect_after_save, |this| {
                this.child(
                    v_flex()
                        .flex_none()
                        .gap_1()
                        .px_4()
                        .py_3()
                        .bg(cx.theme().primary.opacity(0.08))
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .child(
                            gpui::div()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(t!("session_credentials_required").to_string()),
                        )
                        .child(
                            gpui::div()
                                .text_color(cx.theme().muted_foreground)
                                .child(t!(if self.auth == AuthMethod::Password {
                                    "session_password_required"
                                } else {
                                    "session_key_required"
                                })),
                        ),
                )
            })
            .child(
                h_flex()
                    .flex_1()
                    .min_h(px(0.))
                    .items_stretch()
                    .when(!self.connect_after_save, |this| this.child(sidebar))
                    .child(if self.page == SshEditorPage::Connection {
                        connection_page.into_any_element()
                    } else {
                        proxy_page.into_any_element()
                    }),
            )
            .when_some(self.error.clone(), |this, error| {
                this.child(
                    gpui::div()
                        .flex_none()
                        .px_4()
                        .py_2()
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .text_size(rems(0.82))
                        .text_color(cx.theme().danger)
                        .child(error),
                )
            })
            .child(
                h_flex()
                    .flex_none()
                    .justify_end()
                    .gap_2()
                    .px_4()
                    .py_2()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        Button::new("ssh-editor-cancel")
                            .secondary()
                            .label(t!("cancel").to_string())
                            .on_click(|_, window, _| {
                                crate::app::deregister_auxiliary_window(window.window_handle());
                                window.remove_window();
                            }),
                    )
                    .child(
                        Button::new("ssh-editor-submit")
                            .primary()
                            .label(if self.connect_after_save {
                                t!("save_and_connect").to_string()
                            } else if is_editing {
                                t!("save").to_string()
                            } else {
                                t!("save_and_connect").to_string()
                            })
                            .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                    ),
            )
            .children(Root::render_dialog_layer(window, cx))
            .children(Root::render_notification_layer(window, cx))
    }
}

fn same_session_revision(left: &Session, right: &Session) -> bool {
    left.id == right.id
        && left.name == right.name
        && left.host == right.host
        && left.port == right.port
        && left.user == right.user
        && left.auth == right.auth
        && left.password == right.password
        && left.private_key_path == right.private_key_path
        && left.private_key_inline == right.private_key_inline
        && left.passphrase == right.passphrase
        && left.managed_key_id == right.managed_key_id
        && left.group == right.group
        && left.proxy_type == right.proxy_type
        && left.proxy_host == right.proxy_host
        && left.proxy_port == right.proxy_port
        && left.proxy_user == right.proxy_user
        && left.proxy_password == right.proxy_password
}

fn window_options(cx: &mut App, compact: bool) -> WindowOptions {
    let (min_size, preferred_size) = if compact {
        (size(px(540.), px(340.)), size(px(560.), px(360.)))
    } else {
        (size(px(680.), px(420.)), size(px(620.), px(400.)))
    };
    crate::app::platform::auxiliary_window_options(
        cx,
        crate::app::platform::AuxiliaryWindowSpec::new(preferred_size)
            .with_min_size(min_size)
            .with_max_ratio(0.9, 0.9),
    )
}

pub(crate) fn open(owner: Entity<TinyShell>, request: SshEditorRequest, cx: &mut App) {
    let owner_id = owner.read(cx).session_owner_id;
    let credentials_only = matches!(&request, SshEditorRequest::Credentials { .. });
    let editing = matches!(
        request,
        SshEditorRequest::Edit { .. } | SshEditorRequest::Credentials { .. }
    );
    let title = if credentials_only {
        t!("session_credentials_required").to_string()
    } else if editing {
        t!("create_or_edit_ssh_session").to_string()
    } else {
        t!("new_ssh_connection").to_string()
    };
    let owner_for_window = owner.clone();
    let options = window_options(cx, credentials_only);
    let opened = cx.open_window(options, move |window, cx| {
        window.set_window_title(&title);
        let window_handle = window.window_handle();
        crate::app::register_auxiliary_window(window_handle, owner_id);
        let editor = cx.new(|cx| SshEditorWindow::new(owner_for_window, request, window, cx));
        let focus_input =
            if editor.read(cx).connect_after_save && editor.read(cx).auth == AuthMethod::Password {
                editor.read(cx).inputs.password.clone()
            } else {
                editor.read(cx).inputs.host.clone()
            };
        window.defer(cx, move |window, cx| {
            window.activate_window();
            window.focus(&focus_input.read(cx).focus_handle(cx), cx);
        });
        window.on_window_should_close(cx, move |_, _| {
            crate::app::deregister_auxiliary_window(window_handle);
            true
        });
        cx.new(|cx| Root::new(editor, window, cx))
    });
    if let Err(error) = opened {
        tracing::error!("failed to open SSH editor window: {error:?}");
        crate::feedback::Feedback::show_for_owner(
            &owner,
            cx,
            crate::feedback::FeedbackKind::Error,
            t!(
                "connection_manager_action_failed",
                error = format!("{error:?}")
            )
            .to_string(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Session {
        let mut session = Session::password(
            "example.test".to_string(),
            22,
            "root".to_string(),
            "secret".to_string(),
        );
        session.id = "session-1".to_string();
        session.name = "example".to_string();
        session
    }

    #[test]
    fn unchanged_session_revision_is_accepted() {
        let baseline = session();
        assert!(same_session_revision(&baseline, &baseline.clone()));
    }

    #[test]
    fn runtime_last_used_change_does_not_conflict() {
        let baseline = session();
        let mut latest = baseline.clone();
        latest.last_used = Some("2026-03-14T00:00:00Z".to_string());
        assert!(same_session_revision(&latest, &baseline));
    }

    #[test]
    fn concurrent_session_change_is_detected() {
        let baseline = session();
        let mut latest = baseline.clone();
        latest.host = "changed.example.test".to_string();
        assert!(!same_session_revision(&latest, &baseline));
    }
}
