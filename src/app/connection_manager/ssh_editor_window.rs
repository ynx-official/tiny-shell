use gpui::{
    App, AppContext as _, Bounds, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled,
    Window, WindowOptions, point, prelude::FluentBuilder as _, px, rems, size,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Root, Sizable as _,
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
    key_path: Entity<InputState>,
    key_inline: Entity<InputState>,
    passphrase: Entity<InputState>,
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
    using_custom_key: bool,
    ssh_config_entries: Vec<SshConfigEntry>,
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
        let (editing_id, baseline, session, group) = match request {
            SshEditorRequest::New { group, prefill } => (None, None, prefill, group),
            SshEditorRequest::Edit { session } => (
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
            .map_or(AuthMethod::Password, |item| item.auth);
        let proxy_type = session
            .as_ref()
            .map(|item| item.proxy_type.clone())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "none".to_string());
        let managed_key_id = session
            .as_ref()
            .and_then(|item| item.managed_key_id.clone());
        let using_custom_key = session.as_ref().is_some_and(|item| {
            item.auth == AuthMethod::Key
                && item.managed_key_id.is_none()
                && (!item.private_key_path.is_empty() || !item.private_key_inline.is_empty())
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
            key_path: new_input(
                window,
                t!("private_key_path").to_string(),
                session
                    .as_ref()
                    .map(|item| item.private_key_path.clone())
                    .unwrap_or_default(),
                false,
                cx,
            ),
            key_inline: cx.new(|cx| {
                InputState::new(window, cx)
                    .multi_line(true)
                    .rows(5)
                    .placeholder(t!("private_key_data").to_string())
                    .default_value(
                        session
                            .as_ref()
                            .map(|item| item.private_key_inline.clone())
                            .unwrap_or_default(),
                    )
            }),
            passphrase: new_input(
                window,
                t!("key_passphrase").to_string(),
                session
                    .as_ref()
                    .map(|item| item.passphrase.clone())
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
            &inputs.key_path,
            &inputs.key_inline,
            &inputs.passphrase,
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
            using_custom_key,
            ssh_config_entries: crate::session::ssh_config::parse_ssh_config().unwrap_or_default(),
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

    fn set_input(
        input: &Entity<InputState>,
        value: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        input.update(cx, |input, cx| input.set_value(value, window, cx));
    }

    fn select_config(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.ssh_config_entries.get(index).cloned() else {
            return;
        };
        let user = if entry.user.is_empty() {
            std::env::var("USER")
                .or_else(|_| std::env::var("USERNAME"))
                .unwrap_or_else(|_| "root".to_string())
        } else {
            entry.user
        };
        Self::set_input(&self.inputs.name, entry.host_alias, window, cx);
        Self::set_input(&self.inputs.host, entry.hostname, window, cx);
        Self::set_input(&self.inputs.port, entry.port.to_string(), window, cx);
        Self::set_input(&self.inputs.user, user, window, cx);
        Self::set_input(
            &self.inputs.key_path,
            entry.identity_files.first().cloned().unwrap_or_default(),
            window,
            cx,
        );
        cx.notify();
    }

    fn pick_key_path(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let start_dir = directories::BaseDirs::new()
            .map(|dirs| dirs.home_dir().join(".ssh"))
            .unwrap_or_else(|| std::path::PathBuf::from("/"));
        let picker = rfd::AsyncFileDialog::new()
            .set_directory(start_dir)
            .pick_file();
        cx.spawn_in(window, async move |this, cx| {
            if let Some(file) = picker.await {
                let path = file.path().to_string_lossy().to_string();
                cx.update(|window, cx| {
                    this.update(cx, |this, cx| {
                        Self::set_input(&this.inputs.key_path, path, window, cx);
                    })
                })??;
            }
            Ok::<(), anyhow::Error>(())
        })
        .detach();
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
        if self.auth == AuthMethod::Key && !self.using_custom_key && self.managed_key_id.is_none() {
            anyhow::bail!(t!("select_managed_key_hint").to_string());
        }

        let password = Self::input_value(&self.inputs.password, cx);
        let key_path = Self::input_value(&self.inputs.key_path, cx)
            .trim()
            .to_string();
        let key_inline = Self::input_value(&self.inputs.key_inline, cx);
        let passphrase = Self::input_value(&self.inputs.passphrase, cx);
        let mut session = match self.auth {
            AuthMethod::Password => Session::password(host.clone(), port, user.clone(), password),
            AuthMethod::Key if !self.using_custom_key => {
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
            AuthMethod::Key => Session::key(
                host.clone(),
                port,
                user.clone(),
                key_path,
                key_inline,
                passphrase,
            ),
            AuthMethod::Config => {
                let mut session = Session::key(
                    host.clone(),
                    port,
                    user.clone(),
                    key_path,
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
                self.error = Some(error.to_string().into());
                cx.notify();
                return;
            }
        };
        let editing_id = self.editing_id.clone();
        let baseline = self.baseline.clone();
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
            staged.save()?;
            owner.config = staged;
            if editing_id.is_none() {
                owner.open_ssh_session(session, cx);
            }
            cx.notify();
            Ok(())
        });
        match result {
            Ok(()) => window.remove_window(),
            Err(error) => {
                self.error = Some(error.to_string().into());
                cx.notify();
            }
        }
    }

    fn render_key_fields(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let editor = cx.entity();
        let managed_keys = self.owner.read(cx).config.managed_keys().to_vec();
        let selected_label = self
            .managed_key_id
            .as_ref()
            .and_then(|id| managed_keys.iter().find(|key| &key.id == id))
            .map(|key| format!("{} ({})", key.name, key.key_type))
            .unwrap_or_else(|| t!("select_managed_key").to_string());
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("ssh-editor-managed-mode")
                            .small()
                            .flex_1()
                            .when(!self.using_custom_key, |button| button.primary())
                            .label(t!("select_managed_key").to_string())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.using_custom_key = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("ssh-editor-custom-mode")
                            .small()
                            .flex_1()
                            .when(self.using_custom_key, |button| button.primary())
                            .label(t!("use_custom_path").to_string())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.using_custom_key = true;
                                this.managed_key_id = None;
                                cx.notify();
                            })),
                    ),
            )
            .when(!self.using_custom_key, |this| {
                this.child(
                    Button::new("ssh-editor-managed-key")
                        .w_full()
                        .label(selected_label)
                        .dropdown_menu({
                            let managed_keys = managed_keys.clone();
                            move |mut menu, window, _| {
                                for key in &managed_keys {
                                    let key_id = key.id.clone();
                                    menu = menu.item(
                                        PopupMenuItem::new(format!(
                                            "{} ({})",
                                            key.name, key.key_type
                                        ))
                                        .on_click(
                                            window.listener_for(&editor, move |this, _, _, cx| {
                                                this.managed_key_id = Some(key_id.clone());
                                                cx.notify();
                                            }),
                                        ),
                                    );
                                }
                                menu
                            }
                        }),
                )
            })
            .when(self.using_custom_key, |this| {
                this.child(
                    h_flex()
                        .gap_2()
                        .child(Input::new(&self.inputs.key_path).flex_1())
                        .child(
                            Button::new("ssh-editor-browse-key")
                                .label(t!("browse").to_string())
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.pick_key_path(window, cx)
                                })),
                        ),
                )
                .child(Input::new(&self.inputs.key_inline).h(px(88.)))
                .child(Input::new(&self.inputs.passphrase).mask_toggle())
            })
            .into_any_element()
    }
}

