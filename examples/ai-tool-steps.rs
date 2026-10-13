//! Native production trace preview with synthetic data; no user profile or network access.
use gpui::{
    AppContext as _, Bounds, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, Window,
    WindowOptions, div, prelude::FluentBuilder as _, px, size,
};
use gpui_component::{
    ActiveTheme as _, IconName, Root, Sizable as _, Theme, ThemeMode,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::InputState,
    v_flex,
};
use gpui_component_assets::Assets;
use rust_i18n::t;
use serde_json::json;
use std::rc::Rc;

#[allow(dead_code)]
#[path = "../src/ai/model.rs"]
mod model;
#[allow(dead_code)]
#[path = "../src/ai/usage.rs"]
mod usage;
use model::Conversation;
use usage::{ModelRequest, TurnRecord};
mod ai {
    pub(crate) use super::model::*;
}
// The standalone preview uses Tools; full transcript messages are rendered by AiPanel.
#[allow(dead_code)]
#[path = "../src/app/ai/composer.rs"]
mod composer;
#[allow(dead_code)]
#[path = "../src/app/ai/tool_steps.rs"]
mod tool_steps;
use tool_steps::view::ToolGroupView;
use tool_steps::{ChatItem, ToolGroup, ToolState};
rust_i18n::i18n!("locales", fallback = "en");

