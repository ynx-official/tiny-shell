use crate::{
    session::config::{ConfigStore, Session},
    sftp::mcp_service::McpRemote,
};
use std::sync::{Arc, Mutex, OnceLock};
use tiny_shell_mcp::{
    engine::{Engine, EngineHandler},
    ipc::Listener,
    storage::Store,
};

pub(crate) struct Service {
    pub engine: Arc<Engine>,
    pub remote: Arc<McpRemote>,
    pub runtime: tokio::runtime::Handle,
    pub listener_error: Arc<Mutex<Option<String>>>,
}
static SERVICE: OnceLock<Result<Arc<Service>, String>> = OnceLock::new();
pub(crate) fn initialize(runtime: &tokio::runtime::Handle) {
    SERVICE.get_or_init(|| {
        let _enter = runtime.enter();
        let directory = ConfigStore::config_directory().map_err(|_| "CONFIG_ERROR".to_owned())?;
        let store = Store::new(&directory).map_err(|_| "STORAGE_ERROR".to_owned())?;
        // Claim the endpoint before starting any profile-owned background work.
        let listener = Listener::bind(&directory).map_err(|_| "BRIDGE_UNAVAILABLE".to_owned())?;
        let remote = Arc::new(McpRemote::new());
        let engine =
            Engine::new(store.clone(), remote.clone()).map_err(|_| "STORAGE_ERROR".to_owned())?;
        engine.audit.start_profile(engine.generation.clone());
        let listener_error = Arc::new(Mutex::new(None));
        let error = listener_error.clone();
        let handler = Arc::new(EngineHandler(engine.clone()));
        let cleanup = store.clone();
        runtime.spawn(async move {
            loop {
                let cleanup = cleanup.clone();
                let delay = tokio::task::spawn_blocking(move || {
                    cleanup.prune_audit()?;
                    cleanup.next_audit_expiry()
                })
                .await
                .ok()
                .and_then(Result::ok)
                .unwrap_or(std::time::Duration::from_secs(3600));
                tokio::time::sleep(delay).await;
            }
        });
        runtime.spawn(async move {
            let result = listener.run(store, handler).await;
            if result.is_err()
                && let Ok(mut error) = error.lock()
            {
                *error = Some("BRIDGE_UNAVAILABLE".to_owned());
            }
        });
        Ok(Arc::new(Service {
            engine,
            remote,
            runtime: runtime.clone(),
            listener_error,
        }))
    });
}
pub(crate) fn service() -> Result<Arc<Service>, String> {
    SERVICE
        .get()
        .cloned()
        .unwrap_or_else(|| Err("BRIDGE_UNAVAILABLE".to_owned()))
}
pub(crate) fn provide_credentials(task: &str, session: Session) -> Result<(), String> {
    let service = service()?;
    let _enter = service.runtime.enter();
    let id = service
        .engine
        .credentials_session(task)
        .map_err(|e| e.code)?;
    service
        .remote
        .credentials(id.clone(), session)
        .map_err(|e| e.code)?;
    let result = service.engine.approve(task, true).map_err(|e| e.code);
    if result.is_err() {
        service.remote.discard_credentials(&id);
    }
    result
}
impl crate::TinyShell {
    pub(crate) fn poll_mcp(&mut self, cx: &mut gpui::Context<Self>) -> bool {
        if let Ok(service) = service() {
            let stamp = service.engine.configuration_stamp();
            if self
                .sync_runtime
                .mcp_configuration_stamp
                .replace(stamp)
                .is_some_and(|old| old != stamp)
            {
                self.note_local_config_saved(cx);
            }
        }
        let stamp = service()
            .map(|s| {
                let _enter = s.runtime.enter();
                let _ = s.engine.maintain();
                s.engine.stamp()
            })
            .unwrap_or(0);
        if stamp == self.mcp_stamp {
            return false;
        }
        self.mcp_stamp = stamp;
        true
    }
}
