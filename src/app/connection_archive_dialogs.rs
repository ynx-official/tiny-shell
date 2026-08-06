use std::{fs, path::PathBuf};

use rust_i18n::t;

use crate::TinyShell;

impl TinyShell {
    pub(crate) fn export_connection_archive(
        &mut self,
        path: &PathBuf,
        password: &str,
    ) -> anyhow::Result<()> {
        let archive = crate::session::connection_archive::ConnectionArchive::new(
            self.config.connection_groups(),
            self.config.sessions(),
            true,
        );
        let json = archive.export_json(password)?;
        fs::write(path, json)?;
        self.status = t!("connection_archive_exported").to_string().into();
        Ok(())
    }

    pub(crate) fn import_connection_archive(
        &mut self,
        path: &PathBuf,
        password: &str,
    ) -> anyhow::Result<()> {
        let json = fs::read_to_string(path)?;
        let archive = crate::session::connection_archive::import_json(&json, password)?;
        let mut staged = self.config.clone();
        let summary = crate::session::connection_archive::apply_import(&mut staged, archive);
        crate::app::config_persistence::save_full(&self.config_repository, &staged)?;
        self.config = staged;
        self.status = t!(
            "connection_archive_imported",
            count = summary.imported_sessions
        )
        .to_string()
        .into();
        Ok(())
    }
}
