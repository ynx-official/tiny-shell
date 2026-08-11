//! Shared SSH/SFTP host-key verification with application-owned OpenSSH storage.
//!
//! TinyShell uses `accept-new` TOFU semantics: a first-seen key is recorded,
//! while any later key mismatch is rejected before authentication begins.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    net::IpAddr,
    path::{Path, PathBuf},
    sync::Mutex,
    thread,
    time::{Duration, Instant, SystemTime},
};

use anyhow::{Context, Result, anyhow, bail};
use directories::BaseDirs;
use hmac::{Hmac, Mac};
use rust_i18n::t;
use sha1::Sha1;
use ssh_key::{
    PublicKey,
    known_hosts::{HostPatterns, KnownHosts, Marker},
};
use uuid::Uuid;

const MAX_KNOWN_HOSTS_BYTES: u64 = 8 * 1024 * 1024;
const LOCK_RETRY_INTERVAL: Duration = Duration::from_millis(25);
const LOCK_WAIT_TIMEOUT: Duration = Duration::from_secs(5);
const STALE_LOCK_AGE: Duration = Duration::from_secs(60);

static KNOWN_HOSTS_MUTEX: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug)]
pub(crate) struct HostKeyVerifier {
    host: String,
    port: u16,
    path: PathBuf,
}

impl HostKeyVerifier {
    pub(crate) fn new(host: &str, port: u16) -> Result<Self> {
        let host = normalize_host(host)
            .ok_or_else(|| anyhow!(t!("ssh_host_key_invalid_host").to_string()))?;
        let base_dirs = BaseDirs::new().ok_or_else(|| {
            anyhow!(
                t!(
                    "ssh_host_key_storage_failed",
                    host = known_host_label(&host, port),
                    error = "user home directory is unavailable"
                )
                .to_string()
            )
        })?;
        let path = base_dirs
            .home_dir()
            .join(".config")
            .join("tiny-shell")
            .join("known_hosts");
        Ok(Self { host, port, path })
    }

    pub(crate) async fn verify(&self, server_public_key: &PublicKey) -> Result<bool> {
        let verifier = self.clone();
        let public_key = server_public_key.clone();
        let fingerprint = server_public_key
            .fingerprint(Default::default())
            .to_string();
        let decision = tokio::task::spawn_blocking(move || verifier.verify_blocking(&public_key))
            .await
            .map_err(|error| self.storage_error(format!("verification worker stopped: {error}")))?
            .map_err(|error| self.storage_error(format!("{error:#}")))?;

        match decision {
            KnownHostDecision::Trusted => Ok(true),
            KnownHostDecision::Added => {
                tracing::info!(
                    host = %self.host_label(),
                    %fingerprint,
                    path = %self.path.display(),
                    "recorded newly observed SSH host key"
                );
                Ok(true)
            }
            KnownHostDecision::Changed { line } => Err(anyhow!(
                t!(
                    "ssh_host_key_changed",
                    host = self.host_label(),
                    line = line,
                    fingerprint = fingerprint,
                    path = self.path.display().to_string()
                )
                .to_string()
            )),
            KnownHostDecision::Revoked { line } => Err(anyhow!(
                t!(
                    "ssh_host_key_revoked",
                    host = self.host_label(),
                    line = line,
                    fingerprint = fingerprint,
                    path = self.path.display().to_string()
                )
                .to_string()
            )),
            // `verify_blocking` converts an unknown key into `Added` while
            // holding the storage lock. Keep this arm defensive if that
            // invariant ever changes.
            KnownHostDecision::Unknown => Ok(false),
        }
    }

    fn verify_blocking(&self, server_public_key: &PublicKey) -> Result<KnownHostDecision> {
        let process_guard = match KNOWN_HOSTS_MUTEX.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let _process_guard = process_guard;

        prepare_storage_directory(&self.path)?;
        let _file_lock = KnownHostsLock::acquire(&self.path)?;
        let bytes = read_known_hosts(&self.path)?;
        let contents = std::str::from_utf8(&bytes).context("known_hosts is not valid UTF-8")?;
        let host_label = self.host_label();

        match classify_known_hosts(contents, &host_label, server_public_key)? {
            KnownHostDecision::Unknown => {
                let updated = append_known_host(bytes, &host_label, server_public_key)?;
                write_atomically(&self.path, &updated)?;
                Ok(KnownHostDecision::Added)
            }
            decision => Ok(decision),
        }
    }

