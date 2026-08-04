use std::path::Path;

use anyhow::{Context, Result};

/// Opens a URL in the user's default browser.
pub(crate) fn open_url(url: &str) -> Result<()> {
    open::that(url).with_context(|| format!("failed to open url: {url}"))
}

/// Opens a file or directory with the system's default application.
pub(crate) fn open_path(path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    open::that(path).with_context(|| format!("failed to open path: {}", path.display()))
}

/// Opens the README.md file in the current working directory.
pub(crate) fn open_documentation() -> Result<()> {
    open_path("README.md")
}
