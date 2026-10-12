//! Isolated native preview of the production MCP mode cards. No service or user config is loaded.
use gpui::{
    AppContext as _, Bounds, Context, IntoElement, ParentElement as _, Render, Styled as _, Window,
    WindowOptions, div, px, size,
};
use gpui_component::{ActiveTheme as _, Root, Theme, ThemeMode, button::Button, h_flex, v_flex};
use gpui_component_assets::Assets;
use rust_i18n::t;
use tiny_shell_mcp::policy::Mode;

#[path = "../src/app/settings/mcp/permissions.rs"]
mod permissions;
rust_i18n::i18n!("locales", fallback = "en");

struct Preview {
    mode: Option<Mode>,
    dark: bool,
    large: bool,
    english: bool,
    narrow: bool,
    compact: bool,
}
impl Render for Preview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let owner = cx.entity();
        v_flex()
            .size_full()
            .p_4()
            .gap_4()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        Button::new("theme")
                            .label(t!("mcp_preview_theme").to_string())
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
                    )
                    .child(
                        Button::new("language")
                            .label(t!("mcp_preview_language").to_string())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.english = !this.english;
                                rust_i18n::set_locale(if this.english { "en" } else { "zh-CN" });
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("font")
                            .label(t!("mcp_preview_font").to_string())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.large = !this.large;
                                Theme::global_mut(cx).font_size =
                                    px(if this.large { 21. } else { 14. });
                                window.set_rem_size(px(if this.large { 21. } else { 14. }));
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("size")
                            .label(t!("mcp_preview_size").to_string())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.narrow = !this.narrow;
                                window.resize(size(
                                    px(if this.narrow { 720. } else { 980. }),
                                    px(if this.narrow { 520. } else { 420. }),
                                ));
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("disabled")
                            .label(t!("mcp_preview_disabled").to_string())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.mode = if this.mode.is_some() {
                                    None
                                } else {
                                    Some(Mode::ReadOnly)
                                };
                                cx.notify();
                            })),
                    ),
            )
            .child(div().text_sm().child(t!("mcp_modes").to_string()))
            .child(
                div()
                    .w_full()
                    .max_w(px(if self.compact { 710. } else { 2000. }))
                    .child(permissions::selector(
                        self.mode,
                        window.viewport_size().width < px(850.) || self.large,
                        window,
                        cx,
                        move |mode, _, cx| {
                            owner.update(cx, |this, cx| {
                                this.mode = Some(mode);
                                cx.notify();
                            })
                        },
                    )),
            )
            .child(div().text_xs().child(t!("mcp_preview_note").to_string()))
    }
}

fn main() {
    rust_i18n::set_locale("zh-CN");
    gpui_platform::application()
        .with_assets(Assets)
        .with_quit_mode(gpui::QuitMode::LastWindowClosed)
        .run(|cx| {
            gpui_component::init(cx);
            Theme::change(ThemeMode::Light, None, cx);
            Theme::global_mut(cx).font_size = px(14.);
            let opened = cx.open_window(
                WindowOptions {
                    window_bounds: Some(gpui::WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(980.), px(420.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("TinyShell MCP Permission Preview");
                    let compact = std::env::args().any(|arg| arg == "--compact");
                    let view = cx.new(|_| Preview {
                        mode: Some(Mode::ReadOnly),
                        dark: false,
                        large: false,
                        english: false,
                        narrow: false,
                        compact,
                    });
                    cx.new(|cx| Root::new(view, window, cx))
                },
            );
            if let Err(error) = opened {
                eprintln!("MCP preview failed: {error}");
                cx.quit();
            }
        });
}
