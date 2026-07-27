//! 共享的加密原语。
//!
//! 这个模块把原本在 `src/sync/mod.rs` 和 `src/session/config.rs` 中重复的
//! `Argon2id + XChaCha20Poly1305` 组合抽到一处，避免两份几乎一致的实现各自漂移。
//! 同时提供字段级加密（`encrypt_field` / `decrypt_field`）和与硬件 UUID 绑定的
//! 加密（`seal_with_hardware_key` / `open_with_hardware_key`）。

use anyhow::{Context, Result, anyhow};
use argon2::{
    Argon2, PasswordHash, PasswordHasher as _, PasswordVerifier as _, password_hash::SaltString,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit},
};
use rand::{RngCore, rngs::OsRng};

/// 字段级密文的前缀，用于在上传/下载时区分密文与明文/脱敏占位。
pub const FIELD_CIPHER_PREFIX: &str = "v1:";

const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 24;
const KEY_LEN: usize = 32;

/// 用 Argon2id 从用户口令派生 32 字节密钥。
pub fn derive_key(password: &str, salt: &[u8]) -> Result<[u8; KEY_LEN]> {
    let mut key = [0u8; KEY_LEN];
    Argon2::default()
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|err| anyhow!("derive encryption key: {err}"))?;
    Ok(key)
}

/// 生成可同步的隐私密码校验值。
///
/// 返回标准 PHC 字符串，只能用于验证输入是否一致，不能恢复原密码。
pub fn hash_privacy_password(password: &str) -> Result<String> {
    let mut salt = [0u8; SALT_LEN];
    OsRng.fill_bytes(&mut salt);
    let salt = SaltString::encode_b64(&salt)
        .map_err(|err| anyhow!("encode privacy password verifier salt: {err}"))?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|err| anyhow!("hash privacy password verifier: {err}"))
}

/// 验证输入密码是否与同步文件中的不可逆校验值一致。
pub fn verify_privacy_password(password: &str, verifier: &str) -> Result<bool> {
    let parsed = PasswordHash::new(verifier)
        .map_err(|err| anyhow!("parse privacy password verifier: {err}"))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

/// 加密单个字符串字段，返回自包含的密文。
///
/// 格式：`v1:{base64(salt)}:{base64(nonce)}:{base64(ciphertext)}`
/// 每个字段独立生成 salt 与 nonce，避免跨字段信息关联。
/// 空串输入会原样返回空串（脱敏标记），不进入加密路径。
pub fn encrypt_field(plaintext: &str, password: &str) -> Result<String> {
    if plaintext.is_empty() {
        return Ok(String::new());
    }
    let mut salt = [0u8; SALT_LEN];
    let mut nonce = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut nonce);

    let key = derive_key(password, &salt)?;
    let ciphertext = XChaCha20Poly1305::new((&key).into())
        .encrypt(XNonce::from_slice(&nonce), plaintext.as_bytes())
        .map_err(|_| anyhow!("encrypt field"))?;

    Ok(format!(
        "{FIELD_CIPHER_PREFIX}{}:{}:{}",
        STANDARD.encode(salt),
        STANDARD.encode(nonce),
        STANDARD.encode(ciphertext),
    ))
}

/// 解密 `encrypt_field` 产生的密文。
///
/// - 空串返回空串（脱敏占位，调用方负责保留本地原值）。
/// - 不以 `v1:` 开头的输入返回错误，调用方可据此判断是否为密文。
/// - 格式错误或密码错误时返回 `Err`。
pub fn decrypt_field(ciphertext: &str, password: &str) -> Result<String> {
    if ciphertext.is_empty() {
        return Ok(String::new());
    }
    let body = ciphertext
        .strip_prefix(FIELD_CIPHER_PREFIX)
        .ok_or_else(|| anyhow!("not a sealed field (missing v1: prefix)"))?;
    let mut parts = body.splitn(3, ':');
    let salt_b64 = parts.next().ok_or_else(|| anyhow!("missing salt"))?;
    let nonce_b64 = parts.next().ok_or_else(|| anyhow!("missing nonce"))?;
    let ct_b64 = parts.next().ok_or_else(|| anyhow!("missing ciphertext"))?;

    let salt = STANDARD
        .decode(salt_b64)
        .context("decode sealed field salt")?;
    let nonce = STANDARD
        .decode(nonce_b64)
        .context("decode sealed field nonce")?;
    if nonce.len() != NONCE_LEN {
        return Err(anyhow!("invalid sealed field nonce length"));
    }
    let ct = STANDARD
        .decode(ct_b64)
        .context("decode sealed field ciphertext")?;

    let key = derive_key(password, &salt)?;
    let plaintext = XChaCha20Poly1305::new((&key).into())
        .decrypt(XNonce::from_slice(&nonce), ct.as_ref())
        .map_err(|_| anyhow!("cannot decrypt field; password mismatch or corrupted data"))?;
    String::from_utf8(plaintext).context("decrypted field is not valid UTF-8")
}

