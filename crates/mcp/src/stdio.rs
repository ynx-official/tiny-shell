use anyhow::{Context as _, Result, anyhow, bail};
use rmcp::ServiceExt as _;
use std::time::Duration;

use crate::{
    server::Bridge,
    storage::{Store, default_config_dir},
};

const POLICY_POLL_INTERVAL: Duration = Duration::from_millis(500);
const SHUTDOWN_GRACE: Duration = Duration::from_millis(250);

/// Run the authenticated stdio transport before any GUI or application logging starts.
/// The caller must exit the process after this returns; stdin may still have a blocked reader.
pub fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    let mut client = None;
    let mut directory = default_config_dir()?;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--client" => client = Some(args.next().context("missing --client value")?),
            "--config-dir" => directory = args.next().context("missing --config-dir value")?.into(),
            _ => bail!("unknown bridge argument"),
        }
    }
    if !directory.is_absolute() {
        bail!("config directory must be absolute");
    }
    let client_id = client.context("--client is required; register a client in TinyShell")?;
    let store = Store::new(&directory)?;
    let registration = store.client(&client_id)?;
    if !registration.enabled {
        bail!("client registration revoked");
    }
    // The client owns process creation. A disabled bridge must return before it
    // allocates a runtime or opens stdio, even if that client keeps relaunching it.
    if !bridge_enabled(&store, &client_id)? {
        return Ok(());
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(async move {
        let disabled = wait_until_disabled(&store, &client_id);
        tokio::pin!(disabled);
        let bridge = Bridge {
            config_dir: directory,
            client_id: client_id.clone(),
        };
        // Initialization can wait forever for client input too. Monitor the
        // persisted policy before and after the MCP handshake, in both entry points.
        let service = tokio::select! {
            biased;
            result = &mut disabled => return result,
            result = bridge.serve(rmcp::transport::stdio()) => result?,
        };
        let cancellation = service.cancellation_token();
        let waiting = service.waiting();
        tokio::pin!(waiting);
        tokio::select! {
            biased;
            result = &mut disabled => {
                cancellation.cancel();
                // A stalled client must not hold up process exit during cleanup.
                let _ = tokio::time::timeout(SHUTDOWN_GRACE, &mut waiting).await;
                result
            }
            result = &mut waiting => {
                result?;
                Ok(())
            }
        }
    });
    // Tokio's stdin uses an uncancellable blocking read. Dropping the runtime
    // normally waits forever when the client retains its pipe. Bound that wait;
    // returning to either binary's main then releases all remaining OS threads.
    runtime.shutdown_timeout(SHUTDOWN_GRACE);
    result
}

fn bridge_enabled(store: &Store, client: &str) -> Result<bool> {
    let policy = store
        .load_policy()
        .map_err(|_| anyhow!("MCP policy unavailable"))?;
    if !policy.enabled {
        return Ok(false);
    }
    let registration = store
        .client(client)
        .map_err(|_| anyhow!("MCP client registration unavailable"))?;
    Ok(registration.enabled)
}

async fn wait_until_disabled(store: &Store, client: &str) -> Result<()> {
    while bridge_enabled(store, client)? {
        // Read this profile's existing policy/registration only; no global PID
        // matching, extra GUI requests, or effect on the independent built-in AI.
        tokio::time::sleep(POLICY_POLL_INTERVAL).await;
    }
    Ok(())
}
