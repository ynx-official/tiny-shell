use std::time::Duration;

use gpui::{
    Animation, AnimationExt as _, Hsla, IntoElement, ParentElement as _, Styled as _, div,
    ease_out_quint, px,
};
use gpui_component::{Icon, IconName, Sizable as _, h_flex};

const CONTAINER_SIZE: f32 = 36.0;
const ICON_SIZE: f32 = 24.0;
const ACTIVE_PORTION: f32 = 0.86;

pub(crate) fn pulse_icon(animation_id: &'static str, color: Hsla) -> impl IntoElement {
    h_flex()
        .relative()
        .flex_none()
        .size(px(CONTAINER_SIZE))
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
                        let size = ICON_SIZE + (CONTAINER_SIZE - ICON_SIZE) * eased;
                        let offset = (CONTAINER_SIZE - size) / 2.0;

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
                .size(px(ICON_SIZE))
                .items_center()
                .justify_center()
                .rounded_full()
                .border_1()
                .border_color(color)
                .text_color(color)
                .child(Icon::new(IconName::ArrowDown).xsmall()),
        )
}
