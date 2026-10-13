//! Production audit UI with synthetic, isolated data. No app services, SSH or provider requests.
use super::*;
use gpui::{Bounds, WindowBounds, WindowOptions};
use gpui_component::{Theme, ThemeMode};
use serde_json::json;
use tiny_shell_mcp::audit::{Context as AuditContext, Event, ModelRequest, Usage, now_ms};

pub(crate) fn run() {
    let directory =
        std::env::temp_dir().join(format!("tiny-shell-audit-preview-{}", uuid::Uuid::new_v4()));
    let store = Store::with_key(directory, [71; 32]);
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("Audit preview runtime failed: {error}");
            return;
        }
    };
    let target = uuid::Uuid::new_v4().to_string();
    let conversation = uuid::Uuid::new_v4().to_string();
    if !runtime.block_on(seed(&store, &target, &conversation)) {
        eprintln!("Audit preview data could not be persisted");
        return;
    }
    rust_i18n::set_locale(if std::env::args().any(|arg| arg == "--english") {
        "en"
    } else {
        "zh-CN"
    });
    let dark = std::env::args().any(|arg| arg == "--dark");
    let compact = std::env::args().any(|arg| arg == "--compact");
    gpui_platform::application()
        .with_assets(crate::app::assets::Assets)
        .with_quit_mode(gpui::QuitMode::LastWindowClosed)
        .run(move |cx| {
            gpui_component::init(cx);
            Theme::change(
                if dark {
                    ThemeMode::Dark
                } else {
                    ThemeMode::Light
                },
                None,
                cx,
            );
            // Match the production UI default so the isolated preview uses the same layout breakpoints.
            Theme::global_mut(cx).font_size =
                px(crate::session::config_file::default_ui_font_size());
            let dimensions = if compact {
                size(px(760.), px(600.))
            } else {
                size(px(1040.), px(760.))
            };
            let opened = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None, dimensions, cx,
                    ))),
                    ..Default::default()
                },
                move |window, cx| {
                    window.set_window_title("TinyShell Audit Preview — synthetic data");
                    let panel = cx.new(|cx| {
                        AuditPanel::with_scope(
                            store,
                            Filter::default(),
                            None,
                            vec![(target, "Example SSH".into())],
                            window,
                            cx,
                        )
                    });
                    cx.new(|cx| Root::new(panel, window, cx))
                },
            );
            if let Err(error) = opened {
                eprintln!("Audit preview window failed: {error}");
                cx.quit();
            }
        });
}
async fn seed(store: &Arc<Store>, target: &str, conversation: &str) -> bool {
    let mut base = AuditContext::client(&format!("ai:{conversation}"));
    base.target_id = Some(target.into());
    base.target_name = Some("Example SSH".into());
    base.turn_id = Some(uuid::Uuid::new_v4().to_string());
    for index in 0..2 {
        let mut context = base.child();
        context.model_request_id = Some(context.operation_id.clone());
        let request = ModelRequest {
            purpose: tiny_shell_mcp::audit::RequestPurpose::Chat,
            id: context.operation_id.clone(),
            conversation_id: Some(conversation.into()),
            turn_id: base.turn_id.clone().unwrap_or_default(),
            provider_id: "preview-provider".into(),
            provider_name: "Example Provider".into(),
            model: "example-chat-model".into(),
            conversation_title: "Synthetic service inspection".into(),
            response_model: Some("example-chat-model".into()),
            provider_request_id: Some(format!("preview-{index}")),
            http_status: Some(200),
            started_ms: now_ms() - 1500,
            first_byte_ms: Some(now_ms() - 1200),
            finished_ms: Some(now_ms()),
            state: if index == 0 { "completed" } else { "cancelled" }.into(),
            usage: if index == 0 {
                Usage::parse(
                    &json!({"prompt_tokens":300,"completion_tokens":40,"total_tokens":340,"prompt_tokens_details":{"cached_tokens":120},"completion_tokens_details":{"reasoning_tokens":8}}),
                )
            } else {
                None
            },
            error: if index == 0 {
                None
            } else {
                Some("AI_CANCELLED".into())
            },
        };
        if !store
            .durable(Event::new(
                context,
                "model_finished",
                "model",
                &request.state,
                json!({"request":request}),
            ))
            .await
        {
            return false;
        }
    }
    let mut command = base.child();
    command.purpose = Some("Inspect the service status; synthetic fixture".into());
    command.purpose_source = Some("model_statement".into());
    command.task_id = Some(uuid::Uuid::new_v4().to_string());
    let call = json!({"tool":"run_command","arguments":{"command":"systemctl is-active example-service","session_id":"preview-session","password":"synthetic-secret"}});
    for (kind, state, extra) in [
        ("request_received", "received", json!({})),
        (
            "approval_requested",
            "awaiting_user",
            json!({"approval_kind":"operation_permission","mode":"controlled"}),
        ),
        (
            "approval_approved",
            "approved",
            json!({"actor":"local_user","channel":"application"}),
        ),
        (
            "execution_result",
            "succeeded",
            json!({"result":{"stdout":format!("active\n{}","Synthetic output line\n".repeat(160)),"stderr":"","exit_code":0,"truncated":false}}),
        ),
    ] {
        if !store.durable(Event::new(command.clone(),kind,"run_command",state,json!({"call":call,"target":{"id":target,"name":"Example SSH"},"extra":extra,"approved":matches!(state,"approved" | "succeeded")}))).await{return false;}
    }
    let file = base.child();
    if !store.durable(Event::new(file,"execution_result","write_file","succeeded",json!({"call":{"arguments":{"path":"/tmp/example.conf","mode":"update","expected_version":"synthetic-old-version"}},"preview":{"old_content":"mode=old\npassword=synthetic-old-secret\n","new_content":"mode=new\npassword=synthetic-new-secret\n"},"extra":{"result":{"version":"synthetic-new-version","bytes":20}}}))).await{return false;}
    let mut external = AuditContext::new(Source::External);
    external.client_id = Some(uuid::Uuid::new_v4().to_string());
    external.client_name = Some("Example MCP Client".into());
    external.target_id = Some(target.into());
    external.target_name = Some("Example SSH".into());
    if !store
        .durable(Event::new(
            external,
            "request_result",
            "read_file",
            "succeeded",
            json!({"arguments":{"path":"/tmp/example.conf"},"result":{"content":"mode=new\n"}}),
        ))
        .await
    {
        return false;
    }
    let budget = base.child();
    store.durable(Event::new(budget,"budget_rejected","budget","rejected",json!({"approval_kind":"tool_budget","actor":"local_user","channel":"system_notification","used":16,"allowance":16,"decision":"terminate_turn"}))).await
}
