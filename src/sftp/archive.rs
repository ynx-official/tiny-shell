use std::{fs, path::Path};

use anyhow::{Context, Result, anyhow};
use flate2::read::GzDecoder;
use walkdir::WalkDir;
use zip::read::ZipArchive;

pub(super) fn create_zip_from_directory(source: &Path, target: &Path) -> Result<()> {
    let file =
        fs::File::create(target).with_context(|| format!("create ZIP {}", target.display()))?;
    let mut archive = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for entry in WalkDir::new(source) {
        let entry = entry.with_context(|| format!("walk {}", source.display()))?;
        let path = entry.path();
        let relative = path
            .strip_prefix(source)
            .with_context(|| format!("strip ZIP root from {}", path.display()))?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let name = relative.to_string_lossy().replace('\\', "/");
        if entry.file_type().is_dir() {
            archive
                .add_directory(format!("{name}/"), options)
                .with_context(|| format!("add ZIP directory {name}"))?;
        } else {
            archive
                .start_file(&name, options)
                .with_context(|| format!("add ZIP file {name}"))?;
            let mut input = fs::File::open(path)
                .with_context(|| format!("open ZIP source {}", path.display()))?;
            std::io::copy(&mut input, &mut archive)
                .with_context(|| format!("write ZIP file {name}"))?;
        }
    }
    archive.finish().context("finish ZIP archive")?;
    Ok(())
}

pub(super) async fn extract_archive_to(path: &Path, target_dir: &Path) -> Result<()> {
    let Some(file_name) = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
    else {
        return Ok(());
    };
    let archive_path = path.to_path_buf();
    let target_dir = target_dir.to_path_buf();

    tokio::task::spawn_blocking(move || -> Result<()> {
        fs::create_dir_all(&target_dir)
            .with_context(|| format!("create {}", target_dir.display()))?;

        if file_name.ends_with(".zip") {
            let file = fs::File::open(&archive_path)
                .with_context(|| format!("open {}", archive_path.display()))?;
            let mut zip = ZipArchive::new(file).context("read zip archive")?;
            for index in 0..zip.len() {
                let mut entry = zip.by_index(index).context("read zip entry")?;
                let name = entry.enclosed_name().ok_or_else(|| {
                    anyhow!("ZIP entry escapes extraction directory: {}", entry.name())
                })?;
                let output = target_dir.join(name);
                if entry.is_dir() {
                    fs::create_dir_all(&output)?;
                } else {
                    if let Some(parent) = output.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    let mut output_file = fs::File::create(&output)?;
                    std::io::copy(&mut entry, &mut output_file)?;
                }
            }
        } else if file_name.ends_with(".tar.gz") || file_name.ends_with(".tgz") {
            let file = fs::File::open(&archive_path)
                .with_context(|| format!("open {}", archive_path.display()))?;
            let decoder = GzDecoder::new(file);
            let mut archive = tar::Archive::new(decoder);
            archive
                .unpack(&target_dir)
                .context("unpack tar.gz archive")?;
        } else if file_name.ends_with(".tar") {
            let file = fs::File::open(&archive_path)
                .with_context(|| format!("open {}", archive_path.display()))?;
            let mut archive = tar::Archive::new(file);
            archive.unpack(&target_dir).context("unpack tar archive")?;
        }

        Ok(())
    })
    .await
    .context("extract archive task join failure")??;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;
    use uuid::Uuid;

    #[test]
    fn creates_zip_with_nested_files() -> Result<()> {
        let root = std::env::temp_dir().join(format!("tiny-shell-zip-test-{}", Uuid::new_v4()));
        let source = root.join("source");
        let nested = source.join("folder");
        fs::create_dir_all(&nested)?;
        fs::write(source.join("root.txt"), b"root")?;
        fs::write(nested.join("nested.txt"), b"nested")?;
        let target = root.join("archive.zip");

        create_zip_from_directory(&source, &target)?;

        let file = fs::File::open(&target)?;
        let mut archive = ZipArchive::new(file)?;
        assert!(archive.by_name("root.txt").is_ok());
        assert!(archive.by_name("folder/nested.txt").is_ok());
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[tokio::test]
    async fn extracts_created_zip_with_nested_files() -> Result<()> {
        let root = std::env::temp_dir().join(format!("tiny-shell-unzip-test-{}", Uuid::new_v4()));
        let source = root.join("source");
        fs::create_dir_all(source.join("folder"))?;
        fs::write(source.join("folder/nested.txt"), b"nested")?;
        let archive = root.join("archive.zip");
        let extracted = root.join("extracted");

        create_zip_from_directory(&source, &archive)?;
        extract_archive_to(&archive, &extracted).await?;

        assert_eq!(fs::read(extracted.join("folder/nested.txt"))?, b"nested");
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[tokio::test]
    async fn zip_extraction_rejects_directory_traversal_entries() -> Result<()> {
        let root =
            std::env::temp_dir().join(format!("tiny-shell-zip-slip-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root)?;
        let archive_path = root.join("archive.zip");
        let extracted = root.join("extracted");

        let file = fs::File::create(&archive_path)?;
        let mut archive = zip::ZipWriter::new(file);
        archive.start_file("../escaped.txt", zip::write::SimpleFileOptions::default())?;
        archive.write_all(b"must stay contained")?;
        archive.finish()?;

        let result = extract_archive_to(&archive_path, &extracted).await;

        assert!(result.is_err());
        assert!(!root.join("escaped.txt").exists());
        assert!(!extracted.join("escaped.txt").exists());
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
