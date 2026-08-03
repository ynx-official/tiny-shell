use async_trait::async_trait;
use russh::client::Handler;

#[derive(Clone)]
pub(crate) struct SftpClientHandler;

#[async_trait]
impl Handler for SftpClientHandler {
    type Error = anyhow::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}
