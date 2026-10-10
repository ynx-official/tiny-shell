use gpui::{
    Div, FontWeight, InteractiveElement as _, ParentElement as _, Stateful,
    StatefulInteractiveElement as _, Styled as _, div, px, rems,
};
use gpui_component::tooltip::Tooltip;

/// Reserve the remaining toolbar width for the title itself, rather than a spacer.
pub(super) fn title(name: String) -> Stateful<Div> {
    let full_name = name.clone();
    div()
        .id("quick-command-category-title")
        .flex_1()
        .min_w(px(0.))
        .truncate()
        .text_size(rems(0.875))
        .font_weight(FontWeight::SEMIBOLD)
        .tooltip(move |window, cx| Tooltip::new(full_name.clone()).build(window, cx))
        .child(name)
}
