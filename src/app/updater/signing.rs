//! The trust anchor is compiled into the application, never supplied by a mirror.
use anyhow::{Context, ensure};
use ring::signature::{ED25519, UnparsedPublicKey};

pub(super) const PUBLIC_KEY: Option<&str> = option_env!("TINY_SHELL_UPDATE_PUBLIC_KEY");

pub(super) fn verify(bytes: &[u8], signature: &[u8], public_key: &str) -> anyhow::Result<()> {
    let key = hex::decode(public_key.trim()).context("invalid update public key encoding")?;
    let signature = hex::decode(std::str::from_utf8(signature)?.trim())
        .context("invalid update signature encoding")?;
    ensure!(
        key.len() == 32 && signature.len() == 64,
        "invalid update signature or key length"
    );
    UnparsedPublicKey::new(&ED25519, key)
        .verify(bytes, &signature)
        .map_err(|_| anyhow::anyhow!("update publisher signature verification failed"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    #[test]
    fn signature_covers_exact_bytes_and_rejects_wrong_key() {
        let key = Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
        let public = hex::encode(key.public_key().as_ref());
        let signature = hex::encode(key.sign(b"manifest\n").as_ref());
        verify(b"manifest\n", signature.as_bytes(), &public).unwrap();
        assert!(verify(b"manifest", signature.as_bytes(), &public).is_err());
        assert!(verify(b"manifest\n", signature.as_bytes(), &"00".repeat(32)).is_err());
        assert!(verify(b"manifest\n", b"invalid", &public).is_err());
    }
}
