use super::*;
use crate::contracts::GetTask;

impl Engine {
    fn task_snapshot(&self, client: &str, query: &GetTask, epoch: Option<u64>) -> Result<Value> {
        if !self.client_enabled(client) {
            return Err(Failure::new("CLIENT_REVOKED"));
        }
        let inner = self.lock()?;
        self.verify_internal_epoch(client, epoch)?;
        if !self.effective_policy(client, &inner.policy)?.enabled {
            return Err(Failure::new("MCP_DISABLED"));
        }
        let task = inner
            .tasks
            .get(&query.task_id)
            .filter(|task| task.review.client == client)
            .ok_or_else(|| Failure::new("TASK_NOT_FOUND"))?;
        let mut result = task.result.clone();
        let stdout = result.get("stdout").and_then(Value::as_str).unwrap_or("");
        let stderr = result.get("stderr").and_then(Value::as_str).unwrap_or("");
        let all = format!("{stdout}{stderr}");
        let start = all.ceil_char_boundary(query.offset.min(all.len()));
        let end = all.floor_char_boundary(start.saturating_add(query.limit).min(all.len()));
        let mut output = Vec::new();
        if start < stdout.len() {
            output.push(json!({"stream":"stdout","text":&all[start..end.min(stdout.len())]}));
        }
        if end > stdout.len() {
            output.push(json!({"stream":"stderr","text":&all[start.max(stdout.len())..end]}));
        }
        if let Some(object) = result.as_object_mut() {
            object.remove("stdout");
            object.remove("stderr");
        }
        Ok(
            json!({"id":query.task_id,"state":task.review.state,"error":task.review.error,"result":result,"output":output,"next_offset":(end<all.len()).then_some(end),"truncated":task.result.get("truncated").and_then(Value::as_bool).unwrap_or(false)}),
        )
    }

    pub(super) async fn wait_task(
        &self,
        client: &str,
        query: &GetTask,
        epoch: Option<u64>,
    ) -> Result<Value> {
        // Subscribe before reading so a completion between snapshot and await cannot be lost.
        let mut changes = self.changes.subscribe();
        self.maintain()?;
        let initial = self.task_snapshot(client, query, epoch)?;
        if query.wait_ms == 0 || terminal(&initial) {
            return Ok(initial);
        }
        let deadline = tokio::time::Instant::now() + Duration::from_millis(query.wait_ms);
        loop {
            let timed_out = tokio::time::timeout_at(deadline, changes.changed())
                .await
                .is_err();
            self.maintain()?;
            let current = self.task_snapshot(client, query, epoch)?;
            if timed_out || current["state"] != initial["state"] || terminal(&current) {
                return Ok(current);
            }
        }
    }
}
fn terminal(value: &Value) -> bool {
    !matches!(
        value["state"].as_str(),
        Some("queued" | "running" | "awaiting_user" | "cancel_requested")
    )
}
