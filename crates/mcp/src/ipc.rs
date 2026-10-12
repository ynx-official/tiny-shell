use crate::{
    contracts::{Call, Failure, MAX_FRAME},
    storage::Store,
};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use rand::RngCore;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
#[cfg(windows)]
use sha2::Digest;
use sha2::Sha256;
use std::{path::Path, sync::Arc, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use uuid::Uuid;

#[async_trait]
pub trait Handler: Send + Sync {
    fn generation(&self) -> Option<String> {
        None
    }
    async fn call(&self, client: &str, call: Call) -> crate::contracts::Result<Value>;
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Challenge {
    pub version: u32,
    pub generation: String,
    pub nonce: String,
}
impl Challenge {
    pub fn new(generation: &str) -> Self {
        let mut bytes = [0; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        Self {
            version: 1,
            generation: generation.to_owned(),
            nonce: hex::encode(bytes),
        }
    }
    fn payload(&self, id: &str) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(&(
            self.version,
            &self.generation,
            &self.nonce,
            id,
        ))?)
    }
    pub fn proof(&self, id: &str, secret: &str) -> Result<String> {
        let mut mac = Hmac::<Sha256>::new_from_slice(&hex::decode(secret)?)?;
        mac.update(&self.payload(id)?);
        Ok(hex::encode(mac.finalize().into_bytes()))
    }
    pub fn verify(&self, id: &str, secret: &str, proof: &str) -> Result<()> {
        let mut mac = Hmac::<Sha256>::new_from_slice(&hex::decode(secret)?)?;
        mac.update(&self.payload(id)?);
        mac.verify_slice(&hex::decode(proof)?)?;
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Authentication {
    client_id: String,
    proof: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    generation: String,
    tool: String,
    arguments: Value,
}

pub async fn read_frame<R: AsyncRead + Unpin, T: DeserializeOwned>(reader: &mut R) -> Result<T> {
    let length = reader.read_u32().await? as usize;
    if length == 0 || length > MAX_FRAME {
        bail!("invalid IPC frame size");
    }
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes).await?;
    Ok(serde_json::from_slice(&bytes)?)
}
pub async fn write_frame<W: AsyncWrite + Unpin, T: Serialize>(
    writer: &mut W,
    value: &T,
) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.is_empty() || bytes.len() > MAX_FRAME {
        bail!("IPC frame exceeds limit");
    }
    writer.write_u32(bytes.len() as u32).await?;
    writer.write_all(&bytes).await?;
    writer.flush().await?;
    Ok(())
}
pub async fn serve_stream<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
    store: Store,
    generation: String,
    handler: Arc<dyn Handler>,
) -> Result<()> {
    let client = tokio::time::timeout(Duration::from_secs(5), async {
        let challenge = Challenge::new(&generation);
        write_frame(&mut stream, &challenge).await?;
        let auth: Authentication = read_frame(&mut stream).await?;
        let client = store.client(&auth.client_id)?;
        if !client.enabled {
            bail!("registration revoked");
        }
        challenge.verify(&client.id, &client.secret, &auth.proof)?;
        Ok::<_, anyhow::Error>(client)
    })
    .await??;
    let response = tokio::time::timeout(Duration::from_secs(15), async {
        let request: Request = read_frame(&mut stream).await?;
        let data = if request.generation != generation {
            Failure::new("INSTANCE_CHANGED").envelope()
        } else if !store.client(&client.id).is_ok_and(|c| c.enabled) {
            Failure::new("CLIENT_REVOKED").envelope()
        } else {
            match Call::parse(&request.tool, request.arguments) {
                Ok(call) => match handler.call(&client.id, call).await {
                    Ok(value) => crate::contracts::success(value),
                    Err(e) => e.envelope(),
                },
                Err(e) => e.envelope(),
            }
        };
        Ok::<_, anyhow::Error>(data)
    })
    .await??;
    let response = if serde_json::to_vec(&response)?.len() > MAX_FRAME {
        Failure::new("OUTPUT_LIMIT").envelope()
    } else {
        response
    };
    write_frame(&mut stream, &response).await?;
    stream.shutdown().await?;
    Ok(())
}
pub fn endpoint(config_dir: &Path) -> Result<String> {
    #[cfg(windows)]
    {
        let absolute = std::fs::canonicalize(config_dir)?;
        let path = absolute.to_string_lossy().to_lowercase();
        Ok(format!(
            r"\\.\pipe\tiny-shell-mcp-{}",
            hex::encode(Sha256::digest(path.as_bytes()))
        ))
    }
    #[cfg(unix)]
    {
        let _ = std::marker::PhantomData::<Sha256>;
        let absolute = std::fs::canonicalize(config_dir)?;
        Ok(absolute
            .join("mcp/bridge.sock")
            .to_string_lossy()
            .into_owned())
    }
}
pub struct Listener {
    #[cfg(windows)]
    pipe: tokio::net::windows::named_pipe::NamedPipeServer,
    #[cfg(windows)]
    name: String,
    #[cfg(unix)]
    socket: tokio::net::UnixListener,
    #[cfg(unix)]
    name: String,
}
#[cfg(windows)]
type Stream = tokio::net::windows::named_pipe::NamedPipeServer;
#[cfg(unix)]
type Stream = tokio::net::UnixStream;
impl Listener {
    pub fn bind(config_dir: &Path) -> Result<Self> {
        let name = endpoint(config_dir)?;
        #[cfg(windows)]
        {
            Ok(Self {
                pipe: create_pipe(&name, true)?,
                name,
            })
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{FileTypeExt, MetadataExt};
            if let Ok(metadata) = std::fs::symlink_metadata(&name) {
                // Only reclaim a stale socket owned by this user; a live listener remains exclusive.
                if !metadata.file_type().is_socket() || metadata.uid() != unsafe { libc::geteuid() }
                {
                    bail!("unsafe IPC endpoint");
                }
                match std::os::unix::net::UnixStream::connect(&name) {
                    Ok(_) => bail!("IPC endpoint already active"),
                    Err(error) if error.kind() == std::io::ErrorKind::ConnectionRefused => {
                        std::fs::remove_file(&name)?
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            let socket = tokio::net::UnixListener::bind(&name)?;
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&name, std::fs::Permissions::from_mode(0o600))?;
            Ok(Self { socket, name })
        }
    }
    async fn accept(&mut self) -> Result<Stream> {
        #[cfg(windows)]
        {
            self.pipe.connect().await?;
            let next = create_pipe(&self.name, false)?;
            Ok(std::mem::replace(&mut self.pipe, next))
        }
        #[cfg(unix)]
        {
            Ok(self.socket.accept().await?.0)
        }
    }
    pub async fn run(mut self, store: Store, handler: Arc<dyn Handler>) -> Result<()> {
        let generation = handler
            .generation()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let slots = Arc::new(tokio::sync::Semaphore::new(16));
        loop {
            let stream = self.accept().await?;
            let permit = match slots.clone().try_acquire_owned() {
                Ok(p) => p,
                Err(_) => {
                    drop(stream);
                    continue;
                }
            };
            let store = store.clone();
            let handler = handler.clone();
            let generation = generation.clone();
            tokio::spawn(async move {
                let _permit = permit;
                let _ = serve_stream(stream, store, generation, handler).await;
            });
        }
    }
}
#[cfg(unix)]
impl Drop for Listener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.name);
    }
}
#[cfg(windows)]
fn create_pipe(
    name: &str,
    first: bool,
) -> Result<tokio::net::windows::named_pipe::NamedPipeServer> {
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    let descriptor = crate::storage::WindowsDescriptor::current_user()?;
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    // Tokio/Win32 copies the security descriptor during creation; the DACL grants only the current user.
    let pipe = unsafe {
        tokio::net::windows::named_pipe::ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .create_with_security_attributes_raw(
                name,
                (&mut attributes as *mut SECURITY_ATTRIBUTES).cast(),
            )
    }?;
    Ok(pipe)
}
pub async fn invoke(
    config_dir: &Path,
    id: &str,
    tool: &str,
    arguments: Value,
) -> crate::contracts::Result<Value> {
    let store = Store::new(config_dir).map_err(|_| Failure::new("STORAGE_ERROR"))?;
    let client = store
        .client(id)
        .map_err(|_| Failure::new("CLIENT_REVOKED"))?;
    if !client.enabled {
        return Err(Failure::new("CLIENT_REVOKED"));
    }
    if serde_json::to_vec(&arguments).map_or(true, |bytes| bytes.len() > MAX_FRAME - 1024) {
        return Err(Failure::new("INPUT_LIMIT"));
    }
    let operation = async {
        #[cfg(windows)]
        let mut stream =
            tokio::net::windows::named_pipe::ClientOptions::new().open(endpoint(config_dir)?)?;
        #[cfg(unix)]
        let mut stream = tokio::net::UnixStream::connect(endpoint(config_dir)?).await?;
        let challenge: Challenge = read_frame(&mut stream).await?;
        if challenge.version != 1
            || challenge.nonce.len() != 64
            || Uuid::parse_str(&challenge.generation).is_err()
        {
            bail!("invalid challenge");
        }
        let authentication = Authentication {
            client_id: id.to_owned(),
            proof: challenge.proof(id, &client.secret)?,
        };
        write_frame(&mut stream, &authentication).await?;
        write_frame(
            &mut stream,
            &Request {
                generation: challenge.generation,
                tool: tool.to_owned(),
                arguments,
            },
        )
        .await?;
        read_frame::<_, Value>(&mut stream).await
    };
    tokio::time::timeout(Duration::from_secs(15), operation)
        .await
        .context("IPC deadline")
        .and_then(|r| r)
        .map_err(|_| Failure::new("APP_NOT_RUNNING"))
}
