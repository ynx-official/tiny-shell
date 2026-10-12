//! Run with `cargo run --locked --example confirmation-dialog`.
//! Uses the production component without loading user settings, sessions or remote services.

use gpui::{
    AppContext as _, Bounds, Context, IntoElement, ParentElement as _, Render, Styled as _, Window,
    WindowOptions, div, px, size,
};
use gpui_component::{
    ActiveTheme as _, Root, Theme, ThemeMode, WindowExt as _, button::Button, h_flex, v_flex,
};
use gpui_component_assets::Assets;
use rust_i18n::t;

#[path = "../src/app/confirmation_dialog.rs"]
mod confirmation_dialog;
// The production layout module also exposes sizing for the full updater dialog.
#[allow(dead_code)]
#[path = "../src/app/dialog_layout.rs"]
mod dialog_layout;

rust_i18n::i18n!("locales", fallback = "en");

#[derive(Clone, Copy)]
enum PreviewKind {
    Short,
    Long,
    Neutral,
    Closing,
}

struct Preview {
    confirmations: usize,
    status: String,
    dark: bool,
    large: bool,
    english: bool,
}

impl Preview {
    fn show(&mut self, kind: PreviewKind, window: &mut Window, cx: &mut Context<Self>) {
        let long = matches!(kind, PreviewKind::Long);
        let closing = matches!(kind, PreviewKind::Closing);
        let owner = cx.entity();
        let description = if long {
            format!(
                "{}\n\n{}",
                t!("confirm_delete_desc", count = 24),
                (1..=24)
                    .map(|i| format!("/example/project/archive/item-{i:02}-long-file-name.txt"))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        } else {
            t!("close_window_active_connections", count = 2).to_string()
        };
        let mut dialog = confirmation_dialog::ConfirmationDialog::new(
            if long {
                t!("confirm_delete").to_string()
            } else {
                t!("close_window_confirm_title").to_string()
            },
            description,
        )
        .danger(!matches!(kind, PreviewKind::Neutral))
        .keyboard(!long)
        .confirm_label(if long {
            t!("delete").to_string()
        } else {
            t!("close_window_confirm").to_string()
        })
        .cancel_label(t!("cancel").to_string());
        if closing {
            dialog = dialog.pending_on_confirm(
                t!("close_window_closing_title").to_string(),
                t!("close_window_closing_desc").to_string(),
                t!("close_window_closing").to_string(),
            );
        }
        dialog
            .on_ok({
                let owner = owner.clone();
                move |_, window, cx| {
                    owner.update(cx, |this, cx| {
                        this.confirmations += 1;
                        this.status =
                            t!("confirmation_preview_confirmed", count = this.confirmations)
                                .to_string();
                        cx.notify();
                    });
                    if closing {
                        let handle = window.window_handle();
                        // Simulate asynchronous completion without closing a user's real workspace.
                        cx.spawn(async move |cx| {
                            cx.background_executor()
                                .timer(std::time::Duration::from_secs(10))
                                .await;
                            let _ = handle.update(cx, |_, window, cx| window.close_dialog(cx));
                        })
                        .detach();
                    }
                    true
                }
            })
            .on_cancel({
                let owner = owner.clone();
                move |_, _, cx| {
                    owner.update(cx, |this, cx| {
                        this.status = t!("confirmation_preview_cancelled").to_string();
                        cx.notify();
                    });
                    true
                }
            })
            .on_close(move |_, _, cx| owner.update(cx, |_, cx| cx.notify()))
            .open(window, cx);
    }
}

impl Render for Preview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .p_6()
            .gap_4()
            .child(
                div()
                    .text_xl()
                    .child(t!("confirmation_preview_title").to_string()),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        Button::new("short")
                            .label(t!("close_window_confirm").to_string())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.show(PreviewKind::Short, window, cx)
                            })),
                    )
                    .child(
                        Button::new("long")
                            .label(t!("confirmation_preview_long").to_string())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.show(PreviewKind::Long, window, cx)
                            })),
                    )
                    .child(
                        Button::new("neutral")
                            .label(t!("confirmation_preview_neutral").to_string())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.show(PreviewKind::Neutral, window, cx)
                            })),
                    )
                    .child(
                        Button::new("closing")
                            .label(t!("confirmation_preview_closing").to_string())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.show(PreviewKind::Closing, window, cx)
                            })),
                    )
                    .child(
                        Button::new("theme")
                            .label(t!("confirmation_preview_theme").to_string())
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
                        Button::new("font")
                            .label(t!("confirmation_preview_font").to_string())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.large = !this.large;
                                Theme::global_mut(cx).font_size =
                                    px(if this.large { 21. } else { 14. });
                                window.refresh();
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("language")
                            .label(t!("confirmation_preview_language").to_string())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.english = !this.english;
                                rust_i18n::set_locale(if this.english { "en" } else { "zh-CN" });
                                cx.notify();
                            })),
                    ),
            )
            .child(div().text_sm().child(self.status.clone()))
            .children(Root::render_dialog_layer(window, cx))
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
            let result = cx.open_window(
                WindowOptions {
                    window_bounds: Some(gpui::WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1000.), px(680.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("TinyShell Confirmation Preview");
                    let view = cx.new(|_| Preview {
                        confirmations: 0,
                        status: t!("confirmation_preview_ready").to_string(),
                        dark: false,
                        large: false,
                        english: false,
                    });
                    let owner = view.clone();
                    window.defer(cx, move |window, cx| {
                        owner.update(cx, |this, cx| this.show(PreviewKind::Closing, window, cx))
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
