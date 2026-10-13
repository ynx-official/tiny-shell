use super::{Event, Filter, Page, Settings, Summary, Usage, redact};
use anyhow::{Result, anyhow, ensure};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use rand::RngCore;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

pub(super) struct Database {
    conn: Connection,
    key: [u8; 32],
    pub settings: Settings,
    path: PathBuf,
    pruned_at: i64,
}
impl Database {
    pub fn open(directory: &Path, supplied_key: Option<[u8; 32]>) -> Result<Self> {
        let root = directory.join("audit");
        crate::storage::private_directory(&root)?;
        let path = root.join("events.sqlite3");
        for name in ["events.sqlite3", "events.sqlite3-wal", "events.sqlite3-shm"] {
            let file = root.join(name);
            if let Ok(meta) = std::fs::symlink_metadata(file) {
                ensure!(!meta.file_type().is_symlink(), "AUDIT_UNSAFE_PATH");
            }
        }
        let mut conn = Connection::open(&path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA temp_store=MEMORY; CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY,value BLOB NOT NULL); CREATE TABLE IF NOT EXISTS events(seq INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT UNIQUE NOT NULL,at INTEGER NOT NULL,op TEXT NOT NULL,source TEXT NOT NULL,conversation TEXT,client TEXT,target TEXT,category TEXT NOT NULL,state TEXT NOT NULL,parent TEXT,payload BLOB NOT NULL); CREATE INDEX IF NOT EXISTS events_op ON events(op,seq); CREATE INDEX IF NOT EXISTS events_conversation ON events(conversation,seq); CREATE INDEX IF NOT EXISTS events_source ON events(source,seq); CREATE INDEX IF NOT EXISTS events_target ON events(target,seq); CREATE TABLE IF NOT EXISTS model_requests(id TEXT PRIMARY KEY,conversation TEXT,at INTEGER NOT NULL,input INTEGER,output INTEGER,total INTEGER,cached INTEGER,reasoning INTEGER);")?;
        // The database transaction serializes first-key creation across helper/GUI processes.
        let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let columns = transaction
            .prepare("PRAGMA table_info(events)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for name in ["priority", "folded", "model_record"] {
            if !columns.iter().any(|s| s == name) {
                transaction.execute_batch(&format!(
                    "ALTER TABLE events ADD COLUMN {name} INTEGER NOT NULL DEFAULT 0;"
                ))?;
            }
        }
        transaction.execute_batch("CREATE INDEX IF NOT EXISTS events_latest ON events(op,priority DESC,seq DESC); CREATE INDEX IF NOT EXISTS events_parent ON events(parent,op);")?;
        let binding: Option<Vec<u8>> = transaction
            .query_row("SELECT value FROM meta WHERE key='key-check'", [], |r| {
                r.get(0)
            })
            .optional()?;
        let key = if let Some(key) = supplied_key {
            key
        } else {
            let canonical = root.canonicalize()?;
            let service = format!(
                "tiny-shell.audit.{}",
                hex::encode(Sha256::digest(canonical.to_string_lossy().as_bytes()))
            );
            let entry = keyring::Entry::new(&service, "audit-key")
                .map_err(|_| anyhow!("AUDIT_KEY_UNAVAILABLE"))?;
            match entry.get_secret() {
                Ok(bytes) => bytes
                    .try_into()
                    .map_err(|_| anyhow!("AUDIT_KEY_UNAVAILABLE"))?,
                Err(keyring::Error::NoEntry) if binding.is_none() => {
                    let mut key = [0; 32];
                    rand::thread_rng().fill_bytes(&mut key);
                    entry
                        .set_secret(&key)
                        .map_err(|_| anyhow!("AUDIT_KEY_UNAVAILABLE"))?;
                    key
                }
                _ => return Err(anyhow!("AUDIT_KEY_UNAVAILABLE")),
            }
        };
        if let Some(binding) = binding {
            ensure!(
                decrypt(&key, "key-check", &binding)? == b"tiny-shell-audit-v1",
                "AUDIT_KEY_UNAVAILABLE"
            );
        } else {
            transaction.execute(
                "INSERT INTO meta(key,value) VALUES('key-check',?1)",
                [encrypt(&key, "key-check", b"tiny-shell-audit-v1")?],
            )?;
        }
        let version: Option<Vec<u8>> = transaction
            .query_row("SELECT value FROM meta WHERE key='schema'", [], |r| {
                r.get(0)
            })
            .optional()?;
        ensure!(
            version.as_deref().is_none_or(|v| v == b"1"),
            "AUDIT_UNSUPPORTED_SCHEMA"
        );
        transaction.execute(
            "INSERT OR IGNORE INTO meta(key,value) VALUES('schema',?1)",
            [b"1".as_slice()],
        )?;
        let stored: Option<Vec<u8>> = transaction
            .query_row("SELECT value FROM meta WHERE key='settings'", [], |r| {
                r.get(0)
            })
            .optional()?;
        let settings = stored
            .map(|b| {
                decrypt(&key, "settings", &b)
                    .and_then(|b| Ok(serde_json::from_slice::<Settings>(&b)?))
            })
            .transpose()?
            .unwrap_or_default();
        settings.validate()?;
        transaction.commit()?;
        Ok(Self {
            conn,
            key,
            settings,
            path,
            pruned_at: 0,
        })
    }
    pub fn bytes(&self) -> u64 {
        let wal = self.path.with_extension("sqlite3-wal");
        [&self.path, &wal]
            .iter()
            .filter_map(|p| std::fs::metadata(p).ok())
            .map(|m| m.len())
            .sum()
    }
    pub fn configure(&mut self, settings: Settings) -> Result<()> {
        settings.validate()?;
        let sealed = encrypt(&self.key, "settings", &serde_json::to_vec(&settings)?)?;
        self.conn.execute("INSERT INTO meta(key,value) VALUES('settings',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[sealed])?;
        self.settings = settings;
        self.pruned_at = 0;
        Ok(())
    }
    pub fn append(&mut self, event: &mut Event) -> Result<()> {
        if super::now_ms().saturating_sub(self.pruned_at) >= 60_000 {
            self.prune()?;
        }
        ensure!(
            self.bytes() < u64::from(self.settings.capacity_mb) * 1024 * 1024,
            "AUDIT_CAPACITY"
        );
        let custom = self
            .settings
            .redact_patterns
            .iter()
            .filter_map(|p| regex::Regex::new(p).ok())
            .collect::<Vec<_>>();
        // Numeric usage and generated request IDs survive user redaction rules.
        let model_usage = (event.tool == "model"
            && event.context.model_request_id.is_some()
            && event.details["request"].is_object())
        .then(|| {
            serde_json::from_value::<Usage>(event.details["request"]["usage"].clone())
                .unwrap_or_default()
        });
        let request_started = event.details["request"]["started_ms"]
            .as_i64()
            .unwrap_or(event.timestamp_ms);
        redact::event(event, &custom);
        // Preserve generated correlation IDs and schema keys even for broad custom expressions.
        let document = serde_json::to_value(&*event)?;
        let bytes = serde_json::to_vec(&document)?;
        ensure!(bytes.len() <= 3 * 1024 * 1024, "AUDIT_RECORD_LIMIT");
        let sealed = encrypt(&self.key, &event.id, &bytes)?;
        let source = enum_name(event.context.source)?;
        let category = enum_name(event.category)?;
        // Only opaque identifiers and fixed enums are visible in the SQLite index/WAL.
        let safe_id = |s: Option<&String>| s.map(|s| hex::encode(Sha256::digest(s.as_bytes())));
        let transaction = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute("INSERT OR IGNORE INTO events(id,at,op,source,conversation,client,target,category,state,parent,payload,priority,folded,model_record) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",params![event.id,event.timestamp_ms,hex::encode(Sha256::digest(event.context.operation_id.as_bytes())),source,safe_id(event.context.conversation_id.as_ref()),safe_id(event.context.client_id.as_ref()),safe_id(event.context.target_id.as_ref()),category,safe_state(&event.state),safe_id(event.context.parent_id.as_ref()),sealed,priority(event),event.tool=="get_task" && event.state!="rejected",model_usage.is_some()])?;
        if let Some(usage) = model_usage {
            let number = |n: Option<u64>| n.and_then(|n| i64::try_from(n).ok());
            transaction.execute("INSERT INTO model_requests(id,conversation,at,input,output,total,cached,reasoning) VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(id) DO UPDATE SET input=COALESCE(excluded.input,input),output=COALESCE(excluded.output,output),total=COALESCE(excluded.total,total),cached=COALESCE(excluded.cached,cached),reasoning=COALESCE(excluded.reasoning,reasoning)",params![safe_id(event.context.model_request_id.as_ref()),safe_id(event.context.conversation_id.as_ref()),request_started,number(usage.input),number(usage.output),number(usage.total),number(usage.cached_input),number(usage.reasoning_output)])?;
        }
        transaction.commit()?;
        Ok(())
    }
    pub fn summary(&self, conversation: Option<String>) -> Result<Summary> {
        let conversation = conversation.map(|s| hex::encode(Sha256::digest(s.as_bytes())));
        // Model events use the request ID as their operation ID. Reuse the indexed terminal
        // state instead of treating every NULL usage as unknown (or adding a parallel state).
        // Missing/pruned events conservatively leave incomplete usage unknown.
        let mut statement = self.conn.prepare(
            "SELECT m.input,m.output,m.total,m.cached,m.reasoning,
                COALESCE((SELECT e.state IN ('pending','running') FROM events e
                    WHERE e.op=m.id AND e.model_record=1
                    ORDER BY e.priority DESC,e.seq DESC LIMIT 1),0)
             FROM model_requests m WHERE m.conversation IS NOT NULL
                AND (?1 IS NULL OR m.conversation=?1)",
        )?;
        let rows = statement.query_map([conversation], |r| {
            let number = |index| -> rusqlite::Result<Option<u64>> {
                r.get::<_, Option<i64>>(index)?
                    .map(|n| {
                        u64::try_from(n).map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                index,
                                rusqlite::types::Type::Integer,
                                Box::new(error),
                            )
                        })
                    })
                    .transpose()
            };
            Ok((
                super::Usage {
                    input: number(0)?,
                    output: number(1)?,
                    total: number(2)?,
                    cached_input: number(3)?,
                    reasoning_output: number(4)?,
                    ..super::Usage::default()
                },
                r.get::<_, bool>(5)?,
            ))
        })?;
        let mut summary = Summary::default();
        for row in rows {
            let (usage, pending) = row?;
            summary.add(Some(&usage), pending);
        }
        Ok(summary)
    }
    pub fn page(&self, filter: &Filter) -> Result<Page> {
        let limit = filter.limit.clamp(1, 200);
        let hash = |s: Option<&String>| s.map(|s| hex::encode(Sha256::digest(s.as_bytes())));
        let source = filter.source.map(enum_name).transpose()?;
        let category = filter.category.map(enum_name).transpose()?;
        let mut statement = self.conn.prepare("WITH RECURSIVE related(op) AS (SELECT ?3 UNION SELECT e.op FROM events e JOIN related r ON e.parent=r.op) SELECT e.seq,e.id,e.payload FROM events e WHERE (?1 IS NULL OR e.source=?1) AND (?2 IS NULL OR e.conversation=?2) AND (?3 IS NULL OR e.op IN (SELECT op FROM related)) AND (?4 IS NULL OR e.target=?4) AND (?5 IS NULL OR e.category=?5) AND (?6 IS NULL OR e.state=?6) AND (?7 IS NULL OR e.at>=?7) AND (?8 IS NULL OR e.at<=?8) AND (?9 IS NULL OR e.seq<?9) AND (?10=1 OR (e.folded=0 AND e.seq=(SELECT t.seq FROM events t WHERE t.op=e.op ORDER BY t.priority DESC,t.seq DESC LIMIT 1))) AND (?11 IS NULL OR e.client=?11) AND (?12=0 OR e.model_record=1) ORDER BY e.seq DESC LIMIT 1000")?;
        let rows = statement.query_map(
            params![
                source,
                hash(filter.conversation_id.as_ref()),
                hash(filter.operation_id.as_ref()),
                hash(filter.target_id.as_ref()),
                category,
                filter.state.as_deref().map(safe_state),
                filter.since_ms,
                filter.until_ms,
                filter.before_sequence,
                filter.operation_id.is_some() || filter.approvals_only,
                hash(filter.client_id.as_ref()),
                filter.model_requests_only
            ],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                ))
            },
        )?;
        let mut page = Page::default();
        let search = filter.search.to_lowercase();
        let mut scanned_bytes = 0usize;
        for row in rows {
            let (seq, id, payload) = row?;
            if scanned_bytes > 0 && scanned_bytes.saturating_add(payload.len()) > 16 * 1024 * 1024 {
                return Ok(page);
            }
            scanned_bytes = scanned_bytes.saturating_add(payload.len());
            let mut event: Event = serde_json::from_slice(&decrypt(&self.key, &id, &payload)?)?;
            event.sequence = seq;
            ensure!(event.id == id, "AUDIT_CORRUPT");
            page.scanned += 1;
            page.next = Some(seq);
            if filter.approvals_only
                && !event.kind.starts_with("approval_")
                && !event.kind.starts_with("budget_")
            {
                continue;
            }
            if !search.is_empty()
                && !serde_json::to_string(&event)?
                    .to_lowercase()
                    .contains(&search)
            {
                continue;
            }
            page.events.push(event);
            if page.events.len() >= limit {
                return Ok(page);
            }
        }
        if page.scanned < 1000 {
            page.next = None;
        }
        Ok(page)
    }
    pub fn clear(&mut self) -> Result<()> {
        self.conn.execute("DELETE FROM events", [])?;
        self.conn.execute("DELETE FROM model_requests", [])?;
        self.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;")?;
        Ok(())
    }
    /// Only the GUI which owns the IPC endpoint may call this; helpers never recover live work.
    pub fn start_profile(&mut self, directory: &Path, instance: &str) -> Result<()> {
        let imported: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key='legacy-imported'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        if imported.is_none() {
            let path = directory.join("mcp/audit.jsonl");
            if path.exists() {
                let bytes = crate::storage::private_bytes(&path, 10 * 1024 * 1024)?;
                for (index, line) in String::from_utf8_lossy(&bytes).lines().enumerate() {
                    let Ok(details) = serde_json::from_str::<serde_json::Value>(line) else {
                        continue;
                    };
                    let client = details["client_id"].as_str().unwrap_or_default();
                    let mut context = super::Context::client(client);
                    context.operation_id = format!(
                        "legacy-{}",
                        hex::encode(Sha256::digest(format!("{index}:{line}").as_bytes()))
                    );
                    context.target_id = details["connection_id"].as_str().map(str::to_owned);
                    let tool = details["operation"].as_str().unwrap_or("legacy");
                    let state = details["state"].as_str().unwrap_or("unknown");
                    let mut event = Event::new(
                        context,
                        "legacy_summary",
                        tool,
                        state,
                        serde_json::json!({"legacy":details,"details_unknown":true,"usage_unknown":true}),
                    );
                    if let Some(at) = details["timestamp"]
                        .as_i64()
                        .and_then(|n| n.checked_mul(1000))
                    {
                        event.timestamp_ms = at;
                    }
                    event.id = event.context.operation_id.clone();
                    self.append(&mut event)?;
                }
            }
            self.conn.execute(
                "INSERT OR IGNORE INTO meta(key,value) VALUES('legacy-imported',?1)",
                [b"1".as_slice()],
            )?;
        }
        let mut cursor = None;
        loop {
            let page = self.page(&Filter {
                before_sequence: cursor,
                limit: 200,
                ..Filter::default()
            })?;
            for previous in page.events {
                if !matches!(
                    previous.context.source,
                    super::Source::Ai | super::Source::External | super::Source::Local
                ) || previous.context.instance_id.as_deref() == Some(instance)
                    || !matches!(
                        previous.state.as_str(),
                        "pending" | "running" | "queued" | "awaiting_user" | "cancel_requested"
                    )
                {
                    continue;
                }
                let state = if matches!(previous.tool.as_str(), "run_command" | "write_file") {
                    "outcome_unknown"
                } else {
                    "interrupted"
                };
                let mut details = serde_json::json!({"previous_event_id":previous.id,"previous_state":previous.state,"reason":"application_restarted","not_replayed":true});
                if previous.tool == "model" {
                    let mut request = previous.details["request"].clone();
                    request["state"] = "interrupted".into();
                    request["finished_ms"] = super::now_ms().into();
                    details["request"] = request;
                }
                self.append(&mut Event::new(
                    previous.context,
                    "recovery_interrupted",
                    &previous.tool,
                    state,
                    details,
                ))?;
            }
            match page.next {
                Some(next) if cursor != Some(next) => cursor = Some(next),
                _ => break,
            }
        }
        Ok(())
    }
    fn prune(&mut self) -> Result<()> {
        let cutoff =
            super::now_ms().saturating_sub(i64::from(self.settings.retention_days) * 86_400_000);
        // Preserve every event of a recently active operation; trim complete old groups together.
        self.conn.execute(
            "DELETE FROM events WHERE op IN (SELECT op FROM events GROUP BY op HAVING MAX(at)<?1)",
            [cutoff],
        )?;
        self.conn
            .execute("DELETE FROM model_requests WHERE at<?1", [cutoff])?;
        self.pruned_at = super::now_ms();
        Ok(())
    }
}
fn priority(event: &Event) -> i64 {
    match event.kind.as_str() {
        "execution_result" | "tool_completed" | "model_finished" | "recovery_interrupted" => 10,
        "request_received" | "external_received" => 1,
        "request_result" => 2,
        _ => 5,
    }
}
fn safe_state(state: &str) -> &str {
    match state {
        "received" | "queued" | "awaiting_user" | "running" | "succeeded" | "failed"
        | "cancelled" | "cancel_requested" | "outcome_unknown" | "timed_out" | "interrupted"
        | "approved" | "rejected" | "expired" | "invalidated" | "completed" | "reported"
        | "not_executed" | "cached" | "idempotent" | "pending" | "recorded" => state,
        _ => "unknown",
    }
}
fn enum_name(value: impl serde::Serialize) -> Result<String> {
    Ok(serde_json::to_value(value)?
        .as_str()
        .unwrap_or("unknown")
        .to_owned())
}
fn encrypt(key: &[u8; 32], id: &str, bytes: &[u8]) -> Result<Vec<u8>> {
    let mut nonce = [0; 24];
    rand::thread_rng().fill_bytes(&mut nonce);
    let encrypted = XChaCha20Poly1305::new(key.into())
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: bytes,
                aad: id.as_bytes(),
            },
        )
        .map_err(|_| anyhow!("AUDIT_CRYPTO"))?;
    let mut result = b"TSAU1".to_vec();
    result.extend(nonce);
    result.extend(encrypted);
    Ok(result)
}
fn decrypt(key: &[u8; 32], id: &str, bytes: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        bytes.len() >= 45 && &bytes[..5] == b"TSAU1",
        "AUDIT_CORRUPT"
    );
    XChaCha20Poly1305::new(key.into())
        .decrypt(
            XNonce::from_slice(&bytes[5..29]),
            Payload {
                msg: &bytes[29..],
                aad: id.as_bytes(),
            },
        )
        .map_err(|_| anyhow!("AUDIT_CORRUPT"))
}

