use super::*;
use crate::audit::{Context as AuditContext, Event};

impl Task {
    pub(super) fn record(&self, kind: &str, state: &str, extra: Value) {
        let mut context = self.audit_context.clone();
        context.task_id = Some(self.review.id.clone());
        context.target_id = Some(self.review.target.id.clone());
        self.audit_store.record(Event::new(context,kind,tool_name(&self.review.call),state,json!({"call":self.review.call,"target":self.review.target,"target_digest":self.review.target.digest,"mode":self.mode,"policy_revision":self.revision,"approved":self.approved,"elapsed_ms":self.created.elapsed().as_millis(),"error":self.review.error,"preview":self.review.preview,"extra":extra})));
    }
}
pub(super) fn tool_name(call: &Call) -> &'static str {
    match call {
        Call::GetStatus(_) => "get_status",
        Call::ListConnections(_) => "list_connections",
        Call::OpenSession(_) => "open_session",
        Call::CloseSession(_) => "close_session",
        Call::RunCommand(_) => "run_command",
        Call::GetTask(_) => "get_task",
        Call::CancelTask(_) => "cancel_task",
        Call::ListDirectory(_) => "list_directory",
        Call::ReadFile(_) => "read_file",
        Call::WriteFile(_) => "write_file",
    }
}
impl Engine {
    pub(super) fn close_automatically(&self, id: String, session: Session, reason: &str) {
        let context = session.audit_context.child();
        let details = json!({"session_id":id,"reason":reason,"initiated_by":"system","target":session.target});
        let store = self.audit.clone();
        let remote = self.remote.clone();
        store.record(Event::new(
            context.clone(),
            "session_close_started",
            "close_session",
            "pending",
            details.clone(),
        ));
        tokio::spawn(async move {
            remote.close(&id).await;
            store.record(Event::new(
                context,
                "session_close_finished",
                "close_session",
                "completed",
                details,
            ));
        });
    }
    pub(super) async fn dispatch_traced(
        self: &Arc<Self>,
        client: &str,
        call: Call,
        epoch: Option<u64>,
        mut context: AuditContext,
    ) -> Result<Value> {
        context.client_id = Some(client.into());
        context.instance_id = Some(self.generation.clone());
        if let Call::GetTask(query) = &call {
            self.attach_query(&mut context, client, &query.task_id);
        }
        if let Call::CancelTask(query) = &call {
            self.attach_query(&mut context, client, &query.task_id);
        }
        let arguments = serde_json::to_value(&call).unwrap_or(Value::Null);
        let name = tool_name(&call);
        self.audit.record(Event::new(
            context.clone(),
            "request_received",
            name,
            "received",
            json!({"call":arguments}),
        ));
        let result = self
            .dispatch_operation(client, call, epoch, &mut context)
            .await;
        let state = match &result {
            Err(_) => "rejected",
            Ok(value) if value.get("task_id").is_some() => "queued",
            Ok(_) => "succeeded",
        };
        let details = if name == "get_task" {
            match &result {
                Ok(value) => {
                    json!({"task_id":value["task_id"],"state":value["state"],"offset":value["offset"],"next_offset":value["next_offset"],"result_reference":context.parent_id,"response_bytes":value.to_string().len()})
                }
                Err(error) => json!({"error":error}),
            }
        } else {
            json!({"call":arguments,"result":result.as_ref().ok(),"error":result.as_ref().err()})
        };
        self.audit
            .record(Event::new(context, "request_result", name, state, details));
        result
    }
    fn attach_query(&self, context: &mut AuditContext, client: &str, id: &str) {
        if let Ok(inner) = self.lock()
            && let Some(task) = inner.tasks.get(id).filter(|t| t.review.client == client)
        {
            let operation = context.operation_id.clone();
            *context = task.audit_context.child();
            context.operation_id = operation;
            context.task_id = Some(id.into());
            context.target_id = Some(task.review.target.id.clone());
        }
    }
    pub(super) async fn execution_record(
        &self,
        review: &Review,
        revision: u64,
        approved: bool,
    ) -> bool {
        let context = self
            .lock()
            .ok()
            .and_then(|inner| inner.tasks.get(&review.id).map(|t| t.audit_context.clone()));
        let Some(mut context) = context else {
            return false;
        };
        context.task_id = Some(review.id.clone());
        context.target_id = Some(review.target.id.clone());
        self.audit.durable(Event::new(context,"execution_started",tool_name(&review.call),"running",json!({"call":review.call,"target":review.target,"target_digest":review.target.digest,"approved":approved,"policy_revision":revision,"preview":review.preview}))).await
    }
}
