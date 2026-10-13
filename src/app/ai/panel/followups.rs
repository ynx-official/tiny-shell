use super::*;
use crate::app::ai::followups::{self, Action, FollowupView};
use gpui_component::input::RopeExt as _;

impl AiPanel {
    pub(super) fn render_followups(
        &self,
        c: &Conversation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<FollowupView>> {
        let suggestions = c.visible_followups()?;
        let id = c.id.clone();
        let turn = suggestions.turn_id.clone();
        let items = suggestions.items.clone();
        let panel = cx.entity().downgrade();
        Some(
            window.use_keyed_state(format!("ai-followups-{id}-{turn}"), cx, move |_, _| {
                FollowupView::new(items, move |index, action, window, cx| {
                    if let Some(panel) = panel.upgrade() {
                        panel.update(cx, |this, cx| {
                            this.select_followup(&id, &turn, (index, action), window, cx)
                        });
                    }
                })
            }),
        )
    }

    fn select_followup(
        &mut self,
        id: &str,
        turn: &str,
        selection: (usize, Action),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.current.as_deref() != Some(id) {
            return;
        }
        let (index, action) = selection;
        let result = (|| {
            let service = service()?;
            match action {
                Action::Add => {
                    let text = service
                        .followup_text(id, turn, index)
                        .map_err(|e| e.to_string())?;
                    let value = followups::append(&self.input.read(cx).value(), &text);
                    self.input.update(cx, |input, cx| {
                        input.set_value(value, window, cx);
                        let end = input.text().offset_to_position(input.text().len());
                        input.set_cursor_position(end, window, cx);
                    });
                }
                Action::Send => {
                    service
                        .send_followup(id.to_owned(), turn.to_owned(), index)
                        .map_err(|e| e.to_string())?;
                    // Direct suggestion sending leaves the user's unrelated composer draft intact.
                    super::super::attention::bind(id.to_owned(), window.window_handle());
                    self.scroll.scroll_to_bottom();
                }
            }
            Ok::<_, String>(())
        })();
        self.error = result.err();
        cx.notify();
    }
}
