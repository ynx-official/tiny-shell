use gpui::{Context, Entity, Window};
use gpui_component::input::{InputState, RopeExt as _};

pub(crate) fn defer_focus_input_at_end<V: 'static>(
    input: Entity<InputState>,
    window: &mut Window,
    cx: &mut Context<V>,
) {
    window.defer(cx, move |window, cx| {
        input.update(cx, |input, cx| {
            let end = input.text().offset_to_position(input.text().len());
            input.set_cursor_position(end, window, cx);
        });
    });
}
