//! Built-in permissions apply to all conversations, independently of external grants.
use super::{McpPanel, PolicyChange, Snapshot, heading, hint};
use gpui::{Context, IntoElement as _, ParentElement as _, Styled as _, Window, px};
use gpui_component::v_flex;
use rust_i18n::t;

impl McpPanel {
    pub(super) fn render_ai_access(
        &self,
        snapshot: &Snapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let panel = cx.entity();
        let modes = super::permissions::selector(
            snapshot.ai_policy.map(|p| p.mode),
            window.viewport_size().width < px(850.) || window.rem_size() > px(17.),
            window,
            cx,
            move |mode, window, cx| {
                panel.update(cx, |this, cx| {
                    this.request_policy(PolicyChange::AiMode(mode), window, cx);
                });
            },
        );
        v_flex()
            .w_full()
            .gap_3()
            .child(heading(t!("mcp_modes").to_string()))
            .child(modes)
            .child(hint(t!("ai_target_scope_hint").to_string(), cx))
            .into_any_element()
    }
}
