use super::*;
use crate::ai::usage::Metadata;
use tiny_shell_mcp::audit::{Context as AuditContext, Event, Source, now_ms};

impl Service {
    pub(super) async fn test_provider_request(
        self: &Arc<Self>,
        provider: &Provider,
        key: Option<String>,
    ) -> Result<()> {
        let mut context = AuditContext::new(Source::Local);
        context.instance_id = self.engine.as_ref().map(|e| e.generation.clone());
        context.model_request_id = Some(context.operation_id.clone());
        let mut request = ModelRequest {
            id: context.operation_id.clone(),
            conversation_id: None,
            turn_id: String::new(),
            purpose: RequestPurpose::ProviderTest,
            provider_id: provider.id.clone(),
            provider_name: provider.name.clone(),
            model: provider.default_model.clone(),
            conversation_title: String::new(),
            response_model: None,
            provider_request_id: None,
            http_status: None,
            started_ms: now_ms(),
            first_byte_ms: None,
            finished_ms: None,
            state: "pending".into(),
            usage: None,
            error: None,
        };
        self.audit.record(Event::new(context.clone(),"model_started","model","pending",serde_json::json!({"request":request,"purpose":"provider_connection_test","max_tokens":8})));
        let result = async {
            let key = self.request_key(provider, key).await?;
            crate::ai::provider::test(provider, &key, |metadata| {
                if let Some(usage) = metadata.usage {
                    request
                        .usage
                        .get_or_insert_with(Default::default)
                        .merge(usage);
                }
                if metadata.http_status.is_some() {
                    request.http_status = metadata.http_status;
                }
                if metadata.provider_request_id.is_some() {
                    request.provider_request_id = metadata.provider_request_id;
                }
                if metadata.response_model.is_some() {
                    request.response_model = metadata.response_model;
                }
            })
            .await
        }
        .await;
        request.finished_ms = Some(now_ms());
        request.state = if result.is_ok() {
            "completed"
        } else {
            "failed"
        }
        .into();
        request.error = result.as_ref().err().map(ToString::to_string);
        self.audit.record(Event::new(context,"model_finished","model",&request.state,serde_json::json!({"request":request,"purpose":"provider_connection_test","conversation_tokens_excluded":true})));
        result
    }
    pub(super) fn audit_context(&self, c: &Conversation) -> AuditContext {
        let mut context = AuditContext::client(&format!("ai:{}", c.id));
        context.turn_id = Some(c.turn_id());
        context.target_id = c.target.as_ref().map(|t| t.id.clone());
        context.target_name = c.target.as_ref().map(|t| t.name.clone());
        context.instance_id = self.engine.as_ref().map(|e| e.generation.clone());
        if let Ok(state) = self.state.lock()
            && let Some(live) = state.snapshot.conversations.iter().find(|v| v.id == c.id)
        {
            context.model_request_id = live
                .requests
                .iter()
                .rev()
                .find(|r| r.turn_id == c.turn_id() && r.purpose == RequestPurpose::Chat)
                .map(|r| r.id.clone());
        }
        context
    }
    pub(super) fn request_event(&self, c: &Conversation, request: &ModelRequest, kind: &str) {
        let mut context = self.audit_context(c);
        context.operation_id = request.id.clone();
        context.model_request_id = Some(request.id.clone());
        context.turn_id = Some(request.turn_id.clone());
        self.audit.record(Event::new(
            context,
            kind,
            "model",
            &request.state,
            serde_json::json!({"request":request}),
        ));
    }
    pub(super) fn observe_usage(
        &self,
        c: &Conversation,
        holder: &Arc<Mutex<ModelRequest>>,
        metadata: Metadata,
    ) {
        let request = holder.lock().ok().and_then(|mut record| {
            let before = record.clone();
            if let Some(usage) = metadata.usage {
                record
                    .usage
                    .get_or_insert_with(Default::default)
                    .merge(usage);
            }
            if metadata.response_model.is_some() {
                record.response_model = metadata.response_model;
            }
            if metadata.provider_request_id.is_some() {
                record.provider_request_id = metadata.provider_request_id;
            }
            if metadata.http_status.is_some() {
                record.http_status = metadata.http_status;
            }
            (before.usage != record.usage
                || before.http_status != record.http_status
                || before.response_model != record.response_model
                || before.provider_request_id != record.provider_request_id)
                .then(|| record.clone())
        });
        if let Some(request) = request {
            let _ = self.mutate(&c.id, |live| {
                if let Some(record) = live.requests.iter_mut().find(|r| r.id == request.id) {
                    *record = request.clone();
                }
                Ok(())
            });
            self.request_event(c, &request, "model_usage");
        }
    }
    pub(super) fn finish_request(
        &self,
        c: &Conversation,
        holder: &Arc<Mutex<ModelRequest>>,
        result: &Result<Message>,
        partial: &str,
    ) {
        let request = holder.lock().ok().map(|mut record| {
            record.finished_ms = Some(now_ms());
            record.state = match result.as_ref().err().map(ToString::to_string).as_deref() {
                None => "completed",
                Some("AI_CANCELLED") => "cancelled",
                Some("AI_ACCESS_CHANGED") => "interrupted",
                _ => "failed",
            }
            .into();
            record.error = result.as_ref().err().map(ToString::to_string);
            record.clone()
        });
        if let Some(request) = request {
            let _ = self.mutate(&c.id, |live| {
                if let Some(record) = live.requests.iter_mut().find(|r| r.id == request.id) {
                    *record = request.clone();
                }
                Ok(())
            });
            let mut context = self.audit_context(c);
            context.operation_id = request.id.clone();
            context.model_request_id = Some(request.id.clone());
            context.turn_id = Some(request.turn_id.clone());
            self.audit.record(Event::new(context,"model_finished","model",&request.state,serde_json::json!({"request":request,"response":result.as_ref().ok().map(|m|m.wire()),"partial_response":if result.is_err(){Some(partial)}else{None},"partial_response_complete":result.is_ok()})));
        }
    }
    pub(super) fn budget_event(
        &self,
        c: &Conversation,
        kind: &str,
        state: &str,
        details: serde_json::Value,
    ) {
        let mut context = self.audit_context(c);
        context.operation_id = c
            .continuation
            .as_ref()
            .and_then(|v| v.audit_id.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        self.audit
            .record(Event::new(context, kind, "budget", state, details));
    }
    pub(super) fn history_deleted(&self, id: &str) {
        let mut context = AuditContext::new(Source::Local);
        context.conversation_id = Some(id.into());
        self.audit.record(Event::new(
            context,
            "conversation_deleted",
            "history",
            "completed",
            serde_json::json!({"audit_retained":true}),
        ));
    }
}
