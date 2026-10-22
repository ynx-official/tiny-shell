use super::InstallationKind;

/// Both update entry points must describe the same platform-specific handoff.
pub(crate) struct InstallPresentation {
    pub(crate) action: &'static str,
    pub(crate) hint: &'static str,
    pub(crate) confirmation_title: &'static str,
    pub(crate) confirmation_description: &'static str,
}

pub(crate) fn install_presentation(kind: InstallationKind) -> InstallPresentation {
    if kind == InstallationKind::WindowsInstaller {
        InstallPresentation {
            action: "update_install_now",
            hint: "update_setup_hint",
            confirmation_title: "update_setup_confirm_title",
            confirmation_description: "update_setup_confirm_desc",
        }
    } else {
        InstallPresentation {
            action: "update_restart_now",
            hint: "update_restart_hint",
            confirmation_title: "update_restart_confirm_title",
            confirmation_description: "update_restart_confirm_desc",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_installer_describes_a_wizard_instead_of_an_automatic_restart() {
        let copy = install_presentation(InstallationKind::WindowsInstaller);
        assert_eq!(copy.action, "update_install_now");
        assert_eq!(copy.hint, "update_setup_hint");
        assert_eq!(copy.confirmation_description, "update_setup_confirm_desc");
    }

    #[test]
    fn other_installations_keep_the_install_and_restart_flow() {
        for kind in [
            InstallationKind::Portable,
            InstallationKind::MacApp,
            InstallationKind::MacInstaller,
            InstallationKind::LinuxAppImage,
            InstallationKind::LinuxPackage,
        ] {
            let copy = install_presentation(kind);
            assert_eq!(copy.action, "update_restart_now");
            assert_eq!(copy.hint, "update_restart_hint");
            assert_eq!(copy.confirmation_description, "update_restart_confirm_desc");
        }
    }
}
