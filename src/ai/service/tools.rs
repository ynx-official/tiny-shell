use super::budget::{Budget, Decision, bounded_result};
use super::*;
use serde_json::{Value, json};
use tiny_shell_mcp::{contracts::Call, engine::InternalClient};

#[derive(Default)]
pub(super) struct Tools {
    session: Option<String>,
    pub(super) budget: Budget,
}
impl Tools {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            session: None,
            budget: Budget::new(limit),
        }
    }
}
struct Invocation<'a> {
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
        let decision = runtime.budget.begin(&name, &args);
        let mut slot = 0;
        self.mutate(&conversation.id, |c| {
            c.state = TurnState::RunningTool;
            slot = c.activities.len();
            c.activities.push(activity);
            Ok(())
        })?;
        let result = match decision {
            Decision::Reject(code) => Ok(json!({"state":"not_executed","error":code})),
            Decision::Cached(value) => Ok(value),
            Decision::Execute => {
                self.execute(
                    conversation,
                    client,
                    runtime,
                    Invocation {
                        name: &name,
                        arguments: args.clone(),
                        activity: slot,
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
                return Err(error);
            }
            Err(error) => json!({"error":error.to_string()}),
            Ok(value) => value,
        };
        let result = bounded_result(result);
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
            let opened = self.call_and_wait(id, client, Call::parse("open_session", json!({"connection_id":target.id,"request_id":uuid::Uuid::new_v4().to_string()})).map_err(|e| anyhow!(e.code))?, slot, true, cancel).await?;
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
        self.call_and_wait(id, client, call, slot, false, cancel)
            .await
    }

    async fn call_and_wait(
        &self,
        id: &str,
        client: &InternalClient,
        call: Call,
        slot: usize,
        opening: bool,
        cancel: &mut watch::Receiver<bool>,
    ) -> Result<Value> {
        ensure!(!*cancel.borrow(), "AI_CANCELLED");
        let result = tokio::select! {
            result = client.call(call) => result.map_err(|e| anyhow!(e.code))?,
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
