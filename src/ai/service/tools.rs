use super::budget::{Budget, Decision, bounded_result};
use super::*;
use serde_json::{Value, json};
use tiny_shell_mcp::{contracts::Call, engine::InternalClient};

#[derive(Default)]
pub(super) struct Tools {
    session: Option<String>,
    pub(super) budget: Budget,
    audit_calls: HashMap<String, String>,
}
impl Tools {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            session: None,
            budget: Budget::new(limit),
            audit_calls: HashMap::new(),
        }
    }
}
struct Invocation<'a> {
    context: tiny_shell_mcp::audit::Context,
    name: &'a str,
    arguments: Value,
    activity: usize,
}
impl Service {
    pub(super) async fn invoke(
        &self,
        conversation: &Conversation,
        client: &InternalClient,
        runtime: &mut Tools,
        activity: Activity,
        args: Value,
        cancel: &mut watch::Receiver<bool>,
    ) -> Result<Value> {
        let name = activity.name.clone();
        let mut context = self.audit_context(conversation);
        context.tool_call_id = activity.tool_call_id.clone();
        if let Ok(state) = self.state.lock()
            && let Some(live) = state
                .snapshot
                .conversations
                .iter()
                .find(|c| c.id == conversation.id)
            && let Some(message) = activity.message_index.and_then(|i| live.messages.get(i))
            && !message.content.is_empty()
        {
            context.purpose = Some(message.content.clone());
            context.purpose_source = Some("model_statement".into());
        }
        self.audit.record(tiny_shell_mcp::audit::Event::new(context.clone(),"tool_requested",&name,"received",json!({"arguments":args,"user_request":conversation.turns.last().and_then(|t|conversation.messages.get(t.user_message)).map(|m|&m.content)})));
        let decision = runtime.budget.begin(&name, &args);
        let fingerprint = format!(
            "{name}:{}",
            serde_json::to_string(&args).unwrap_or_default()
        );
        let cached = matches!(&decision, Decision::Cached(_));
        if cached && let Some(original) = runtime.audit_calls.get(&fingerprint) {
            context.parent_id = Some(original.clone());
        }
        if matches!(&decision, Decision::Execute) {
            runtime
                .audit_calls
                .insert(fingerprint, context.operation_id.clone());
        }
        let mut slot = 0;
        self.mutate(&conversation.id, |c| {
            c.state = TurnState::RunningTool;
            slot = c.activities.len();
            c.activities.push(activity);
            Ok(())
        })?;
        let result = match decision {
            Decision::Reject(code) => {
                self.audit.record(tiny_shell_mcp::audit::Event::new(
                    context.clone(),
                    "tool_not_executed",
                    &name,
                    "not_executed",
                    json!({"reason":code}),
                ));
                Ok(json!({"state":"not_executed","error":code}))
            }
            Decision::Cached(value) => {
                self.audit.record(tiny_shell_mcp::audit::Event::new(
                    context.clone(),
                    "tool_cached",
                    &name,
                    "cached",
                    json!({"arguments":args,"result":value}),
                ));
                Ok(value)
            }
            Decision::Execute => {
                self.execute(
                    conversation,
                    client,
                    runtime,
                    Invocation {
                        name: &name,
                        arguments: args.clone(),
                        activity: slot,
                        context: context.clone(),
                    },
                    cancel,
                )
                .await
            }
        };
        let result = match result {
            Err(error)
                if matches!(
                    error.to_string().as_str(),
                    "AI_CANCELLED" | "AI_ACCESS_CHANGED"
                ) =>
            {
                self.audit.record(tiny_shell_mcp::audit::Event::new(context.clone(),"tool_completed",&name,"interrupted",json!({"arguments":args,"error":error.to_string(),"remote_outcome_may_be_unknown":true})));
                return Err(error);
            }
            Err(error) => json!({"error":error.to_string()}),
            Ok(value) => value,
        };
        let result = bounded_result(result);
        self.audit.record(tiny_shell_mcp::audit::Event::new(
            context,
            "tool_completed",
            &name,
            if cached {
                "cached"
            } else {
                result["state"].as_str().unwrap_or("completed")
            },
            json!({"arguments":args,"result":result,"model_result_truncated":result["model_result_truncated"]}),
        ));
        runtime.budget.record(&name, &args, &result);
        self.mutate(&conversation.id, |c| {
            if let Some(activity) = c.activities.get_mut(slot) {
                activity.state = result["state"]
                    .as_str()
                    .unwrap_or_else(|| {
                        if result.get("error").is_some_and(|error| !error.is_null()) {
                            "failed"
                        } else {
                            "succeeded"
                        }
                    })
                    .to_owned();
            }
            Ok(())
        })?;
        Ok(result)
    }

