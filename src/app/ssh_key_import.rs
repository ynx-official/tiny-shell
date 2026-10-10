use crate::session::{config::ManagedKey, ssh_keys::validate_and_inspect};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum KeyImportValidation {
    #[default]
    WaitingForFile,
    Validating,
    Invalid(String),
    Duplicate,
    Valid {
        key_type: String,
        fingerprint: String,
    },
}

impl KeyImportValidation {
    pub(crate) fn can_confirm(&self) -> bool {
        matches!(self, Self::Valid { .. })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum KeyImportSource {
    #[default]
    File,
    Text,
}

#[derive(Default)]
pub(crate) struct KeyImportState {
    pub(crate) source: KeyImportSource,
    pub(crate) open: bool,
    pub(crate) path: String,
    pub(crate) content: String,
    pub(crate) validation: KeyImportValidation,
}

impl KeyImportState {
    pub(crate) fn open(&mut self) {
        *self = Self::default();
        self.open = true;
    }

    pub(crate) fn close(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn switch_source(&mut self, source: KeyImportSource) {
        self.source = source;
        self.path.clear();
        self.content.clear();
        self.validation = KeyImportValidation::WaitingForFile;
    }

    pub(crate) fn set_text(&mut self, content: String, passphrase: &str, keys: &[ManagedKey]) {
        if self.source != KeyImportSource::Text {
            return;
        }
        self.content = content;
        self.validation = KeyImportValidation::WaitingForFile;
        self.revalidate(passphrase, keys);
    }

    pub(crate) fn begin_file_validation(&mut self, path: String) {
        self.path = path;
        self.content.clear();
        self.validation = KeyImportValidation::Validating;
    }

    pub(crate) fn set_file(
        &mut self,
        path: String,
        content: String,
        passphrase: &str,
        managed_keys: &[ManagedKey],
    ) {
        self.path = path;
        self.content = content;
        self.validation = KeyImportValidation::WaitingForFile;
        self.revalidate(passphrase, managed_keys);
    }

    pub(crate) fn set_read_error(&mut self, path: String, error: String) {
        self.path = path;
        self.content.clear();
        self.validation = KeyImportValidation::Invalid(error);
    }

    pub(crate) fn revalidate(&mut self, passphrase: &str, managed_keys: &[ManagedKey]) {
        if matches!(self.validation, KeyImportValidation::Validating) {
            return;
        }
        if (self.source == KeyImportSource::File && self.path.is_empty())
            || (self.source == KeyImportSource::Text && self.content.trim().is_empty())
        {
            self.validation = KeyImportValidation::WaitingForFile;
            return;
        }

        let passphrase = (!passphrase.is_empty()).then_some(passphrase);
        self.validation = match validate_and_inspect(&self.content, passphrase) {
            Ok((key_type, fingerprint)) => {
                if managed_keys
                    .iter()
                    .any(|key| key.fingerprint == fingerprint)
                {
                    KeyImportValidation::Duplicate
                } else {
                    KeyImportValidation::Valid {
                        key_type,
                        fingerprint,
                    }
                }
            }
            Err(err) => KeyImportValidation::Invalid(format!("{err:#}")),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::{KeyImportSource, KeyImportState, KeyImportValidation};

    fn openssh_key() -> ssh_key::PrivateKey {
        ssh_key::PrivateKey::random(&mut rand::thread_rng(), ssh_key::Algorithm::Ed25519).unwrap()
    }

    #[test]
    fn openssh_text_and_file_produce_same_fingerprint() {
        let content = openssh_key()
            .to_openssh(ssh_key::LineEnding::LF)
            .unwrap()
            .to_string();
        let mut state = KeyImportState::default();
        state.open();
        state.begin_file_validation("id_ed25519".into());
        state.set_file("id_ed25519".into(), content.clone(), "", &[]);
        let file_validation = state.validation.clone();
        assert!(file_validation.can_confirm());
        state.switch_source(KeyImportSource::Text);
        assert!(!state.validation.can_confirm());
        assert!(state.content.is_empty());
        state.set_text(content.replace('\n', "\r\n"), "", &[]);
        assert_eq!(state.validation, file_validation);
        assert!(state.path.is_empty());
        state.switch_source(KeyImportSource::File);
        assert!(!state.validation.can_confirm());
        assert!(state.content.is_empty());
    }

    #[test]
    fn text_import_rejects_empty_public_and_incomplete_keys() {
        let mut state = KeyImportState::default();
        state.switch_source(KeyImportSource::Text);
        for content in [
            "",
            "  \n",
            "ssh-ed25519 AAAA",
            "-----BEGIN OPENSSH PRIVATE KEY-----",
        ] {
            state.set_text(content.into(), "", &[]);
            assert!(!state.validation.can_confirm());
        }
    }

    #[test]
    fn encrypted_text_requires_correct_passphrase_and_rejects_duplicates() {
        let key = openssh_key();
        let encrypted = key.encrypt(&mut rand::thread_rng(), "test-only").unwrap();
        let content = encrypted
            .to_openssh(ssh_key::LineEnding::LF)
            .unwrap()
            .to_string();
        let mut state = KeyImportState::default();
        state.switch_source(KeyImportSource::Text);
        state.set_text(content.clone(), "", &[]);
        assert!(!state.validation.can_confirm());
        state.revalidate("incorrect", &[]);
        assert!(!state.validation.can_confirm());
        state.revalidate("test-only", &[]);
        assert!(state.validation.can_confirm());
        let KeyImportValidation::Valid {
            key_type,
            fingerprint,
        } = state.validation.clone()
        else {
            unreachable!()
        };
        let existing = crate::session::config::ManagedKey {
            id: "test".into(),
            name: "test".into(),
            key_type,
            fingerprint,
            inline_content: content,
            passphrase: "test-only".into(),
            created_at: 0,
        };
        state.revalidate("test-only", &[existing]);
        assert_eq!(state.validation, KeyImportValidation::Duplicate);
        state.close();
        assert!(state.content.is_empty());
    }

    #[test]
    fn file_read_completion_leaves_validating_state() {
        let mut state = KeyImportState::default();
        state.open();
        state.begin_file_validation("key".into());
        state.set_file("key".into(), "invalid".into(), "", &[]);
        assert!(matches!(state.validation, KeyImportValidation::Invalid(_)));
    }

    #[test]
    fn only_valid_import_can_be_confirmed() {
        assert!(!KeyImportValidation::WaitingForFile.can_confirm());
        assert!(!KeyImportValidation::Validating.can_confirm());
        assert!(!KeyImportValidation::Duplicate.can_confirm());
        assert!(!KeyImportValidation::Invalid("invalid key".into()).can_confirm());
        assert!(
            KeyImportValidation::Valid {
                key_type: "ed25519".into(),
                fingerprint: "SHA256:test".into(),
            }
            .can_confirm()
        );
    }

    #[test]
    fn closing_import_clears_transient_key_material() {
        let mut state = KeyImportState {
            source: super::KeyImportSource::File,
            open: true,
            path: "id_ed25519".into(),
            content: "private key".into(),
            validation: KeyImportValidation::Invalid("invalid key".into()),
        };

        state.close();

        assert!(!state.open);
        assert!(state.path.is_empty());
        assert!(state.content.is_empty());
        assert_eq!(state.validation, KeyImportValidation::WaitingForFile);
    }
}
