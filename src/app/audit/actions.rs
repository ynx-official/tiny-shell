use super::*;
use crate::app::confirmation_dialog::ConfirmationDialog;
use serde_json::json;
use tiny_shell_mcp::audit::{Context as AuditContext, Event, Settings};

impl AuditPanel {
    pub(super) fn reset_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter = super::presentation::reset_record_filter(&self.filter);
        self.search
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.selection = None;
        self.load(cx);
    }
    pub(super) fn retry(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        let store = self.store.clone();
        cx.spawn(async move |this, cx| {
            let result = store.retry().await;
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                this.status = store.status();
                this.error = result.err().map(|e| e.to_string());
                if this.error.is_none() {
                    this.load(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn save_settings(&mut self, cx: &mut Context<Self>) {
        let settings = (|| {
            let settings = Settings {
                retention_days: self.days.read(cx).value().trim().parse()?,
                capacity_mb: self.capacity.read(cx).value().trim().parse()?,
                redact_patterns: self
                    .patterns
                    .read(cx)
                    .value()
                    .lines()
                    .map(str::trim)
                    .filter(|p| !p.is_empty())
                    .map(str::to_owned)
                    .collect(),
            };
            settings.validate()?;
            Ok::<_, anyhow::Error>(settings)
        })();
        let Ok(settings) = settings else {
            self.error = Some("AUDIT_INVALID_SETTINGS".into());
            cx.notify();
            return;
        };
        let store = self.store.clone();
        self.loading = true;
        cx.spawn(async move|this,cx|{
            let result=store.configure(settings.clone()).await;
            if result.is_ok(){store.record(Event::new(AuditContext::new(Source::Local),"settings_changed","audit","completed",json!({"retention_days":settings.retention_days,"capacity_mb":settings.capacity_mb,"redaction_rules":settings.redact_patterns.len()})));}
            let _=this.update(cx,|this,cx|{this.loading=false;this.status=store.status();match result{Ok(())=>{this.notice=Some("audit_settings_saved".into());this.error=None;},Err(e)=>this.error=Some(e.to_string())}cx.notify();});
        }).detach();
        cx.notify();
    }
    pub(super) fn confirm_clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let store = self.store.clone();
        let panel = cx.entity().downgrade();
        ConfirmationDialog::new(
            t!("audit_clear").to_string(),
            t!("audit_clear_confirm").to_string(),
        )
        .danger(true)
        .confirm_label(t!("audit_clear").to_string())
        .on_ok(move |_, _, cx| {
            let store = store.clone();
            let panel = panel.clone();
            cx.spawn(async move |cx| {
                let result = store.clear().await;
                if result.is_ok() {
                    store.record(Event::new(
                        AuditContext::new(Source::Local),
                        "audit_cleared",
                        "audit",
                        "completed",
                        json!({"chat_history_retained":true}),
                    ));
                }
                let _ = panel.update(cx, |this, cx| {
                    match result {
                        Ok(()) => {
                            this.notice = Some("audit_cleared".into());
                            this.filter.before_sequence = None;
                            this.load(cx);
                        }
                        Err(e) => this.error = Some(e.to_string()),
                    }
                    cx.notify();
                });
            })
            .detach();
            true
        })
        .open(window, cx);
    }
    pub(super) fn confirm_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let panel = cx.entity().downgrade();
        ConfirmationDialog::new(
            t!("audit_export_details").to_string(),
            t!("audit_export_confirm").to_string(),
        )
        .confirm_label(t!("audit_export_details").to_string())
        .on_ok(move |_, _, cx| {
            if let Some(panel) = panel.upgrade() {
                panel.update(cx, |this, cx| this.export_details(cx));
            }
            true
        })
        .open(window, cx);
    }
    pub(super) fn export_summary(&mut self, cx: &mut Context<Self>) {
        let mut csv = String::from(
            "timestamp,source,operation,type,state,operation_id,conversation_id,target_id,client_name,target_name,purpose,turn_id,task_id,model_request_id,input_tokens,output_tokens,total_tokens,usage_state\n",
        );
        let cell = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
        for event in &self.page.events {
            // Prefix formula-like fields so an audit export is safe to open in a spreadsheet.
            let safe = |s: &str| {
                if s.trim_start_matches([' ', '\t', '\r', '\n'])
                    .starts_with(['=', '+', '-', '@'])
                {
                    format!("'{s}")
                } else {
                    s.to_owned()
                }
            };
            let fields = [
                event.timestamp_ms.to_string(),
                format!("{:?}", event.context.source),
                event.tool.clone(),
                format!("{:?}", event.category),
                event.state.clone(),
                event.context.operation_id.clone(),
                event.context.conversation_id.clone().unwrap_or_default(),
                event.context.target_id.clone().unwrap_or_default(),
                event.context.client_name.clone().unwrap_or_default(),
                event.context.target_name.clone().unwrap_or_default(),
                event.context.purpose.clone().unwrap_or_default(),
                event.context.turn_id.clone().unwrap_or_default(),
                event.context.task_id.clone().unwrap_or_default(),
                event.context.model_request_id.clone().unwrap_or_default(),
                event.details["request"]["usage"]["input"]
                    .as_u64()
                    .map(|n| n.to_string())
                    .unwrap_or_default(),
                event.details["request"]["usage"]["output"]
                    .as_u64()
                    .map(|n| n.to_string())
                    .unwrap_or_default(),
                event.details["request"]["usage"]["total"]
                    .as_u64()
                    .map(|n| n.to_string())
                    .unwrap_or_default(),
                if event.context.source == Source::External {
                    "excluded"
                } else if event.details["request"]["usage"].is_object() {
                    "reported"
                } else {
                    "unknown"
                }
                .into(),
            ];
            csv.push_str(
                &fields
                    .iter()
                    .map(|s| cell(&safe(s)))
                    .collect::<Vec<_>>()
                    .join(","),
            );
            csv.push('\n');
        }
        self.save_export(
            "tiny-shell-audit-summary.csv",
            "CSV",
            "csv",
            csv.into_bytes(),
            false,
            cx,
        );
    }
    fn export_details(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selection.clone() else {
            return;
        };
        self.loading = true;
        let store = self.store.clone();
        cx.spawn(async move |this, cx| {
            let result = async {
                let mut cursor = None;
                let mut bytes = vec![b'['];
                let mut first = true;
                loop {
                    let page = store
                        .page(Filter {
                            operation_id: Some(id.clone()),
                            before_sequence: cursor,
                            limit: 200,
                            ..Default::default()
                        })
                        .await?;
                    let next = page.next;
                    let block = cx
                        .background_executor()
                        .spawn(async move { serde_json::to_vec(&page.events) })
                        .await?;
                    anyhow::ensure!(
                        bytes.len().saturating_add(block.len()) <= 64 * 1024 * 1024,
                        "AUDIT_EXPORT_LIMIT"
                    );
                    if block.len() > 2 {
                        if !first {
                            bytes.push(b',');
                        }
                        bytes.extend_from_slice(&block[1..block.len() - 1]);
                        first = false;
                    }
                    match next {
                        Some(next) if cursor != Some(next) => cursor = Some(next),
                        _ => break,
                    }
                }
                bytes.push(b']');
                Ok::<_, anyhow::Error>(bytes)
            }
            .await;
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok(bytes) => this.save_export(
                        "tiny-shell-audit-details.json",
                        "JSON",
                        "json",
                        bytes,
                        true,
                        cx,
                    ),
                    Err(error) => this.error = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn save_export(
        &mut self,
        name: &str,
        format: &str,
        extension: &str,
        bytes: Vec<u8>,
        details: bool,
        cx: &mut Context<Self>,
    ) {
        let dialog = rfd::AsyncFileDialog::new()
            .add_filter(format, &[extension])
            .set_file_name(name)
            .save_file();
        let store = self.store.clone();
        cx.spawn(async move |this, cx| {
            if let Some(file) = dialog.await {
                let path = file.path().to_path_buf();
                let result = cx
                    .background_executor()
                    .spawn(async move { tiny_shell_mcp::storage::atomic_bytes(&path, &bytes) })
                    .await;
                if result.is_ok() {
                    store.record(Event::new(
                        AuditContext::new(Source::Local),
                        "audit_exported",
                        "audit",
                        "completed",
                        json!({"details":details,"desensitized":true}),
                    ));
                }
                let _ = this.update(cx, |this, cx| {
                    match result {
                        Ok(()) => this.notice = Some("audit_exported".into()),
                        Err(e) => this.error = Some(e.to_string()),
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }
}
