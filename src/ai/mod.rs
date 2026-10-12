//! Model traffic and local conversation data; never reads a human terminal.
mod model;
mod provider;
mod service;
mod storage;
pub(crate) use model::*;
pub(crate) use service::{Service, Snapshot};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_remote_http_and_embedded_credentials() {
        let mut provider = Provider {
            base_url: "http://example.com/v1".into(),
            ..Provider::default()
        };
        assert!(provider.validate().is_err());
        provider.base_url = "https://secret@example.com/v1".into();
        assert!(provider.validate().is_err());
        provider.base_url = "http://127.0.0.1:1234/v1".into();
        assert!(provider.validate().is_err());
        provider.allow_http = true;
        assert!(provider.validate().is_ok());
    }
    #[test]
    fn history_encryption_binds_identity_and_detects_changes() {
        let key = [42; 32];
        let sealed = storage::seal(&key, "one", b"private conversation").unwrap();
        assert!(!String::from_utf8_lossy(&sealed).contains("private conversation"));
        assert_eq!(
            storage::unseal(&key, "one", &sealed).unwrap(),
            b"private conversation"
        );
        assert!(storage::unseal(&key, "two", &sealed).is_err());
        let mut corrupt = sealed;
        *corrupt.last_mut().unwrap() ^= 1;
        assert!(storage::unseal(&key, "one", &corrupt).is_err());
    }
}
