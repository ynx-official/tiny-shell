//! Native regression preview using the production title and localized input lifecycle.
//! Run with `cargo run --locked --example quick-command-header`.
use gpui::{
    AppContext as _, Bounds, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    Window, WindowOptions, div, px, size,
};
use gpui_component::{
    ActiveTheme as _, ElementExt as _, Icon, IconName, Root, Sizable as _, Theme, ThemeMode,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputState},
    v_flex,
};
use gpui_component_assets::Assets;
use rust_i18n::t;

#[path = "../src/app/localization.rs"]
mod localization;
#[path = "../src/app/ui/quick_command_header.rs"]
mod quick_command_header;

rust_i18n::i18n!("locales", fallback = "en");

struct Preview {
    search: Entity<InputState>,
    draft: Entity<InputState>,
    english: bool,
    narrow: bool,
    dark: bool,
}

impl Render for Preview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = if self.english {
            "Search & Text"
        } else {
            "查找与文本"
        };
        v_flex()
            .size_full()
            .p_4()
            .gap_4()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("language")
                            .label("中文 / English")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.english = !this.english;
                                localization::set_display_locale(
                                    if this.english { "en" } else { "zh-CN" },
                                    cx,
                                );
                                assert_eq!(this.draft.read(cx).value().as_str(), "keep-this-draft");
                                cx.refresh_windows();
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("width")
                            .label("440 / 800 px")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.narrow = !this.narrow;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("theme")
                            .label("Light / Dark")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.dark = !this.dark;
                                Theme::change(
                                    if this.dark {
                                        ThemeMode::Dark
                                    } else {
                                        ThemeMode::Light
                                    },
                                    Some(window),
                                    cx,
                                );
                                cx.notify();
                            })),
                    ),
            )
            .child(
                h_flex()
                    .w(px(if self.narrow { 440. } else { 800. }))
                    .h(px(40.))
                    .flex_none()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(quick_command_header::title(title.into()).on_prepaint(
                        move |bounds, _, _| {
                            // Both supported labels must fit even in the narrow toolbar.
                            assert!(
                                f32::from(bounds.size.width) >= 100.,
                                "Title was squeezed: {bounds:?}"
                            );
                            eprintln!(
                                "Title width: {:.1}px ({title})",
                                f32::from(bounds.size.width)
                            );
                        },
                    ))
                    .child(
                        div()
                            .flex_none()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("quick_command_count", count = 19).to_string()),
                    )
                    .child(
                        div().w(px(180.)).min_w(px(120.)).child(
                            Input::new(&self.search)
                                .small()
                                .prefix(Icon::new(IconName::Search).small()),
                        ),
                    )
                    .child(
                        Button::new("manage")
                            .ghost()
                            .small()
                            .icon(IconName::Settings),
                    ),
            )
            .child(Input::new(&self.draft).w(px(300.)))
    }
}

fn main() {
    gpui_platform::application()
        .with_assets(Assets)
        .with_quit_mode(gpui::QuitMode::LastWindowClosed)
        .run(|cx| {
            gpui_component::init(cx);
            Theme::change(ThemeMode::Light, None, cx);
            Theme::global_mut(cx).font_size = px(14.);
            localization::set_display_locale("zh-CN", cx);
            let result = cx.open_window(
                WindowOptions {
                    window_bounds: Some(gpui::WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(860.), px(260.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("Quick Command Header Check");
                    let view = cx.new(|cx| Preview {
                        search: cx.new(|cx| {
                            localization::localized_input(window, cx, || {
                                t!("quick_command_search").to_string()
                            })
                        }),
                        draft: cx.new(|cx| {
                            localization::localized_input(window, cx, || t!("host").to_string())
                                .default_value("keep-this-draft")
                        }),
                        english: false,
                        narrow: false,
                        dark: false,
                    });
                    cx.new(|cx| Root::new(view, window, cx))
                },
            );
            if let Err(error) = result {
                eprintln!("Preview failed: {error}");
                cx.quit();
            }
        });
}