impl Render for SshEditorWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = cx.entity();
        let is_editing = self.editing_id.is_some();
        let groups = self.owner.read(cx).config.connection_groups().to_vec();
        let group_label = self
            .group
            .clone()
            .unwrap_or_else(|| t!("ssh_editor_group_unselected").to_string());
        let auth_label = match self.auth {
            AuthMethod::Password => t!("ssh_editor_password_label").to_string(),
            AuthMethod::Key => t!("ssh_editor_key_label").to_string(),
            AuthMethod::Config => t!("ssh_editor_config_label").to_string(),
        };
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
            .border_1()
            .border_color(cx.theme().border)
            .rounded_md()
            .child(
                gpui::div()
                    .absolute()
                    .top(px(-10.))
                    .left(px(12.))
                    .px_2()
                    .bg(cx.theme().background)
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(t!("ssh_editor_general").to_string()),
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
                        PopupMenuItem::new(t!("ssh_editor_config_label").to_string()).on_click(
                            window.listener_for(&editor, |this, _, _, cx| {
                                this.auth = AuthMethod::Config;
                                cx.notify();
                            }),
                        ),
                    )
                }
            });

        let authentication_section = v_flex()
            .relative()
            .mt_2()
            .gap_2()
            .px_3()
            .pb_3()
            .pt_4()
            .border_1()
            .border_color(cx.theme().border)
            .rounded_md()
            .child(
                gpui::div()
                    .absolute()
                    .top(px(-10.))
                    .left(px(12.))
                    .px_2()
                    .bg(cx.theme().background)
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(t!("ssh_editor_authentication").to_string()),
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
                let list = v_flex()
                    .gap_2()
                    .when(self.ssh_config_entries.is_empty(), |list| {
                        list.child(
                            gpui::div()
                                .text_color(cx.theme().muted_foreground)
                                .child(t!("ssh_config_empty").to_string()),
                        )
                    })
                    .children(self.ssh_config_entries.clone().into_iter().enumerate().map(
                        |(index, entry)| {
                            gpui::div()
                                .id(("ssh-editor-config-entry", index))
                                .cursor_pointer()
                                .rounded_md()
                                .border_1()
                                .border_color(cx.theme().border)
                                .p_2()
                                .hover(|row| row.bg(cx.theme().secondary))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.select_config(index, window, cx)
                                }))
                                .child(format!(
                                    "{} — {}@{}:{}",
                                    entry.host_alias, entry.user, entry.hostname, entry.port
                                ))
                        },
                    ));
                this.child(
                    h_flex()
                        .items_start()
                        .gap_3()
                        .child(
                            gpui::div()
                                .w(px(64.))
                                .pt_2()
                                .whitespace_nowrap()
                                .child(t!("ssh_editor_config_label").to_string()),
                        )
                        .child(gpui::div().flex_1().child(list)),
                )
            });

        let connection_page = v_flex()
            .id("ssh-editor-connection-scroll")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .gap_3()
            .p_4()
            .child(general_section)
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
                    window.remove_window();
                }
            })
            .bg(cx.theme().background)
            .child(
                h_flex()
                    .flex_1()
                    .min_h(px(0.))
                    .items_stretch()
                    .child(sidebar)
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
                            .on_click(|_, window, _| window.remove_window()),
                    )
                    .child(
                        Button::new("ssh-editor-submit")
                            .primary()
                            .disabled(
                                self.auth == AuthMethod::Config
                                    && Self::input_value(&self.inputs.host, cx).trim().is_empty(),
                            )
                            .label(if is_editing {
                                t!("save").to_string()
                            } else {
                                t!("save_and_connect").to_string()
                            })
                            .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                    ),
            )
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