    async fn execute(
        &self,
        conversation: &Conversation,
        client: &InternalClient,
        runtime: &mut Tools,
        invocation: Invocation<'_>,
        cancel: &mut watch::Receiver<bool>,
    ) -> Result<Value> {
        let id = &conversation.id;
        let slot = invocation.activity;
        let mode = client.mode().map_err(|_| anyhow!("AI_ACCESS_CHANGED"))?;
        let mut call = super::toolset::parse(invocation.name, invocation.arguments, mode)
            .map_err(|e| anyhow!(e.code))?;
        if runtime.session.is_none() {
            let target = conversation
                .target
                .as_ref()
                .ok_or_else(|| anyhow!("AI_TOOLS_DISABLED"))?;
            let opened = self.call_and_wait(id, client, Call::parse("open_session", json!({"connection_id":target.id,"request_id":uuid::Uuid::new_v4().to_string()})).map_err(|e| anyhow!(e.code))?, slot, invocation.context.child(),cancel).await?;
            if opened["state"] != "succeeded" {
                return Ok(opened);
            }
            runtime.session = Some(
                opened["result"]["session_id"]
                    .as_str()
                    .ok_or_else(|| anyhow!("AI_INVALID_TOOL"))?
                    .into(),
            );
        }
        let session = runtime
            .session
            .as_deref()
            .ok_or_else(|| anyhow!("AI_INVALID_TOOL"))?;
        super::toolset::bind(&mut call, session);
        self.call_and_wait(id, client, call, slot, invocation.context.child(), cancel)
            .await
    }

    async fn call_and_wait(
        &self,
        id: &str,
        client: &InternalClient,
        call: Call,
        slot: usize,
        context: tiny_shell_mcp::audit::Context,
        cancel: &mut watch::Receiver<bool>,
    ) -> Result<Value> {
        let opening = matches!(call, Call::OpenSession(_));
        ensure!(!*cancel.borrow(), "AI_CANCELLED");
        let result = tokio::select! {
            result = client.call_traced(call,context) => result.map_err(|e| anyhow!(e.code))?,
            _ = cancel.changed() => return Err(anyhow!("AI_CANCELLED")),
            _ = client.revoked() => return Err(anyhow!("AI_ACCESS_CHANGED")),
        };
        let Some(task) = result["task_id"].as_str() else {
            return Ok(result);
        };
        self.mutate(id, |c| {
            if let Some(activity) = c.activities.get_mut(slot) {
                activity.task_id = Some(task.into());
                activity.state = "running".into();
            }
            Ok(())
        })?;
        let mut previous = String::new();
        loop {
            ensure!(!*cancel.borrow(), "AI_CANCELLED");
            let query = Call::parse(
                "get_task",
                json!({"task_id":task,"wait_ms":if previous.is_empty() {0} else {10000}}),
            )
            .map_err(|e| anyhow!(e.code))?;
            let result = tokio::select! {
                result = client.call(query) => result.map_err(|e| anyhow!(e.code))?,
                _ = cancel.changed() => return Err(anyhow!("AI_CANCELLED")),
                _ = client.revoked() => return Err(anyhow!("AI_ACCESS_CHANGED")),
            };
            let status = result["state"].as_str().unwrap_or("failed");
            if previous != status {
                self.mutate(id, |c| {
                    c.state = if status == "awaiting_user" {
                        TurnState::AwaitingApproval
                    } else {
                        TurnState::RunningTool
                    };
                    if let Some(activity) = c.activities.get_mut(slot) {
                        activity.state = if opening && status == "succeeded" {
                            "running"
                        } else {
                            status
                        }
                        .into();
                    }
                    Ok(())
                })?;
                previous = status.into();
            }
            if !matches!(
                status,
                "queued" | "running" | "awaiting_user" | "cancel_requested"
            ) {
                return Ok(result);
            }
        }
    }
}
