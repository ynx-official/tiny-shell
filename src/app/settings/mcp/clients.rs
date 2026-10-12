use super::{ConfirmationDialog, Feedback, McpPanel, Snapshot, heading, hint, mcp, report};
use gpui::{
    ClipboardItem, Context, InteractiveElement as _, IntoElement as _, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::{DropdownMenu as _, PopupMenuItem},
    v_flex,
};
use rust_i18n::t;

const CLIENTS: [&str; 4] = ["Claude Code", "Codex CLI", "OpenCode", "MCP Client"];

impl McpPanel {
    fn register(&mut self, label: &str, window: &mut Window, cx: &mut Context<Self>) {
        let result = (|| {
            let service = mcp::service()?;
            let clients = service
                .engine
                .store
                .clients()
                .map_err(|_| "STORAGE_ERROR".to_string())?;
            // Repeated clicks should not issue additional secrets for the same template.
            if let Some(existing) = clients.iter().find(|c| c.enabled && c.label == label) {
                self.selected_client = Some(existing.id.clone());
                return Ok(false);
            }
            let client = service
                .engine
                .store
                .register(label)
                .map_err(|_| "STORAGE_ERROR".to_string())?;
            self.selected_client = Some(client.id);
            Ok(true)
        })();
        match result {
            Ok(true) => Feedback::success(window, cx, t!("mcp_registration_done").to_string()),
            Ok(false) => Feedback::info(window, cx, t!("mcp_registration_existing").to_string()),
            Err(code) => {
                report(Err(code), "mcp_registration_done", window, cx);
            }
        }
        cx.refresh_windows();
    }

    pub(super) fn render_clients(
        &self,
        snapshot: &Snapshot,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let panel = cx.entity();
        let active = snapshot.clients.iter().filter(|c| c.enabled).count();
        let revoked = snapshot.clients.len() - active;
        let mut rows = v_flex()
            .id("mcp-clients-list")
            .w_full()
            .max_h(px(220.))
            .overflow_y_scroll()
            .gap_2();
        for client in snapshot
            .clients
            .iter()
            .filter(|c| c.enabled || self.show_revoked)
        {
            let id = client.id.clone();
            let copy_id = id.clone();
            let revoke_id = id.clone();
            let name = format!("{} · {}", client.label, short_id(&id));
            rows = rows.child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .py_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(120.))
                            .gap_1()
                            .child(heading(name))
                            .child(hint(
                                t!(if client.enabled {
                                    "mcp_registered"
                                } else {
                                    "mcp_revoked"
                                })
                                .to_string(),
                                cx,
                            )),
                    )
                    .child(
                        Button::new(format!("mcp-preview-{id}"))
                            .small()
                            .ghost()
                            .disabled(!client.enabled)
                            .label(t!("mcp_config_preview").to_string())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.selected_client = if this.selected_client.as_ref() == Some(&id)
                                {
                                    None
                                } else {
                                    Some(id.clone())
                                };
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new(format!("mcp-copy-{copy_id}"))
                            .small()
                            .outline()
                            .disabled(!client.enabled)
                            .label(t!("mcp_copy_config").to_string())
                            .on_click(move |_, window, cx| copy_config(&copy_id, window, cx)),
                    )
                    .child(
                        Button::new(format!("mcp-revoke-{revoke_id}"))
                            .small()
                            .ghost()
                            .disabled(!client.enabled)
                            .label(t!("mcp_revoke").to_string())
                            .on_click(move |_, window, cx| {
                                let id = revoke_id.clone();
                                ConfirmationDialog::new(
                                    t!("mcp_revoke").to_string(),
                                    t!("mcp_revoke_confirm").to_string(),
                                )
                                .danger(true)
                                .on_ok(move |_, window, cx| {
                                    let result = mcp::service().and_then(|service| {
                                        let _enter = service.runtime.enter();
                                        service.engine.revoke_client(&id).map_err(|e| e.code)
                                    });
                                    let success = report(result, "mcp_revoked_done", window, cx);
                                    cx.refresh_windows();
                                    success
                                })
                                .open(window, cx);
                            }),
                    ),
            );
        }
        if active == 0 && (!self.show_revoked || revoked == 0) {
            rows = rows.child(
                div()
                    .py_4()
                    .child(hint(t!("mcp_no_clients").to_string(), cx)),
            );
        }
        let mut content = v_flex()
            .w_full()
            .min_w_0()
            .gap_3()
            .child(
                h_flex()
                    .justify_between()
                    .gap_2()
                    .flex_wrap()
                    .child(heading(t!("mcp_clients").to_string()))
                    .child(
                        Button::new("mcp-add-client")
                            .small()
                            .icon(IconName::Plus)
                            .label(t!("mcp_add_client").to_string())
                            .disabled(snapshot.policy.is_none())
                            .dropdown_menu(move |mut menu, window, _cx| {
                                for label in CLIENTS {
                                    menu = menu.item(PopupMenuItem::new(label).on_click(
                                        window.listener_for(&panel, move |this, _, window, cx| {
                                            this.register(label, window, cx)
                                        }),
                                    ));
                                }
                                menu
                            }),
                    ),
            )
            .child(hint(t!("mcp_client_setup_hint").to_string(), cx))
            .child(rows)
            .when(revoked > 0, |content| {
                content.child(
                    Button::new("mcp-show-revoked")
                        .small()
                        .ghost()
                        .label(
                            t!(
                                if self.show_revoked {
                                    "mcp_hide_revoked"
                                } else {
                                    "mcp_show_revoked"
                                },
                                count = revoked
                            )
                            .to_string(),
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_revoked = !this.show_revoked;
                            cx.notify();
                        })),
                )
            });
        if let Some(client) = self
            .selected_client
            .as_ref()
            .and_then(|id| snapshot.clients.iter().find(|c| c.id == *id && c.enabled))
        {
            content = content
                .child(heading(
                    t!("mcp_config_for", client = &client.label).to_string(),
                ))
                .child(hint(t!(setup_key(&client.label)).to_string(), cx));
            match config_for(&client.id, &client.label) {
                Ok(config) => {
                    let id = client.id.clone();
                    content = content
                        .child(code_block("mcp-client-config", &config, 180., cx))
                        .child(
                            Button::new("mcp-copy-preview")
                                .small()
                                .outline()
                                .label(t!("mcp_copy_config").to_string())
                                .on_click(move |_, window, cx| copy_config(&id, window, cx)),
                        );
                }
                Err(_) => content = content.child(hint(t!("mcp_config_failed").to_string(), cx)),
            }
        }
        content.into_any_element()
    }
}

fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}
fn setup_key(label: &str) -> &'static str {
    match label {
        "Codex CLI" => "mcp_setup_codex",
        "OpenCode" => "mcp_setup_opencode",
        "Claude Code" => "mcp_setup_claude",
        _ => "mcp_setup_generic",
    }
}
fn config_for(id: &str, label: &str) -> anyhow::Result<String> {
    let executable = std::env::current_exe()?;
    let appimage = std::env::var_os("APPIMAGE").map(std::path::PathBuf::from);
    let config = tiny_shell_mcp::launch::client_config(&executable, appimage.as_deref(), id)?;
    tiny_shell_mcp::launch::format_client_config(label, &config)
}
fn copy_config(id: &str, window: &mut Window, cx: &mut gpui::App) {
    let result = (|| {
        let service = mcp::service()?;
        let client = service
            .engine
            .store
            .client(id)
            .map_err(|_| "STORAGE_ERROR".to_string())?;
        if !client.enabled {
            return Err("CLIENT_REVOKED".into());
        }
        let config = config_for(id, &client.label).map_err(|_| "CONFIG_ERROR".to_string())?;
        cx.write_to_clipboard(ClipboardItem::new_string(config));
        Ok(())
    })();
    report(result, "mcp_config_copied", window, cx);
}

pub(super) fn code_block(
    id: impl Into<gpui::ElementId>,
    text: &str,
    height: f32,
    cx: &gpui::App,
) -> gpui::AnyElement {
    v_flex()
        .id(id)
        .w_full()
        .max_h(px(height))
        .overflow_scroll()
        .p_3()
        .gap_1()
        .bg(cx.theme().muted)
        .rounded(px(6.))
        .text_sm()
        .font_family("monospace")
        .children(
            text.lines()
                .map(|line| div().whitespace_nowrap().child(line.to_owned())),
        )
        .into_any_element()
}