fn window_options(cx: &App) -> WindowOptions {
    let mut options = WindowOptions {
        is_movable: true,
        is_resizable: true,
        is_minimizable: true,
        window_min_size: Some(size(px(680.), px(420.))),
        ..Default::default()
    };
    if let Some(display) = cx.displays().first().cloned() {
        let display_bounds = display.bounds();
        let window_size = size(
            px(820.).min(display_bounds.size.width * 0.9),
            px(470.).min(display_bounds.size.height * 0.9),
        );
        let origin = point(
            display_bounds.origin.x + (display_bounds.size.width - window_size.width) / 2.,
            display_bounds.origin.y + (display_bounds.size.height - window_size.height) / 2.,
        );
        options.window_bounds = Some(gpui::WindowBounds::Windowed(Bounds::new(
            origin,
            window_size,
        )));
    }
    #[cfg(not(target_os = "macos"))]
    if let Ok(image) =
        image::load_from_memory(include_bytes!("../../../assets/icons/tiny-shell.png"))
    {
        options.icon = Some(std::sync::Arc::new(image.into_rgba8()));
    }
    options
}

pub(crate) fn open(owner: Entity<TinyShell>, request: SshEditorRequest, cx: &mut App) {
    let editing = matches!(request, SshEditorRequest::Edit { .. });
    let title = if editing {
        t!("create_or_edit_ssh_session").to_string()
    } else {
        t!("new_ssh_connection").to_string()
    };
    let opened = cx.open_window(window_options(cx), move |window, cx| {
        window.set_window_title(&title);
        let editor = cx.new(|cx| SshEditorWindow::new(owner, request, window, cx));
        let host = editor.read(cx).inputs.host.clone();
        window.defer(cx, move |window, cx| {
            window.activate_window();
            window.focus(&host.read(cx).focus_handle(cx), cx);
        });
        cx.new(|cx| Root::new(editor, window, cx))
    });
    if let Err(error) = opened {
        tracing::error!("failed to open SSH editor window: {error:?}");
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
