use super::{
    McpPanel, PolicyChange, Snapshot, heading, hint,
    model::{self, GrantState},
};
use gpui::{
    Context, InteractiveElement as _, IntoElement as _, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex,
    input::Input,
    radio::Radio,
    scroll::ScrollableElement as _,
    v_flex,
};
use rust_i18n::t;
use tiny_shell_mcp::policy::Scope;

impl McpPanel {
    pub(super) fn render_access(
        &self,
        snapshot: &Snapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let policy = snapshot.policy.clone().unwrap_or_default();
        let disabled = snapshot.policy.is_none();
        let narrow = window.viewport_size().width < px(850.) || window.rem_size() > px(17.);
        let panel = cx.entity();
        let modes = super::permissions::selector(
            snapshot.policy.as_ref().map(|p| p.mode),
            narrow,
            window,
            cx,
            move |mode, window, cx| {
                panel.update(cx, |this, cx| {
                    if super::mcp::service()
                        .ok()
                        .and_then(|s| s.engine.policy().ok())
                        .is_some_and(|p| p.mode != mode)
                    {
                        this.request_policy(PolicyChange::Mode(mode), window, cx);
                    }
                });
            },
        );
        let query = self.search.read(cx).value();
        let visible = snapshot
            .targets
            .iter()
            .filter(|target| {
                model::matches_query(target, &query)
                    && (!self.authorized_only || policy.authorized(&target.id, &target.digest))
            })
            .cloned()
            .collect::<Vec<_>>();
        let authorized = snapshot
            .targets
            .iter()
            .filter(|target| policy.authorized(&target.id, &target.digest))
            .count();
        let mut list = v_flex()
            .id("mcp-connection-list")
            .w_full()
            .max_h(px(if narrow { 208. } else { 248. }))
            .overflow_y_scroll()
            .track_scroll(&self.connections_scroll);
        if visible.is_empty() {
            list = list.child(
                div().p_4().child(hint(
                    t!(if snapshot.targets.is_empty() {
                        "mcp_no_connections"
                    } else {
                        "mcp_no_matches"
                    })
                    .to_string(),
                    cx,
                )),
            );
        }
        for target in &visible {
            let state = model::grant_state(&policy, target);
            let selected = target.clone();
            let full_name = match target.group.as_deref() {
                Some(group) => format!("{} · {group}", target.name),
                None => target.name.clone(),
            };
            let host = if target.host.contains(':') && !target.host.starts_with('[') {
                format!("[{}]", target.host)
            } else {
                target.host.clone()
            };
            let address = format!("{}@{host}:{}", target.user, target.port);
            let (key, color) = match state {
                GrantState::Authorized => ("mcp_grant_authorized", cx.theme().success),
                GrantState::Changed => ("mcp_grant_changed", cx.theme().warning),
                GrantState::Unassigned => ("mcp_grant_unassigned", cx.theme().muted_foreground),
            };
            list = list.child(
                h_flex()
                    .gap_2()
                    .px_2()
                    .py_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(
                                Checkbox::new(format!("mcp-grant-{}", target.id))
                                    .small()
                                    .label(target.name.clone())
                                    .w_full()
                                    .checked(state == GrantState::Authorized)
                                    .disabled(disabled)
                                    .tooltip(full_name.clone())
                                    .on_click(cx.listener(move |this, checked, window, cx| {
                                        this.request_policy(
                                            PolicyChange::Selection(
                                                vec![selected.clone()],
                                                *checked,
                                            ),
                                            window,
                                            cx,
                                        )
                                    })),
                            )
                            .child(hint(address, cx).pl_5().text_ellipsis().overflow_hidden()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .flex_none()
                            .text_color(color)
                            .child(t!(key).to_string()),
                    ),
            );
        }
        let mut scopes = h_flex().flex_wrap().gap_4();
        for (scope, key) in [
            (Scope::Specified, "mcp_scope_specified"),
            (Scope::All, "mcp_scope_all"),
        ] {
            scopes = scopes.child(
                Radio::new(key)
                    .small()
                    .label(t!(key).to_string())
                    .checked(policy.scope == scope)
                    .disabled(disabled)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if super::mcp::service()
                            .ok()
                            .and_then(|s| s.engine.policy().ok())
                            .is_some_and(|p| p.scope != scope)
                        {
                            this.request_policy(PolicyChange::Scope(scope), window, cx);
                        }
                    })),
            );
        }
        let select = visible.clone();
        let clear = visible;
        v_flex()
            .gap_3()
            .w_full()
            .min_w_0()
            .child(
                h_flex()
                    .justify_between()
                    .gap_2()
                    .flex_wrap()
                    .child(heading(t!("mcp_modes").to_string()))
                    .child(
                        Button::new("mcp-capabilities")
                            .small()
                            .ghost()
                            .label(t!("mcp_capabilities").to_string())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.capabilities = !this.capabilities;
                                cx.notify();
                            })),
                    ),
            )
            .child(modes)
            .when(self.capabilities, |panel| {
                panel.child(hint(t!("mcp_capabilities_detail").to_string(), cx))
            })
            .child(
                h_flex()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .pt_3()
                    .justify_between()
                    .gap_2()
                    .child(heading(t!("mcp_connections").to_string()))
                    .child(hint(
                        t!(
                            "mcp_grants_summary",
                            count = authorized,
                            total = snapshot.targets.len()
                        )
                        .to_string(),
                        cx,
                    )),
            )
            .child(scopes)
            .child(hint(t!("mcp_future_hint").to_string(), cx))
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        div().flex_1().min_w(px(180.)).child(
                            Input::new(&self.search)
                                .small()
                                .prefix(Icon::new(IconName::Search).size(px(16.)))
                                .cleanable(true),
                        ),
                    )
                    .child(
                        Checkbox::new("mcp-authorized-only")
                            .small()
                            .label(t!("mcp_authorized_only").to_string())
                            .checked(self.authorized_only)
                            .on_click(cx.listener(|this, checked, _, cx| {
                                this.authorized_only = *checked;
                                this.connections_scroll.scroll_to_item(0);
                                cx.notify();
                            })),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(
                        Button::new("mcp-select-results")
                            .small()
                            .ghost()
                            .disabled(disabled || select.is_empty())
                            .label(t!("mcp_select_results").to_string())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.request_policy(
                                    PolicyChange::Selection(select.clone(), true),
                                    window,
                                    cx,
                                )
                            })),
                    )
                    .child(
                        Button::new("mcp-clear-results")
                            .small()
                            .ghost()
                            .disabled(disabled || clear.is_empty())
                            .label(t!("mcp_clear_results").to_string())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.request_policy(
                                    PolicyChange::Selection(clear.clone(), false),
                                    window,
                                    cx,
                                )
                            })),
                    ),
            )
            .child(
                div()
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded(px(6.))
                    .overflow_hidden()
                    .relative()
                    .child(list)
                    .vertical_scrollbar(&self.connections_scroll),
            )
            .child(hint(t!("mcp_immediate_hint").to_string(), cx))
            .into_any_element()
    }
}
