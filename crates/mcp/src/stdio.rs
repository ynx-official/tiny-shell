use anyhow::{Context as _, Result, bail};
use rmcp::ServiceExt as _;

use crate::{
    server::Bridge,
    storage::{Store, default_config_dir},
};

/// Run the authenticated stdio transport before any GUI or application logging starts.
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
    let registration = Store::new(&directory)?.client(&client_id)?;
    if !registration.enabled {
        bail!("client registration revoked");
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let service = Bridge {
            config_dir: directory,
            client_id,
        }
        .serve(rmcp::transport::stdio())
        .await?;
        service.waiting().await?;
        Ok(())
    })
}
