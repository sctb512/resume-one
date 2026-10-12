//! Safe local export writes, shared by the CLI and eventual desktop application.

use std::fs;
use std::io::Write;
use std::path::Path;

use atomic_write_file::AtomicWriteFile;
use tempfile::Builder;

#[derive(Debug)]
pub struct ExportError {
    pub code: &'static str,
    pub message: String,
}

fn issue(code: &'static str, message: impl Into<String>) -> ExportError {
    ExportError {
        code,
        message: message.into(),
    }
}

/// Write a generated export without ever replacing its source resume.
/// Existing output is only replaced when `overwrite` is explicit.
pub fn write_export(
    source: &Path,
    output: &Path,
    bytes: &[u8],
    overwrite: bool,
) -> Result<(), ExportError> {
    let source = fs::canonicalize(source).map_err(|err| issue("EXPORT_IO", err.to_string()))?;
    if output.exists() {
        let output_target =
            fs::canonicalize(output).map_err(|err| issue("EXPORT_IO", err.to_string()))?;
        if output_target == source {
            return Err(issue("SAME_FILE", "导出目标不能是原始简历文件"));
        }
    }
    if overwrite {
        let mut writer =
            AtomicWriteFile::open(output).map_err(|err| issue("EXPORT_IO", err.to_string()))?;
        writer
            .write_all(bytes)
            .map_err(|err| issue("EXPORT_IO", err.to_string()))?;
        writer
            .commit()
            .map_err(|err| issue("EXPORT_IO", err.to_string()))?;
    } else {
        let parent = output
            .parent()
            .ok_or_else(|| issue("EXPORT_IO", "目标文件缺少父目录"))?;
        let mut temp = Builder::new()
            .prefix(".resume-one-export-")
            .tempfile_in(parent)
            .map_err(|err| issue("EXPORT_IO", err.to_string()))?;
        temp.write_all(bytes)
            .and_then(|()| temp.as_file().sync_all())
            .map_err(|err| issue("EXPORT_IO", err.to_string()))?;
        temp.persist_noclobber(output).map_err(|err| {
            issue(
                if err.error.kind() == std::io::ErrorKind::AlreadyExists {
                    "FILE_EXISTS"
                } else {
                    "EXPORT_IO"
                },
                err.error.to_string(),
            )
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn does_not_overwrite_without_permission_or_touch_source() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("resume.json");
        let output = dir.path().join("resume.md");
        fs::write(&source, b"{}").unwrap();
        write_export(&source, &output, b"first", false).unwrap();
        assert_eq!(
            write_export(&source, &output, b"second", false)
                .unwrap_err()
                .code,
            "FILE_EXISTS"
        );
        assert_eq!(fs::read(&output).unwrap(), b"first");
        write_export(&source, &output, b"second", true).unwrap();
        assert_eq!(fs::read(&output).unwrap(), b"second");
        assert_eq!(fs::read(&source).unwrap(), b"{}");
        assert_eq!(
            write_export(&source, &source, b"bad", true)
                .unwrap_err()
                .code,
            "SAME_FILE"
        );
        assert_eq!(fs::read(&source).unwrap(), b"{}");
    }
}