/// 判断字符串是否为 `encrypt_field` 产生的密文。
/// 下载合并时用于决定是解密覆盖、保留本地，还是当作明文处理。
pub fn is_sealed_field(value: &str) -> bool {
    value.starts_with(FIELD_CIPHER_PREFIX)
}

/// 用硬件 UUID 绑定的密钥加密一段数据。
///
/// 用于把"隐私信息加密密码"本身持久化到 ConfigFile：
/// 落盘的是密文，与 `encrypt_config` 同源（依赖本机硬件 UUID），
/// 因此换设备后无法解出，需用户重新输入。
#[cfg_attr(
    not(any(target_os = "windows", target_os = "macos", target_os = "linux")),
    allow(dead_code)
)]
pub fn seal_with_hardware_key(plaintext: &str, hardware_uuid: &str) -> Result<String> {
    if plaintext.is_empty() {
        return Ok(String::new());
    }
    encrypt_field(plaintext, hardware_uuid)
}

/// 与 `seal_with_hardware_key` 对应，解出硬件绑定的密文。
#[cfg_attr(
    not(any(target_os = "windows", target_os = "macos", target_os = "linux")),
    allow(dead_code)
)]
pub fn open_with_hardware_key(ciphertext: &str, hardware_uuid: &str) -> Result<String> {
    if ciphertext.is_empty() {
        return Ok(String::new());
    }
    decrypt_field(ciphertext, hardware_uuid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privacy_password_verifier_accepts_only_the_original_password() {
        let verifier = hash_privacy_password("privacy-password").unwrap();

        assert!(verify_privacy_password("privacy-password", &verifier).unwrap());
        assert!(!verify_privacy_password("wrong-password", &verifier).unwrap());
        assert!(!verifier.contains("privacy-password"));
    }

    #[test]
    fn encrypt_field_round_trip() {
        let plaintext = "s3cret-p@ssword";
        let sealed = encrypt_field(plaintext, "user-password").unwrap();
        assert!(is_sealed_field(&sealed));
        let opened = decrypt_field(&sealed, "user-password").unwrap();
        assert_eq!(opened, plaintext);
    }

    #[test]
    fn encrypt_field_uses_unique_salt_and_nonce() {
        let sealed_a = encrypt_field("same", "pw").unwrap();
        let sealed_b = encrypt_field("same", "pw").unwrap();
        // 同明文同密码，密文也应不同（独立 salt+nonce）。
        assert_ne!(sealed_a, sealed_b);
    }

    #[test]
    fn wrong_password_is_rejected() {
        let sealed = encrypt_field("data", "correct").unwrap();
        assert!(decrypt_field(&sealed, "wrong").is_err());
    }

    #[test]
    fn empty_string_round_trips() {
        assert_eq!(encrypt_field("", "pw").unwrap(), "");
        assert_eq!(decrypt_field("", "pw").unwrap(), "");
    }

    #[test]
    fn corrupted_ciphertext_fails() {
        let sealed = encrypt_field("data", "pw").unwrap();
        // 篡改最后一段
        let mut parts: Vec<&str> = sealed.split(':').collect();
        parts[3] = "AAAA";
        let tampered = parts.join(":");
        assert!(decrypt_field(&tampered, "pw").is_err());
    }

    #[test]
    fn hardware_key_round_trip() {
        let hw = "test-hardware-uuid";
        let sealed = seal_with_hardware_key("privacy-pw", hw).unwrap();
        assert!(is_sealed_field(&sealed));
        let opened = open_with_hardware_key(&sealed, hw).unwrap();
        assert_eq!(opened, "privacy-pw");
    }

    #[test]
    fn hardware_key_wrong_device_fails() {
        let sealed = seal_with_hardware_key("privacy-pw", "device-a").unwrap();
        assert!(open_with_hardware_key(&sealed, "device-b").is_err());
    }

    #[test]
    fn is_sealed_field_detects_prefix() {
        assert!(!is_sealed_field(""));
        assert!(!is_sealed_field("plaintext"));
        assert!(!is_sealed_field("v2:foo"));
        let sealed = encrypt_field("x", "pw").unwrap();
        assert!(is_sealed_field(&sealed));
    }
}