    fn host_label(&self) -> String {
        known_host_label(&self.host, self.port)
    }

    fn storage_error(&self, error: impl std::fmt::Display) -> anyhow::Error {
        anyhow!(
            t!(
                "ssh_host_key_storage_failed",
                host = self.host_label(),
                error = error.to_string()
            )
            .to_string()
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KnownHostDecision {
    Trusted,
    Unknown,
    Added,
    Changed { line: usize },
    Revoked { line: usize },
}

fn classify_known_hosts(
    contents: &str,
    host_label: &str,
    server_public_key: &PublicKey,
) -> Result<KnownHostDecision> {
    let mut trusted = false;
    let mut first_changed_line = None;

    for (index, line) in contents.lines().enumerate() {
        let line_number = index + 1;
        let Some(entry) = KnownHosts::new(line).next() else {
            continue;
        };
        let entry =
            entry.with_context(|| format!("invalid known_hosts entry at line {line_number}"))?;
        if !host_patterns_match(entry.host_patterns(), host_label) {
            continue;
        }

        let key_matches = entry.public_key().key_data() == server_public_key.key_data();
        match entry.marker() {
            Some(Marker::Revoked) if key_matches => {
                return Ok(KnownHostDecision::Revoked { line: line_number });
            }
            // russh exposes the negotiated public key here, not enough
            // certificate context to validate an OpenSSH CA entry. Treat a
            // matching CA/revocation record as existing trust and fail closed.
            Some(Marker::Revoked | Marker::CertAuthority) => {
                first_changed_line.get_or_insert(line_number);
            }
            None if key_matches => trusted = true,
            None => {
                first_changed_line.get_or_insert(line_number);
            }
        }
    }

    if trusted {
        Ok(KnownHostDecision::Trusted)
    } else if let Some(line) = first_changed_line {
        Ok(KnownHostDecision::Changed { line })
    } else {
        Ok(KnownHostDecision::Unknown)
    }
}

fn host_patterns_match(patterns: &HostPatterns, host_label: &str) -> bool {
    match patterns {
        HostPatterns::Patterns(patterns) => {
            let mut matched_positive = false;
            for pattern in patterns {
                let (negated, pattern) = match pattern.strip_prefix('!') {
                    Some(pattern) => (true, pattern),
                    None => (false, pattern.as_str()),
                };
                if wildcard_match(pattern, host_label) {
                    if negated {
                        return false;
                    }
                    matched_positive = true;
                }
            }
            matched_positive
        }
        HostPatterns::HashedName { salt, hash } => Hmac::<Sha1>::new_from_slice(salt)
            .map(|mut hmac| {
                hmac.update(host_label.as_bytes());
                hmac.verify_slice(hash).is_ok()
            })
            .unwrap_or(false),
    }
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().map(|ch| ch.to_ascii_lowercase()).collect();
    let value: Vec<char> = value.chars().map(|ch| ch.to_ascii_lowercase()).collect();
    let mut previous = vec![false; value.len() + 1];
    previous[0] = true;

    for pattern_char in pattern {
        let mut current = vec![false; value.len() + 1];
        if pattern_char == '*' {
            current[0] = previous[0];
        }
        for (index, value_char) in value.iter().enumerate() {
            current[index + 1] = match pattern_char {
                '*' => current[index] || previous[index + 1],
                '?' => previous[index],
                literal => previous[index] && literal == *value_char,
            };
        }
        previous = current;
    }

    previous[value.len()]
}

fn normalize_host(host: &str) -> Option<String> {
    let host = host.trim();
    let host = host
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(host);
    // These characters have structural meaning in known_hosts. Rejecting them
    // prevents a connection name from injecting additional trust patterns.
    if host.is_empty()
        || host
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace() || ",|*?!#[]".contains(ch))
    {
        return None;
    }

    if let Ok(address) = host.parse::<IpAddr>() {
        return Some(address.to_string());
    }

    let host = host.trim_end_matches('.').to_lowercase();
    (!host.is_empty()).then_some(host)
}

fn known_host_label(host: &str, port: u16) -> String {
    if port == 22 {
        host.to_string()
    } else {
        format!("[{host}]:{port}")
    }
}

fn append_known_host(
    mut contents: Vec<u8>,
    host_label: &str,
    server_public_key: &PublicKey,
) -> Result<Vec<u8>> {
    if !contents.is_empty() && !contents.ends_with(b"\n") {
        contents.push(b'\n');
    }

    let mut public_key = server_public_key.clone();
    public_key.set_comment("");
    let encoded_key = public_key
        .to_openssh()
        .context("could not encode SSH host key")?;
    let entry = format!("{host_label} {encoded_key}\n");
    let updated_len = contents.len().saturating_add(entry.len());
    if updated_len as u64 > MAX_KNOWN_HOSTS_BYTES {
        bail!(
            "known_hosts exceeds the {} byte limit",
            MAX_KNOWN_HOSTS_BYTES
        );
    }
    contents.extend_from_slice(entry.as_bytes());
    Ok(contents)
}

fn prepare_storage_directory(path: &Path) -> Result<()> {
    let parent = path.parent().context("known_hosts path has no parent")?;
    fs::create_dir_all(parent).with_context(|| format!("could not create {}", parent.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
            .with_context(|| format!("could not secure {}", parent.display()))?;
    }

    Ok(())
}

fn read_known_hosts(path: &Path) -> Result<Vec<u8>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                bail!("{} is not a regular file", path.display());
            }
            if metadata.len() > MAX_KNOWN_HOSTS_BYTES {
                bail!(
                    "known_hosts exceeds the {} byte limit",
                    MAX_KNOWN_HOSTS_BYTES
                );
            }

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(path, fs::Permissions::from_mode(0o600))
                    .with_context(|| format!("could not secure {}", path.display()))?;
            }

