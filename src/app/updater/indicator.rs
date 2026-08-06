use std::time::Duration;

use gpui::{
    Animation, AnimationExt as _, Hsla, IntoElement, ParentElement as _, Styled as _, div,
    ease_out_quint, px,
};
use gpui_component::{Icon, IconName, Sizable as _, h_flex};

const DEFAULT_CONTAINER_SIZE: f32 = 36.0;
const DEFAULT_ICON_SIZE: f32 = 24.0;
const COMPACT_CONTAINER_SIZE: f32 = 20.0;
const COMPACT_ICON_SIZE: f32 = 12.0;
const ACTIVE_PORTION: f32 = 0.86;

pub(crate) fn pulse_icon(animation_id: &'static str, color: Hsla) -> impl IntoElement {
    pulse_icon_with_size(
        animation_id,
        color,
        DEFAULT_CONTAINER_SIZE,
        DEFAULT_ICON_SIZE,
    )
}

pub(crate) fn compact_pulse_icon(animation_id: &'static str, color: Hsla) -> impl IntoElement {
    pulse_icon_with_size(
        animation_id,
        color,
        COMPACT_CONTAINER_SIZE,
        COMPACT_ICON_SIZE,
    )
}

fn pulse_icon_with_size(
    animation_id: &'static str,
    color: Hsla,
    container_size: f32,
    icon_size: f32,
) -> impl IntoElement {
    h_flex()
        .relative()
        .flex_none()
        .size(px(container_size))
        .items_center()
        .justify_center()
        .child(
            div()
                .absolute()
                .rounded_full()
                .border_1()
                .border_color(color)
                .with_animation(
                    animation_id,
                    Animation::new(Duration::from_millis(1_400)).repeat(),
                    move |ring, delta| {
                        let progress = (delta / ACTIVE_PORTION).min(1.0);
                        let eased = ease_out_quint()(progress);
                        let size = icon_size + (container_size - icon_size) * eased;
                        let offset = (container_size - size) / 2.0;

                        ring.size(px(size))
                            .left(px(offset))
                            .top(px(offset))
                            .opacity((1.0 - progress) * 0.55)
                    },
                ),
        )
        .child(
            h_flex()
                .absolute()
                .size(px(icon_size))
                .items_center()
                .justify_center()
                .rounded_full()
                .border_1()
                .border_color(color)
                .text_color(color)
                .child(Icon::new(IconName::ArrowDown).xsmall()),
        )
}
