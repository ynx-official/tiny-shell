//! Windows-native RDP launcher.
//!
//! Windows already ships the mature `mstsc.exe` client, including keyboard,
//! clipboard, drive redirection, fullscreen, and reconnect behavior.  TinyShell
//! owns the saved connection record, but deliberately does not embed or proxy
//! the desktop on Windows.

use std::{fs, path::PathBuf, process::Command, thread};

use anyhow::{Context, Result, anyhow, bail};
use uuid::Uuid;
use windows_sys::Win32::{
    Foundation::GetLastError,
    Security::Credentials::{
        CRED_PERSIST_SESSION, CRED_TYPE_DOMAIN_PASSWORD, CREDENTIALW, CredDeleteW, CredWriteW,
    },
};

use crate::session::config::Session;

pub(crate) fn launch(session: &Session) -> Result<()> {
    if session.host.trim().is_empty() {
        bail!("RDP host cannot be empty");
    }
    if session.port == 0 {
        bail!("RDP port is invalid");
    }

    let credential_target = if session.password.is_empty() {
        None
    } else {
        Some(write_session_credential(session)?)
    };
    let path = match write_rdp_profile(session) {
        Ok(path) => path,
        Err(error) => {
            if let Some(target) = credential_target.as_deref() {
                delete_session_credential(target);
            }
            return Err(error);
        }
    };
    let mut child = match Command::new("mstsc.exe").arg(&path).spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = fs::remove_file(&path);
            if let Some(target) = credential_target.as_deref() {
                delete_session_credential(target);
            }
            return Err(error).context("failed to start Windows Remote Desktop (mstsc.exe)");
        }
    };

    thread::spawn(move || {
        let _ = child.wait();
        let _ = fs::remove_file(path);
    });
    Ok(())
}

fn write_rdp_profile(session: &Session) -> Result<PathBuf> {
    let path = std::env::temp_dir().join(format!("tiny-shell-rdp-{}.rdp", Uuid::new_v4()));
    let address = format!("{}:{}", escape_rdp_value(session.host.trim()), session.port);
    let username = escape_rdp_value(session.user.trim());
    let prompt_for_credentials = if session.password.is_empty() { 1 } else { 0 };
    let profile = format!(
        "full address:s:{address}\r\n\
         username:s:{username}\r\n\
         prompt for credentials:i:{prompt_for_credentials}\r\n\
         redirectclipboard:i:1\r\n\
         drivestoredirect:s:*\r\n\
         redirectprinters:i:0\r\n\
         redirectsmartcards:i:0\r\n\
         session bpp:i:32\r\n"
    );
    fs::write(&path, profile.as_bytes())
        .with_context(|| format!("failed to write RDP profile {}", path.display()))?;
    Ok(path)
}

/// Writes a logon-session-only TERMSRV credential. The password never enters
/// the `.rdp` file or a process command line; mstsc reads it through the
/// Windows credential provider. It expires when the Windows user signs out.
fn write_session_credential(session: &Session) -> Result<Vec<u16>> {
    let address = if session.port == 3389 {
        session.host.trim().to_string()
    } else {
        format!("{}:{}", session.host.trim(), session.port)
    };
    let mut target = wide_string(&format!("TERMSRV/{address}"));
    let mut username = wide_string(session.user.trim());
    let mut password = session
        .password
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    let credential = CREDENTIALW {
        Type: CRED_TYPE_DOMAIN_PASSWORD,
        TargetName: target.as_mut_ptr(),
        UserName: username.as_mut_ptr(),
        CredentialBlobSize: u32::try_from(password.len())
            .map_err(|_| anyhow!("RDP password is too long for Windows Credential Manager"))?,
        CredentialBlob: password.as_mut_ptr(),
        Persist: CRED_PERSIST_SESSION,
        ..Default::default()
    };

    let written = unsafe { CredWriteW(&credential, 0) } != 0;
    password.fill(0);
    if !written {
        return Err(anyhow!(
            "Windows Credential Manager rejected the RDP credential (error {})",
            unsafe { GetLastError() }
        ));
    }
    Ok(target)
}

fn delete_session_credential(target: &[u16]) {
    if target.is_empty() {
        return;
    }
    unsafe {
        let _ = CredDeleteW(target.as_ptr(), CRED_TYPE_DOMAIN_PASSWORD, 0);
    }
}

fn wide_string(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

fn escape_rdp_value(value: &str) -> String {
    value.replace(['\r', '\n'], "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_contains_native_clipboard_and_drive_redirection() {
        let session = Session::rdp(
            "desktop.example".into(),
            3389,
            "alice".into(),
            "secret".into(),
        );
        let path = write_rdp_profile(&session).expect("profile should be written");
        let contents = fs::read_to_string(&path).expect("profile should be readable");
        let _ = fs::remove_file(path);

        assert!(contents.contains("full address:s:desktop.example:3389"));
        assert!(contents.contains("username:s:alice"));
        assert!(contents.contains("redirectclipboard:i:1"));
        assert!(contents.contains("drivestoredirect:s:*"));
        assert!(contents.contains("prompt for credentials:i:0"));
        assert!(!contents.contains("password"));
    }

    #[test]
    fn profile_removes_newlines_from_username() {
        assert_eq!(
            escape_rdp_value("alice\r\nredirect:i:1"),
            "aliceredirect:i:1"
        );
    }

    #[test]
    fn profile_removes_newlines_from_host() {
        let session = Session::rdp(
            "desktop\r\nredirectclipboard:i:0".into(),
            3389,
            "alice".into(),
            String::new(),
        );
        let path = write_rdp_profile(&session).expect("profile should be written");
        let contents = fs::read_to_string(&path).expect("profile should be readable");
        let _ = fs::remove_file(path);

        assert!(contents.contains("full address:s:desktopredirectclipboard:i:0:3389"));
        assert!(!contents.contains("full address:s:desktop\r"));
    }
}
