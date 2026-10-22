use super::*;
use gpui::{
    AppContext as _, Context, IntoElement, ParentElement as _, Render, Styled as _, TestAppContext,
    div, px, size,
};
use gpui_component::Root;

struct Surface;
impl Render for Surface {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .size_full()
            .children(Root::render_notification_layer(window, cx))
    }
}

#[gpui::test]
fn explicit_feedback_and_copy_stay_in_the_origin_window(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let first = cx.open_window(size(px(640.), px(480.)), |window, cx| {
        let view = cx.new(|_| Surface);
        Root::new(view, window, cx)
    });
    let second = cx.open_window(size(px(640.), px(480.)), |window, cx| {
        let view = cx.new(|_| Surface);
        Root::new(view, window, cx)
    });
    // Update the window without borrowing Root: pushing a notification updates Root itself.
    gpui::AnyWindowHandle::from(first)
        .update(cx, |_, window, cx| {
            Feedback::success(window, cx, "saved");
            Feedback::warning(window, cx, "input required");
            Feedback::error(window, cx, "write failed");
            Feedback::info(window, cx, "ready");
            Feedback::copy(window, cx, "clipboard fixture".into());
            assert_eq!(window.notifications(cx).len(), 5);
            assert_eq!(
                cx.read_from_clipboard()
                    .and_then(|item| item.text())
                    .as_deref(),
                Some("clipboard fixture")
            );
        })
        .unwrap();
    gpui::AnyWindowHandle::from(second)
        .update(cx, |_, window, cx| {
            assert!(window.notifications(cx).is_empty())
        })
        .unwrap();
}
