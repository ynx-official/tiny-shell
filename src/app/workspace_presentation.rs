#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum WorkspaceMode {
    #[default]
    Normal,
    Clean {
        sftp: CleanSftpState,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum CleanSftpState {
    #[default]
    Hidden,
    Expanded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WorkspacePresentation {
    pub(crate) clean: bool,
    pub(crate) show_sidebar: bool,
    pub(crate) show_sftp_footer: bool,
    pub(crate) sftp_minimized: bool,
}

impl WorkspaceMode {
    pub(crate) fn toggle_clean(&mut self) {
        *self = match self {
            Self::Normal => Self::Clean {
                sftp: CleanSftpState::Hidden,
            },
            Self::Clean { .. } => Self::Normal,
        };
    }

    pub(crate) fn toggle_clean_sftp(&mut self) {
        if let Self::Clean { sftp } = self {
            *sftp = match sftp {
                CleanSftpState::Hidden => CleanSftpState::Expanded,
                CleanSftpState::Expanded => CleanSftpState::Hidden,
            };
        }
    }

    pub(crate) fn presentation(self, normal_sftp_minimized: bool) -> WorkspacePresentation {
        match self {
            Self::Normal => WorkspacePresentation {
                clean: false,
                show_sidebar: true,
                show_sftp_footer: true,
                sftp_minimized: normal_sftp_minimized,
            },
            Self::Clean { sftp } => WorkspacePresentation {
                clean: true,
                show_sidebar: false,
                show_sftp_footer: false,
                sftp_minimized: sftp == CleanSftpState::Hidden,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CleanSftpState, WorkspaceMode};

    #[test]
    fn clean_mode_starts_with_sftp_hidden_and_restores_normal_presentation() {
        let mut mode = WorkspaceMode::Normal;

        mode.toggle_clean();
        assert_eq!(
            mode,
            WorkspaceMode::Clean {
                sftp: CleanSftpState::Hidden
            }
        );
        assert!(mode.presentation(false).sftp_minimized);

        mode.toggle_clean();
        assert_eq!(mode, WorkspaceMode::Normal);
        assert!(!mode.presentation(false).sftp_minimized);
    }

    #[test]
    fn clean_sftp_state_is_independent_from_normal_sftp_state() {
        let mut mode = WorkspaceMode::Clean {
            sftp: CleanSftpState::Hidden,
        };

        mode.toggle_clean_sftp();
        assert!(!mode.presentation(true).sftp_minimized);

        mode.toggle_clean();
        assert!(mode.presentation(true).sftp_minimized);
    }
}
