use async_trait::async_trait;
use russh::client::Handler;

use crate::session::host_key::HostKeyVerifier;

#[derive(Clone)]
pub(crate) struct SftpClientHandler {
    host_key_verifier: HostKeyVerifier,
}

impl SftpClientHandler {
    pub(crate) fn new(host: &str, port: u16) -> anyhow::Result<Self> {
        Ok(Self {
            host_key_verifier: HostKeyVerifier::new(host, port)?,
        })
    }
}

#[async_trait]
impl Handler for SftpClientHandler {
    type Error = anyhow::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        self.host_key_verifier.verify(server_public_key).await
    }
}