#[cfg(test)]
mod tests {
    use super::super::{Context, Source};
    use super::*;
    use serde_json::{Value, json};
    #[cfg(windows)]
    #[test]
    #[ignore = "requires the current Windows Credential Manager"]
    fn native_vault_database_roundtrip() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let root = directory.path().join("audit");
        crate::storage::private_directory(&root)?;
        let service = format!(
            "tiny-shell.audit.{}",
            hex::encode(Sha256::digest(
                root.canonicalize()?.to_string_lossy().as_bytes()
            ))
        );
        struct Cleanup(keyring::Entry);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = self.0.delete_credential();
            }
        }
        let guard = Cleanup(keyring::Entry::new(&service, "audit-key")?);
        let mut db = Database::open(directory.path(), None)?;
        let mut event = Event::new(
            Context::new(Source::Ai),
            "execution_result",
            "read_file",
            "succeeded",
            json!({"arguments":{"path":"/tmp/synthetic"},"result":{"content":"native-vault-fixture"}}),
        );
        db.append(&mut event)?;
        drop(db);
        assert_eq!(guard.0.get_secret()?.len(), 32);
        let reopened = Database::open(directory.path(), None)?;
        let page = reopened.page(&Filter {
            limit: 20,
            ..Default::default()
        })?;
        assert_eq!(
            page.events[0].details["result"]["content"],
            "native-vault-fixture"
        );
        drop(reopened);
        Ok(())
    }
    #[test]
    fn usage_summary_waits_for_completion_and_preserves_prior_unknown_requests() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = Database::open(dir.path(), Some([31; 32])).unwrap();
        let mut first = Context::client("ai:usage-lifecycle");
        first.model_request_id = Some(first.operation_id.clone());
        let record = |context: &Context, kind: &str, state: &str, usage: Value| {
            Event::new(
                context.clone(),
                kind,
                "model",
                state,
                json!({"request":{
                    "started_ms":super::super::now_ms(),"state":state,"usage":usage
                }}),
            )
        };
        db.append(&mut record(&first, "model_started", "pending", Value::Null))
            .unwrap();
        assert_eq!(
            db.summary(None).unwrap().unknown,
            0,
            "in-flight usage is not missing usage"
        );
        db.append(&mut record(
            &first,
            "model_finished",
            "completed",
            Value::Null,
        ))
        .unwrap();
        assert_eq!(db.summary(None).unwrap().unknown, 1);
        let mut second = Context::client("ai:usage-lifecycle");
        second.model_request_id = Some(second.operation_id.clone());
        db.append(&mut record(
            &second,
            "model_started",
            "pending",
            Value::Null,
        ))
        .unwrap();
        assert_eq!(db.summary(None).unwrap().unknown, 1);
        let usage = json!({"input":40,"output":15,"total":55});
        db.append(&mut record(
            &second,
            "model_usage",
            "pending",
            usage.clone(),
        ))
        .unwrap();
        db.append(&mut record(&second, "model_finished", "completed", usage))
            .unwrap();
        // Late active events must not revert the first request to waiting or erase its unknown usage.
        db.append(&mut record(&first, "model_started", "pending", Value::Null))
            .unwrap();
        let summary = db.summary(None).unwrap();
        assert_eq!(
            (
                summary.requests,
                summary.confirmed,
                summary.unknown,
                summary.total
            ),
            (2, 1, 1, 55)
        );
        drop(db);
        let db = Database::open(dir.path(), Some([31; 32])).unwrap();
        let summary = db.summary(Some("usage-lifecycle".into())).unwrap();
        assert_eq!(
            (
                summary.requests,
                summary.confirmed,
                summary.unknown,
                summary.total
            ),
            (2, 1, 1, 55)
        );
    }
    #[test]
    fn interrupted_model_usage_changes_from_waiting_to_unknown_on_profile_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = Database::open(dir.path(), Some([32; 32])).unwrap();
        let mut context = Context::client("ai:recovered-usage");
        context.instance_id = Some("previous-instance".into());
        context.model_request_id = Some(context.operation_id.clone());
        db.append(&mut Event::new(
            context,
            "model_started",
            "model",
            "pending",
            json!({
                "request":{"started_ms":super::super::now_ms(),"state":"pending","usage":null}
            }),
        ))
        .unwrap();
        assert_eq!(db.summary(None).unwrap().pending, 1);
        drop(db);
        let mut db = Database::open(dir.path(), Some([32; 32])).unwrap();
        db.start_profile(dir.path(), "new-instance").unwrap();
        let summary = db.summary(None).unwrap();
        assert_eq!(
            (summary.requests, summary.pending, summary.unknown),
            (1, 0, 1)
        );
        db.start_profile(dir.path(), "new-instance").unwrap();
        assert_eq!(db.summary(None).unwrap().unknown, 1);
    }
    #[test]
    fn usage_updates_are_idempotent_and_external_operations_never_enter_token_totals() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = Database::open(dir.path(), Some([13; 32])).unwrap();
        let mut context = Context::new(Source::Ai);
        context.conversation_id = Some("conversation".into());
        context.model_request_id = Some(context.operation_id.clone());
        let make = |kind, usage| {
            Event::new(
                context.clone(),
                kind,
                "model",
                "reported",
                json!({"request":{"started_ms":super::super::now_ms(),"usage":usage}}),
            )
        };
        db.append(&mut make("model_started", Value::Null)).unwrap();
        let usage = json!({"input":10,"output":4,"total":14,"cached_input":8,"reasoning_output":2,"cache_write":null,"total_derived":false});
        db.append(&mut make("model_usage", usage.clone())).unwrap();
        db.append(&mut make("model_usage", usage)).unwrap();
        db.append(&mut Event::new(
            Context::new(Source::External),
            "execution_result",
            "run_command",
            "succeeded",
            json!({"usage":{"total":999999}}),
        ))
        .unwrap();
        let sum = db.summary(Some("conversation".into())).unwrap();
        assert_eq!(sum.requests, 1);
        assert_eq!(sum.total, 14);
        assert_eq!(sum.cached_input, 8);
        assert_eq!(sum.reasoning_output, 2);
        assert_eq!(db.summary(None).unwrap().total, 14);
        db.conn
            .execute("UPDATE model_requests SET input=-1", [])
            .unwrap();
        assert!(
            db.summary(None).is_err(),
            "negative index values must never wrap into a huge token count"
        );
    }
    #[test]
    fn custom_redaction_preserves_correlation_and_terminal_state_survives_late_ticket_response() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = Database::open(dir.path(), Some([14; 32])).unwrap();
        db.configure(Settings {
            redact_patterns: vec![".+".into()],
            ..Settings::default()
        })
        .unwrap();
        let context = Context::new(Source::Ai);
        let operation = context.operation_id.clone();
        db.append(&mut Event::new(
            context.clone(),
            "execution_result",
            "run_command",
            "succeeded",
            json!({"stdout":"business-secret"}),
        ))
        .unwrap();
        db.append(&mut Event::new(
            context,
            "request_result",
            "run_command",
            "queued",
            json!({"task_id":"ticket"}),
        ))
        .unwrap();
        let page = db
            .page(&Filter {
                limit: 20,
                ..Filter::default()
            })
            .unwrap();
        assert_eq!(page.events.len(), 1);
        assert_eq!(page.events[0].state, "succeeded");
        assert_eq!(page.events[0].context.operation_id, operation);
        assert!(
            !page.events[0]
                .details
                .to_string()
                .contains("business-secret")
        );
        assert_eq!(
            db.page(&Filter {
                operation_id: Some(operation),
                limit: 20,
                ..Filter::default()
            })
            .unwrap()
            .events
            .len(),
            2
        );
    }
    #[test]
    fn polling_is_folded_but_recursive_operation_timeline_retains_raw_requests() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = Database::open(dir.path(), Some([15; 32])).unwrap();
        let context = Context::new(Source::External);
        let root = context.operation_id.clone();
        db.append(&mut Event::new(
            context.clone(),
            "tool_requested",
            "run_command",
            "received",
            json!({}),
        ))
        .unwrap();
        let child = context.child();
        db.append(&mut Event::new(
            child.clone(),
            "execution_result",
            "run_command",
            "succeeded",
            json!({}),
        ))
        .unwrap();
        let mut raw = child.child();
        let query = raw.operation_id.clone();
        raw.parent_id = None;
        db.append(&mut Event::new(
            raw,
            "external_received",
            "get_task",
            "received",
            json!({"arguments":{"offset":0}}),
        ))
        .unwrap();
        let mut normalized = child.child();
        normalized.operation_id = query;
        db.append(&mut Event::new(
            normalized,
            "request_result",
            "get_task",
            "succeeded",
            json!({"offset":0,"next_offset":20}),
        ))
        .unwrap();
        assert!(
            db.page(&Filter {
                limit: 20,
                ..Filter::default()
            })
            .unwrap()
            .events
            .iter()
            .all(|e| e.tool != "get_task")
        );
        let timeline = db
            .page(&Filter {
                operation_id: Some(root),
                limit: 20,
                ..Filter::default()
            })
            .unwrap();
        assert_eq!(timeline.events.len(), 4);
    }
    #[test]
    fn startup_records_unknown_outcome_once_and_authenticated_payload_detects_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = Database::open(dir.path(), Some([16; 32])).unwrap();
        let mut context = Context::new(Source::Ai);
        context.instance_id = Some("old-instance".into());
        let operation = context.operation_id.clone();
        db.append(&mut Event::new(
            context,
            "execution_started",
            "run_command",
            "running",
            json!({}),
        ))
        .unwrap();
        db.start_profile(dir.path(), "new-instance").unwrap();
        db.start_profile(dir.path(), "new-instance").unwrap();
        let page = db
            .page(&Filter {
                operation_id: Some(operation),
                limit: 20,
                ..Filter::default()
            })
            .unwrap();
        assert_eq!(page.events.len(), 2);
        assert_eq!(page.events[0].state, "outcome_unknown");
        db.conn
            .execute("UPDATE events SET payload=zeroblob(60)", [])
            .unwrap();
        assert!(
            db.page(&Filter {
                limit: 20,
                ..Filter::default()
            })
            .is_err()
        );
    }
    #[test]
    fn database_wal_and_reopened_payload_do_not_expose_command_or_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = Database::open(dir.path(), Some([11; 32])).unwrap();
        let mut event = Event::new(
            Context::new(Source::External),
            "request_received",
            "run_command",
            "received",
            json!({"arguments":{"command":"echo audit-business-value password=hidden-credential"}}),
        );
        db.append(&mut event).unwrap();
        for name in ["events.sqlite3", "events.sqlite3-wal"] {
            let bytes = std::fs::read(dir.path().join("audit").join(name)).unwrap_or_default();
            let text = String::from_utf8_lossy(&bytes);
            assert!(!text.contains("audit-business-value"));
            assert!(!text.contains("hidden-credential"));
        }
        drop(db);
        let reopened = Database::open(dir.path(), Some([11; 32])).unwrap();
        let page = reopened
            .page(&Filter {
                limit: 20,
                ..Filter::default()
            })
            .unwrap();
        let result = page.events[0].details.to_string();
        assert!(result.contains("audit-business-value"));
        assert!(!result.contains("hidden-credential"));
        assert!(Database::open(dir.path(), Some([12; 32])).is_err());
    }
}