struct Preview {
    complete: Entity<ToolGroupView>,
    awaiting: Entity<ToolGroupView>,
    failed: Entity<ToolGroupView>,
    dark: bool,
    english: bool,
    reviewed: bool,
    long_answer: bool,
    input: Entity<InputState>,
    pending_reply: bool,
}
fn fixture() -> ToolGroup {
    let mut c = ai::Conversation::new(None, &ai::Settings::default());
    for (id, name, args, output) in [
        (
            "status",
            "get_status",
            json!({}),
            json!({"mode":"full","enabled":true}),
        ),
        (
            "open",
            "open_session",
            json!({"connection_id":"fixture"}),
            json!({"state":"succeeded","result":{"session_id":"fixture-session"}}),
        ),
        (
            "run",
            "run_command",
            json!({"session_id":"fixture-session","command":"systemctl is-active nginx"}),
            json!({"state":"succeeded","result":{"stdout":"active\n","stderr":"","exit_code":0}}),
        ),
    ] {
        c.messages.push(ai::Message {
            role: "assistant".into(),
            tool_calls: vec![
                json!({"id":id,"function":{"name":name,"arguments":args.to_string()}}),
            ],
            ..ai::Message::default()
        });
        c.messages.push(ai::Message {
            role: "tool".into(),
            tool_call_id: Some(id.into()),
            content: output.to_string(),
            ..ai::Message::default()
        });
    }
    let Some(ChatItem::Tools(mut group)) = tool_steps::project(&c).into_iter().next() else {
        unreachable!("synthetic trace");
    };
    for step in &mut group.steps {
        step.target = Some("Example SSH".into());
    }
    group
}
impl Preview {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let owner = cx.entity().downgrade();
        let review = tool_steps::review::ReviewActions {
            load: Rc::new(|id| {
                Ok(tiny_shell_mcp::engine::Review {
                    id: id.into(),
                    client: "ai:fixture".into(),
                    target: tiny_shell_mcp::engine::Target {
                        id: "fixture".into(),
                        name: "Example SSH".into(),
                        group: None,
                        host: "example.invalid".into(),
                        port: 22,
                        user: "fixture".into(),
                        digest: "fixture".into(),
                    },
                    call: tiny_shell_mcp::contracts::Call::RunCommand(
                        tiny_shell_mcp::contracts::RunCommand {
                            session_id: "fixture-session".into(),
                            command: "systemctl is-active nginx".into(),
                            request_id: "fixture-request".into(),
                            cwd: Some("/tmp".into()),
                            timeout_ms: 30000,
                        },
                    ),
                    state: tiny_shell_mcp::tasks::State::AwaitingUser,
                    error: None,
                    preview: json!({}),
                })
            }),
            decide: Rc::new(move |_, _, _, cx| {
                if let Some(owner) = owner.upgrade() {
                    owner.update(cx, |this, cx| {
                        this.reviewed = true;
                        cx.notify();
                    });
                }
                Ok(())
            }),
            authenticate: Rc::new(|_, _, _| {}),
        };
        let complete = cx.new(|_| ToolGroupView::new(fixture(), review.clone()));
        let mut pending = fixture();
        pending.steps[2].state = ToolState::AwaitingApproval;
        pending.steps[2].result = None;
        pending.steps[2].task_id = Some("fixture-awaiting".into());
        let awaiting = cx.new(|_| ToolGroupView::new(pending, review.clone()));
        let mut error = fixture();
        error.steps[2].state = ToolState::Failed;
        error.steps[2].result = Some(json!({"error":"PERMISSION_DENIED"}).to_string());
        let failed = cx.new(|_| ToolGroupView::new(error, review));
        Self {
            complete,
            awaiting,
            failed,
            dark: false,
            english: false,
            reviewed: false,
            long_answer: false,
            input: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("ai_input_hint").to_string())
                    .auto_grow(3, 7)
            }),
            pending_reply: true,
        }
    }
}
impl Render for Preview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .text_sm()
            .child(
                h_flex()
                    .p_3()
                    .gap_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(div().flex_1().child(t!("ai_steps_preview").to_string()))
                    .child(Button::new("finish").small().outline().label(t!("ai_steps_preview_finish").to_string())
                        .on_click(cx.listener(|this,_,_,cx| {
                            this.complete.update(cx,|view,cx|view.update_group(fixture(),cx));
                            this.long_answer = true;
                            this.pending_reply = false;
                            cx.notify();
                        })))
                    .child(
                        Button::new("running")
                            .small()
                            .outline()
                            .label(t!("mcp_state_running").to_string())
                            .on_click(cx.listener(|this, _, _, cx| {
                                let mut group = fixture();
                                group.steps[2].state = ToolState::Running;
                                group.steps[2].result = None;
                                this.awaiting
                                    .update(cx, |view, cx| view.update_group(group, cx));
                            })),
                    )
                    .child(
                        Button::new("theme")
                            .small()
                            .outline()
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
                        Button::new("language")
                            .small()
                            .outline()
                            .label(t!("confirmation_preview_language").to_string())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.english = !this.english;
                                rust_i18n::set_locale(if this.english { "en" } else { "zh-CN" });
                                cx.notify();
                            })),
                    ),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_start()
                    .child(
                        v_flex().flex_1().min_w_0().h_full().min_h_0().child(v_flex()
                            .id("chat")
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .overflow_y_scroll()
                            .p_3()
                            .gap_3()
                            .child(
                                div()
                                    .p_2()
                                    .rounded(px(6.))
                                    .bg(cx.theme().muted)
                                    .child("当前启用了 nginx 吗？"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("ai_assistant").to_string()),
                            )
                            .child(div().child("我会检查 SSH 连接，然后确认 nginx 的运行状态。"))
                            .child(self.complete.clone())
                            .when(self.long_answer, |messages| messages.child(
                                gpui_component::text::TextView::markdown("preview-long-answer",
                                    (0..24).map(|_| "nginx 已启用并正在运行。主进程、工作进程和服务配置均正常。").collect::<Vec<_>>().join("\n\n"))
                            ))
                            .when(self.pending_reply, |messages|messages.children(composer::render_feedback(composer::TurnFeedback::Dots,cx)))
                            ).child(div().p_3().border_t_1().border_color(cx.theme().border).child(composer::surface(
                                &self.input,self.pending_reply,
                                h_flex().gap_2().child(div().text_xs().child("Example SSH")).child(div().text_xs().child(t!("mcp_full").to_string())),
                                Button::new("preview-model").ghost().xsmall().w_full().min_w_0()
                                    .child(div().w_full().min_w_0().overflow_hidden().text_ellipsis().text_left().child("Preview · fixture-model")),
                                Button::new("preview-send").small().ghost().icon(if self.pending_reply {IconName::Pause} else {IconName::ArrowUp})
                                    .on_click(cx.listener(|this,_,_,cx|{this.pending_reply = !this.pending_reply;cx.notify();})),cx))),
                    )
                    .child(
                        v_flex()
                            .id("states")
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_y_scroll()
                            .p_3()
                            .gap_3()
                            .border_l_1()
                            .border_color(cx.theme().border)
                            .child(self.awaiting.clone())
                            .child(self.failed.clone())
                            .child(div().text_xs().child(if self.reviewed {
                                t!("ai_steps_preview_reviewed").to_string()
                            } else {
                                String::new()
                            })),
                    ),
            )
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
                        size(px(860.), px(780.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("TinyShell AI Tool Steps Preview");
                    let view = cx.new(|cx| Preview::new(window, cx));
                    cx.new(|cx| Root::new(view, window, cx))
                },
            );
            if let Err(error) = result {
                eprintln!("Preview failed: {error}");
                cx.quit();
            }
        });
}
