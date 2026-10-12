use crate::policy::Policy;
use anyhow::{Context, Result, bail};
use rand::RngCore;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registration {
    pub schema_version: u32,
    pub id: String,
    pub label: String,
    pub enabled: bool,
    pub secret: String,
}
#[derive(Clone)]
pub struct Store {
    root: PathBuf,
    audit_lock: std::sync::Arc<std::sync::Mutex<()>>,
}
impl Store {
    pub fn new(config_dir: &Path) -> Result<Self> {
        let root = config_dir.join("mcp");
        private_directory(&root)?;
        private_directory(&root.join("clients"))?;
        Ok(Self {
            root,
            audit_lock: std::sync::Arc::new(std::sync::Mutex::new(())),
        })
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn audit(&self, record: serde_json::Value) -> Result<()> {
        self.update_audit(Some(record))
    }
    pub fn prune_audit(&self) -> Result<()> {
        self.update_audit(None)
    }
    pub fn next_audit_expiry(&self) -> Result<std::time::Duration> {
        use std::io::Read;
        let path = self.root.join("audit.jsonl");
        reject_link(&path)?;
        if !path.exists() {
            return Ok(std::time::Duration::from_secs(86400));
        }
        let mut bytes = Vec::new();
        fs::File::open(path)?.take(4096).read_to_end(&mut bytes)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        let oldest = String::from_utf8_lossy(&bytes).lines().find_map(|line| {
            serde_json::from_str::<serde_json::Value>(line)
                .ok()
                .and_then(|v| v["timestamp"].as_u64())
        });
        Ok(std::time::Duration::from_secs(
            oldest
                .map(|at| {
                    at.saturating_add(7 * 86400)
                        .saturating_add(1)
                        .saturating_sub(now)
                })
                .unwrap_or(86400)
                .max(1),
        ))
    }
    fn update_audit(&self, record: Option<serde_json::Value>) -> Result<()> {
        let _guard = self
            .audit_lock
            .lock()
            .map_err(|_| anyhow::anyhow!("MCP audit lock poisoned"))?;
        let path = self.root.join("audit.jsonl");
        reject_link(&path)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        let mut records = if path.exists() {
            let bytes = fs::read(&path)?;
            if bytes.len() > 10 * 1024 * 1024 {
                bail!("MCP audit exceeds limit");
            }
            String::from_utf8(bytes)?
                .lines()
                .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                .filter(|value| {
                    value["timestamp"]
                        .as_u64()
                        .is_some_and(|at| now.saturating_sub(at) <= 7 * 86400)
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        if let Some(record) = record {
            records.push(record);
        }
        let mut bytes = records
            .iter()
            .map(|value| format!("{value}\n"))
            .collect::<String>()
            .into_bytes();
        while bytes.len() > 10 * 1024 * 1024 {
            records.remove(0);
            bytes = records
                .iter()
                .map(|value| format!("{value}\n"))
                .collect::<String>()
                .into_bytes();
        }
        atomic_bytes(&path, &bytes)
    }
    pub fn clear_audit(&self) -> Result<()> {
        let _guard = self
            .audit_lock
            .lock()
            .map_err(|_| anyhow::anyhow!("MCP audit lock poisoned"))?;
        atomic_bytes(&self.root.join("audit.jsonl"), b"")
    }
    pub fn audit_tail(&self) -> Result<Vec<serde_json::Value>> {
        use std::io::{Read, Seek, SeekFrom};
        let path = self.root.join("audit.jsonl");
        reject_link(&path)?;
        if !path.exists() {
            return Ok(Vec::new());
        }
        let mut file = fs::File::open(path)?;
        let size = file.metadata()?.len();
        file.seek(SeekFrom::Start(size.saturating_sub(64 * 1024)))?;
        let mut bytes = Vec::new();
        file.take(64 * 1024).read_to_end(&mut bytes)?;
        Ok(String::from_utf8_lossy(&bytes)
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .take(20)
            .collect())
    }
    pub fn load_policy(&self) -> Result<Policy> {
        let path = self.root.join("policy.json");
        if !path.exists() {
            return Ok(Policy::default());
        }
        let policy: Policy = read_json(&path)?;
        if policy.schema_version != 1 {
            bail!("unsupported MCP policy version");
        }
        Ok(policy)
    }
    pub fn save_policy(&self, policy: &Policy) -> Result<()> {
        atomic_json(&self.root.join("policy.json"), policy)
    }
    pub fn register(&self, label: &str) -> Result<Registration> {
        if label.is_empty() || label.len() > 128 {
            bail!("invalid client label");
        }
        let mut bytes = [0; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        let client = Registration {
            schema_version: 1,
            id: Uuid::new_v4().to_string(),
            label: label.to_owned(),
            enabled: true,
            secret: hex::encode(bytes),
        };
        atomic_json(&self.client_path(&client.id)?, &client)?;
        Ok(client)
    }
    fn client_path(&self, id: &str) -> Result<PathBuf> {
        let id = Uuid::parse_str(id).context("invalid registration ID")?;
        Ok(self.root.join("clients").join(format!("{id}.json")))
    }
    pub fn client(&self, id: &str) -> Result<Registration> {
        let client: Registration = read_json(&self.client_path(id)?)?;
        if client.schema_version != 1 || client.id != id || hex::decode(&client.secret)?.len() != 32
        {
            bail!("invalid client registration");
        }
        Ok(client)
    }
    pub fn clients(&self) -> Result<Vec<Registration>> {
        let mut clients = Vec::new();
        for entry in fs::read_dir(self.root.join("clients"))? {
            let path = entry?.path();
            if path.extension().is_some_and(|s| s == "json") {
                let id = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .context("invalid registration filename")?;
                clients.push(self.client(id)?);
            }
        }
        clients.sort_by(|a, b| a.label.cmp(&b.label));
        Ok(clients)
    }
    pub fn revoke(&self, id: &str) -> Result<()> {
        let mut client = self.client(id)?;
        client.enabled = false;
        atomic_json(&self.client_path(id)?, &client)
    }
}
pub fn default_config_dir() -> Result<PathBuf> {
    Ok(directories::BaseDirs::new()
        .context("home directory unavailable")?
        .home_dir()
        .join(".config/tiny-shell"))
}
fn reject_link(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => bail!("MCP storage cannot be a symbolic link"),
        Ok(meta) => {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if meta.file_attributes() & 0x400 != 0 {
                    bail!("MCP storage cannot be a reparse point");
                }
            }
            #[cfg(not(windows))]
            let _ = meta;
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
pub fn private_directory(path: &Path) -> Result<()> {
    reject_link(path)?;
    fs::create_dir_all(path)?;
    secure(path, true)
}
fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    reject_link(path)?;
    if fs::metadata(path)?.len() > 1024 * 1024 {
        bail!("MCP configuration too large");
    }
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
fn atomic_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    atomic_bytes(path, &serde_json::to_vec_pretty(value)?)
}
fn atomic_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    reject_link(path)?;
    let temp = path.with_extension(format!("{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        secure(&temp, false)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        #[cfg(unix)]
        fs::rename(&temp, path)?;
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            use windows_sys::Win32::Storage::FileSystem::{
                MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
            };
            let from: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
            let to: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            // Both paths are live, NUL-terminated wide strings; move preserves the restricted DACL.
            if unsafe {
                MoveFileExW(
                    from.as_ptr(),
                    to.as_ptr(),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            } == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}
#[cfg(unix)]
fn secure(path: &Path, directory: bool) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(
        path,
        fs::Permissions::from_mode(if directory { 0o700 } else { 0o600 }),
    )?;
    Ok(())
}
#[cfg(windows)]
fn secure(path: &Path, _directory: bool) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Security::{
        DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, SetFileSecurityW,
    };
    let descriptor = WindowsDescriptor::current_user()?;
    let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SetFileSecurity copies the descriptor; buffers remain live through the call.
    if unsafe {
        SetFileSecurityW(
            name.as_ptr(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor.0,
        )
    } == 0
    {
        bail!(std::io::Error::last_os_error());
    }
    Ok(())
}
#[cfg(windows)]
pub(crate) struct WindowsDescriptor(pub windows_sys::Win32::Security::PSECURITY_DESCRIPTOR);
#[cfg(windows)]
impl Drop for WindowsDescriptor {
    fn drop(&mut self) {
        // ConvertStringSecurityDescriptor allocates with LocalAlloc.
        unsafe {
            windows_sys::Win32::Foundation::LocalFree(self.0);
        }
    }
}
#[cfg(windows)]
impl WindowsDescriptor {
    pub fn current_user() -> Result<Self> {
        use std::ptr;
        use windows_sys::Win32::{
            Foundation::{CloseHandle, LocalFree},
            Security::{
                Authorization::{
                    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
                },
                GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser,
            },
            System::Threading::{GetCurrentProcess, OpenProcessToken},
        };
        // Handles and allocation sizes are supplied by Win32; TOKEN_USER is read only after a successful query.
        unsafe {
            let mut token = ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                bail!(std::io::Error::last_os_error());
            }
            let mut size = 0;
            GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut size);
            let mut buffer = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
            let ok = GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                size,
                &mut size,
            );
            CloseHandle(token);
            if ok == 0 {
                bail!(std::io::Error::last_os_error());
            }
            let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
            let mut sid = ptr::null_mut();
            if ConvertSidToStringSidW(user.User.Sid, &mut sid) == 0 {
                bail!(std::io::Error::last_os_error());
            }
            let mut len = 0;
            while *sid.add(len) != 0 {
                len += 1;
            }
            let sid_text = String::from_utf16_lossy(std::slice::from_raw_parts(sid, len));
            LocalFree(sid.cast());
            let sddl: Vec<u16> = format!("D:P(A;OICI;GA;;;{sid_text})")
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let mut descriptor = ptr::null_mut();
            if ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                ptr::null_mut(),
            ) == 0
            {
                bail!(std::io::Error::last_os_error());
            }
            Ok(Self(descriptor))
        }
    }
}