            fs::read(path).with_context(|| format!("could not read {}", path.display()))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error).with_context(|| format!("could not inspect {}", path.display())),
    }
}

fn write_atomically(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path.parent().context("known_hosts path has no parent")?;
    let temp_path = parent.join(format!(".known_hosts.{}.tmp", Uuid::new_v4()));
    let write_result = (|| -> Result<()> {
        let mut file = create_private_file(&temp_path)
            .with_context(|| format!("could not create {}", temp_path.display()))?;
        file.write_all(contents)
            .with_context(|| format!("could not write {}", temp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("could not sync {}", temp_path.display()))?;
        drop(file);
        // The temporary file lives beside the target so the rename stays on
        // one filesystem and replaces the complete trust snapshot atomically.
        fs::rename(&temp_path, path).with_context(|| {
            format!(
                "could not replace {} with {}",
                path.display(),
                temp_path.display()
            )
        })?;

        #[cfg(unix)]
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .with_context(|| format!("could not sync {}", parent.display()))?;

        Ok(())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    write_result
}

#[cfg(unix)]
fn create_private_file(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;

    OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn create_private_file(path: &Path) -> io::Result<File> {
    OpenOptions::new().create_new(true).write(true).open(path)
}

struct KnownHostsLock {
    path: PathBuf,
    token: String,
}

impl KnownHostsLock {
    fn acquire(known_hosts_path: &Path) -> Result<Self> {
        let path = known_hosts_path.with_extension("lock");
        let token = Uuid::new_v4().to_string();
        let started = Instant::now();

        // The lock file coordinates multiple TinyShell processes. The unique
        // token prevents an expired owner from deleting a successor's lock.
        loop {
            match create_private_file(&path) {
                Ok(mut file) => {
                    if let Err(error) = file
                        .write_all(token.as_bytes())
                        .and_then(|_| file.sync_all())
                    {
                        let _ = fs::remove_file(&path);
                        return Err(error).context("could not initialize known_hosts lock");
                    }
                    return Ok(Self { path, token });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    if lock_is_stale(&path)? {
                        match fs::remove_file(&path) {
                            Ok(()) => continue,
                            Err(remove_error) if remove_error.kind() == io::ErrorKind::NotFound => {
                                continue;
                            }
                            Err(remove_error) => {
                                return Err(remove_error)
                                    .context("could not remove stale known_hosts lock");
                            }
                        }
                    }
                    if started.elapsed() >= LOCK_WAIT_TIMEOUT {
                        bail!("timed out waiting for another known_hosts writer");
                    }
                    thread::sleep(LOCK_RETRY_INTERVAL);
                }
                Err(error) => {
                    return Err(error).context("could not acquire known_hosts lock");
                }
            }
        }
    }
}

impl Drop for KnownHostsLock {
    fn drop(&mut self) {
        let owns_lock = fs::read_to_string(&self.path)
            .map(|token| token == self.token)
            .unwrap_or(false);
        if owns_lock {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn lock_is_stale(path: &Path) -> Result<bool> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("could not inspect {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("{} is not a regular lock file", path.display());
    }
    let modified = metadata.modified().unwrap_or(SystemTime::now());
    Ok(SystemTime::now()
        .duration_since(modified)
        .unwrap_or_default()
        >= STALE_LOCK_AGE)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY_ONE: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ";
    const KEY_TWO: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILIG2T/B0l0gaqj3puu510tu9N1OkQ4znY3LYuEm5zCF";

    fn key(value: &str) -> Result<PublicKey> {
        value.parse().context("test public key is invalid")
    }

    #[test]
    fn classifies_matching_unknown_and_changed_keys() -> Result<()> {
        let key_one = key(KEY_ONE)?;
        assert_eq!(
            classify_known_hosts("", "example.com", &key_one)?,
            KnownHostDecision::Unknown
        );
        assert_eq!(
            classify_known_hosts(&format!("example.com {KEY_ONE}\n"), "example.com", &key_one)?,
            KnownHostDecision::Trusted
        );
        assert_eq!(
            classify_known_hosts(
                &format!("other.example {KEY_ONE}\nexample.com {KEY_TWO}\n"),
                "example.com",
                &key_one
            )?,
            KnownHostDecision::Changed { line: 2 }
        );
        Ok(())
    }

    #[test]
    fn revoked_key_takes_precedence_over_matching_entry() -> Result<()> {
        let key = key(KEY_ONE)?;
        let contents = format!("example.com {KEY_ONE}\n@revoked example.com {KEY_ONE}\n");
        assert_eq!(
            classify_known_hosts(&contents, "example.com", &key)?,
            KnownHostDecision::Revoked { line: 2 }
        );
        Ok(())
    }

    #[test]
    fn supports_hashed_and_wildcard_openssh_hosts() -> Result<()> {
        let key = key(KEY_TWO)?;
        let hashed = "|1|O33ESRMWPVkMYIwJ1Uw+n877jTo=|nuuC5vEqXlEZ/8BXQR7m619W6Ak=";
        assert_eq!(
            classify_known_hosts(&format!("{hashed} {KEY_TWO}\n"), "example.com", &key)?,
            KnownHostDecision::Trusted
        );
        assert_eq!(
            classify_known_hosts(
                &format!("*.example.net,!blocked.example.net {KEY_TWO}\n"),
                "shell.example.net",
                &key
            )?,
            KnownHostDecision::Trusted
        );
        Ok(())
    }

    #[test]
    fn normalizes_hosts_and_formats_nonstandard_ports() {
        assert_eq!(
            normalize_host(" EXAMPLE.COM. ").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            normalize_host("[2001:0db8::1]").as_deref(),
            Some("2001:db8::1")
        );
        assert_eq!(known_host_label("example.com", 22), "example.com");
        assert_eq!(known_host_label("2001:db8::1", 2200), "[2001:db8::1]:2200");
        assert!(normalize_host("example.com\nattacker").is_none());
    }

    #[test]
    fn serialized_entry_drops_untrusted_key_comments() -> Result<()> {
        let mut public_key = key(KEY_ONE)?;
        public_key.set_comment("comment supplied by peer");
        let contents = append_known_host(Vec::new(), "example.com", &public_key)?;
        assert_eq!(contents, format!("example.com {KEY_ONE}\n").as_bytes());
        Ok(())
    }
}
