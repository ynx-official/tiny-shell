use std::sync::OnceLock;

use anyhow::{Context, Result};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit},
};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};

use crate::session::config_file::ConfigFile;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EncryptedConfigEnvelope {
    format_version: u32,
    kdf: String,
    cipher: String,
    salt: String,
    nonce: String,
    payload: String,
}

static HARDWARE_UUID: OnceLock<String> = OnceLock::new();

fn get_hardware_uuid() -> String {
    HARDWARE_UUID.get_or_init(query_hardware_uuid).clone()
}

/// 对外暴露的硬件 UUID，供 `crypto` 模块做硬件绑定加密使用。
pub(crate) fn hardware_uuid() -> String {
    get_hardware_uuid()
}

fn query_hardware_uuid() -> String {
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("ioreg")
            .args(["-rd1", "-c", "IOPlatformExpertDevice"])
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.contains("IOPlatformUUID") {
                    if let Some(uuid) = line.split('"').nth(3) {
                        let uuid = uuid.trim().to_string();
                        if !uuid.is_empty() {
                            return uuid;
                        }
                    }
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(uuid) = std::fs::read_to_string("/sys/class/dmi/id/product_uuid") {
            let uuid = uuid.trim().to_string();
            if !uuid.is_empty() {
                return uuid;
            }
        }
        if let Ok(id) = std::fs::read_to_string("/etc/machine-id") {
            let id = id.trim().to_string();
            if !id.is_empty() {
                return id;
            }
        }
        if let Ok(id) = std::fs::read_to_string("/var/lib/dbus/machine-id") {
            let id = id.trim().to_string();
            if !id.is_empty() {
                return id;
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        use winreg::{RegKey, enums::HKEY_LOCAL_MACHINE};

        if let Ok(key) =
            RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey("SOFTWARE\\Microsoft\\Cryptography")
            && let Ok(guid) = key.get_value::<String, _>("MachineGuid")
        {
            let guid = guid.trim().to_string();
            if !guid.is_empty() {
                return guid;
            }
        }
    }

    "tiny-shell-default-hardware-uuid-fallback".to_string()
}

pub(crate) fn encrypt_config(config: &ConfigFile, password: &str) -> Result<Vec<u8>> {
    let mut salt = [0u8; 16];
    let mut nonce = [0u8; 24];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut nonce);

    let key = crate::crypto::derive_key(password, &salt)?;
    let plaintext = serde_json::to_vec(config).context("serialize config")?;
    let ciphertext = XChaCha20Poly1305::new((&key).into())
        .encrypt(XNonce::from_slice(&nonce), plaintext.as_ref())
        .map_err(|_| anyhow::anyhow!("encrypt config payload"))?;

    serde_json::to_vec_pretty(&EncryptedConfigEnvelope {
        format_version: 1,
        kdf: "argon2id".to_string(),
        cipher: "xchacha20poly1305".to_string(),
        salt: STANDARD.encode(salt),
        nonce: STANDARD.encode(nonce),
        payload: STANDARD.encode(ciphertext),
    })
    .context("serialize encrypted config envelope")
}

pub(crate) fn decrypt_config(raw: &[u8], password: &str) -> Result<ConfigFile> {
    let envelope: EncryptedConfigEnvelope =
        serde_json::from_slice(raw).context("parse encrypted config envelope")?;
    if envelope.format_version != 1
        || envelope.kdf != "argon2id"
        || envelope.cipher != "xchacha20poly1305"
    {
        return Err(anyhow::anyhow!("unsupported encrypted config format"));
    }
    let salt = STANDARD
        .decode(envelope.salt)
        .context("decode config salt")?;
    let nonce = STANDARD
        .decode(envelope.nonce)
        .context("decode config nonce")?;
    if nonce.len() != 24 {
        return Err(anyhow::anyhow!("invalid config nonce"));
    }
    let ciphertext = STANDARD
        .decode(envelope.payload)
        .context("decode encrypted config payload")?;

    let key = crate::crypto::derive_key(password, &salt)?;
    let plaintext = XChaCha20Poly1305::new((&key).into())
        .decrypt(XNonce::from_slice(&nonce), ciphertext.as_ref())
        .map_err(|_| {
            anyhow::anyhow!("cannot decrypt config; hardware UUID mismatch or corrupted data")
        })?;

    serde_json::from_slice(&plaintext).context("parse decrypted config")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_hardware_uuid() {
        let uuid = get_hardware_uuid();
        assert!(!uuid.is_empty());
    }

    #[test]
    fn test_config_encryption_roundtrip() {
        let config = ConfigFile::default();
        let password = "test-password-123";
        let encrypted = encrypt_config(&config, password).unwrap();

        // Ensure it doesn't contain plain text fields of default config
        let encrypted_str = String::from_utf8_lossy(&encrypted);
        assert!(!encrypted_str.contains("Maple Mono NF CN"));
        assert!(encrypted_str.contains("argon2id"));

        let decrypted = decrypt_config(&encrypted, password).unwrap();
        assert_eq!(decrypted.terminal_font_family, config.terminal_font_family);

        // Decrypt with wrong password should fail
        assert!(decrypt_config(&encrypted, "wrong-password").is_err());
    }
}
