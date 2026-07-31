use gpui::{Entity, ParentElement as _, SharedString, Styled as _, div};
use gpui_component::{
    ActiveTheme as _, h_flex,
    input::{Input, InputState},
    v_flex,
};

pub(crate) fn labeled_input(
    label: impl Into<SharedString>,
    input: Entity<InputState>,
) -> gpui::Div {
    v_flex()
        .gap_1()
        .child(div().text_sm().child(label.into()))
        .child(Input::new(&input).w_full())
}

pub(crate) fn labeled_input_with_hint(
    label: impl Into<SharedString>,
    hint: impl Into<SharedString>,
    input: Entity<InputState>,
    cx: &gpui::App,
) -> gpui::Div {
    labeled_input(label, input).child(
        div()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(hint.into()),
    )
}

pub(crate) fn split_inputs(left: gpui::Div, right: gpui::Div) -> gpui::Div {
    h_flex().gap_2().child(left.flex_1()).child(right.flex_1())
}
