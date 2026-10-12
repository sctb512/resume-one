//! Revision-guarded, atomic JSON Patch for GUI and CLI.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

use atomic_write_file::AtomicWriteFile;
use serde_json::Value;

use crate::resume::{parse_resume, revision};

const MAX_PATCH_BYTES: usize = 1_048_576;

#[derive(Debug, Clone)]
pub struct PatchOutcome {
    pub previous_revision: String,
    pub revision: String,
    pub changed_paths: Vec<String>,
    pub dry_run: bool,
}

#[derive(Debug, Clone)]
pub struct StorageError {
    pub code: &'static str,
    pub message: String,
    pub current_revision: Option<String>,
}

fn error(code: &'static str, message: impl Into<String>) -> StorageError {
    StorageError {
        code,
        message: message.into(),
        current_revision: None,
    }
}

/// Apply an RFC 6902 patch only if the exact file bytes still match `if_match`.
/// Dry-runs perform all checks but never write the resume.
pub fn patch_resume(
    path: &Path,
    patch_bytes: &[u8],
    if_match: &str,
    dry_run: bool,
) -> Result<PatchOutcome, StorageError> {
    if !if_match.starts_with("sha256:")
        || if_match.len() != 71
        || !if_match[7..].bytes().all(|c| c.is_ascii_hexdigit())
    {
        return Err(error(
            "INVALID_REVISION",
            "--if-match 必须是 sha256: 后接 64 位十六进制摘要",
        ));
    }
    if patch_bytes.len() > MAX_PATCH_BYTES {
        return Err(error("PATCH_TOO_LARGE", "补丁超过 1 MiB 上限"));
    }
    let patch_value: Value = serde_json::from_slice(patch_bytes)
        .map_err(|err| error("INVALID_PATCH", format!("补丁 JSON 无效：{err}")))?;
    let entries = patch_value
        .as_array()
        .ok_or_else(|| error("INVALID_PATCH", "补丁必须是操作数组"))?;
    let patch: json_patch::Patch = serde_json::from_value(patch_value.clone())
        .map_err(|err| error("INVALID_PATCH", format!("补丁操作无效：{err}")))?;
    let changed_paths = entries
        .iter()
        .filter(|entry| entry.get("op").and_then(Value::as_str) != Some("test"))
        .filter_map(|entry| entry.get("path").and_then(Value::as_str))
        .fold(Vec::<String>::new(), |mut paths, path| {
            if !paths.iter().any(|existing| existing == path) {
                paths.push(path.into());
            }
            paths
        });

    let target = fs::canonicalize(path).map_err(|err| error("RESUME_IO", err.to_string()))?;
    if !target.is_file() {
        return Err(error("RESUME_IO", "目标不是普通文件"));
    }
    // A dry-run is strictly read-only: it must not create even a lock sidecar.
    let _lock = if dry_run {
        None
    } else {
        let file_name = target
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| error("RESUME_IO", "文件名必须可表示为 UTF-8"))?;
        let lock_path = target.with_file_name(format!(".{file_name}.resume-one.lock"));
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)
            .map_err(|err| error("RESUME_IO", err.to_string()))?;
        lock.lock()
            .map_err(|err| error("RESUME_IO", format!("无法锁定简历：{err}")))?;
        Some(lock)
    };

    let original = read_limited(&target, 16 * 1024 * 1024)?;
    let actual = revision(&original);
    if actual != if_match {
        return Err(StorageError {
            code: "REVISION_CONFLICT",
            message: "文件已被其他进程或同步服务修改".into(),
            current_revision: Some(actual),
        });
    }
    let mut document = parse_resume(&original).map_err(|issues| {
        error(
            "RESUME_INVALID",
            format!("原简历无效：{}", issues[0].message),
        )
    })?;
    let prior_value = document.value.clone();
    json_patch::patch(&mut document.value, &patch)
        .map_err(|err| error("PATCH_FAILED", format!("补丁未应用：{err}")))?;
    if document.value == prior_value {
        return Ok(PatchOutcome {
            previous_revision: actual.clone(),
            revision: actual,
            changed_paths: Vec::new(),
            dry_run,
        });
    }
    let mut next = serde_json::to_vec_pretty(&document.value)
        .map_err(|err| error("RESUME_IO", err.to_string()))?;
    next.push(b'\n');
    parse_resume(&next).map_err(|issues| {
        error(
            "RESUME_INVALID",
            format!("补丁结果无效：{}", issues[0].message),
        )
    })?;
    let outcome = PatchOutcome {
        previous_revision: actual,
        revision: revision(&next),
        changed_paths,
        dry_run,
    };
    if dry_run {
        return Ok(outcome);
    }

    // External sync agents do not obey our lock. Recheck immediately before replacement.
    let just_before_write = revision(&read_limited(&target, 16 * 1024 * 1024)?);
    if just_before_write != outcome.previous_revision {
        return Err(StorageError {
            code: "REVISION_CONFLICT",
            message: "写入前文件发生变化".into(),
            current_revision: Some(just_before_write),
        });
    }
    let mut writer =
        AtomicWriteFile::open(&target).map_err(|err| error("RESUME_IO", err.to_string()))?;
    writer
        .write_all(&next)
        .map_err(|err| error("RESUME_IO", err.to_string()))?;
    writer
        .commit()
        .map_err(|err| error("RESUME_IO", err.to_string()))?;
    Ok(outcome)
}

fn read_limited(path: &Path, maximum: usize) -> Result<Vec<u8>, StorageError> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(maximum as u64 + 1).read_to_end(&mut bytes))
        .map_err(|err| error("RESUME_IO", err.to_string()))?;
    if bytes.len() > maximum {
        return Err(error("RESUME_TOO_LARGE", "简历文件超过 16 MiB 上限"));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resume::load_resume;
    use std::sync::Arc;

    fn fixture() -> (tempfile::TempDir, std::path::PathBuf, String) {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("简历.json");
        fs::write(&path, r#"{"basics":{"name":"甲"},"custom":{"keep":true}}"#).unwrap();
        let revision = load_resume(&path).unwrap().revision;
        (root, path, revision)
    }

    #[test]
    fn dry_run_does_not_write_and_commit_preserves_extensions() {
        let (_root, path, before) = fixture();
        let patch = r#"[{"op":"replace","path":"/basics/name","value":"乙"}]"#.as_bytes();
        let original = fs::read(&path).unwrap();
        let preview = patch_resume(&path, patch, &before, true).unwrap();
        assert_eq!(original, fs::read(&path).unwrap());
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
        assert_ne!(preview.revision, before);
        let committed = patch_resume(&path, patch, &before, false).unwrap();
        assert_eq!(committed.revision, preview.revision);
        let value = load_resume(&path).unwrap().value;
        assert_eq!(value["basics"]["name"], "乙");
        assert_eq!(value["custom"]["keep"], true);
    }

    #[test]
    fn stale_revision_never_overwrites_newer_content() {
        let (_root, path, before) = fixture();
        fs::write(&path, r#"{"basics":{"name":"外部修改"}}"#).unwrap();
        let error = patch_resume(&path, br#"[]"#, &before, false).unwrap_err();
        assert_eq!(error.code, "REVISION_CONFLICT");
        assert!(error.current_revision.is_some());
        assert_eq!(
            load_resume(&path).unwrap().value["basics"]["name"],
            "外部修改"
        );
    }

    #[test]
    fn failed_test_operation_rolls_back_whole_patch() {
        let (_root, path, before) = fixture();
        let original = fs::read(&path).unwrap();
        let patch = r#"[{"op":"replace","path":"/basics/name","value":"乙"},{"op":"test","path":"/basics/name","value":"甲"}]"#.as_bytes();
        assert_eq!(
            patch_resume(&path, patch, &before, false).unwrap_err().code,
            "PATCH_FAILED"
        );
        assert_eq!(fs::read(&path).unwrap(), original);
    }

    #[test]
    fn invalid_result_does_not_replace_original() {
        let (_root, path, before) = fixture();
        let original = fs::read(&path).unwrap();
        let patch = br#"[{"op":"replace","path":"/basics/name","value":42}]"#;
        assert_eq!(
            patch_resume(&path, patch, &before, false).unwrap_err().code,
            "RESUME_INVALID"
        );
        assert_eq!(fs::read(&path).unwrap(), original);
    }

    #[test]
    fn no_op_patch_preserves_exact_source_bytes() {
        let (_root, path, before) = fixture();
        let original = fs::read(&path).unwrap();
        let outcome = patch_resume(&path, br#"[]"#, &before, false).unwrap();
        assert_eq!(outcome.revision, before);
        assert!(outcome.changed_paths.is_empty());
        assert_eq!(fs::read(&path).unwrap(), original);
    }

    #[test]
    fn competing_writers_cannot_both_commit_same_revision() {
        let (_root, path, before) = fixture();
        let path = Arc::new(path);
        let workers: Vec<_> = (0..2)
            .map(|index| {
                let path = Arc::clone(&path);
                let before = before.clone();
                std::thread::spawn(move || {
                    let patch =
                        format!(r#"[{{"op":"replace","path":"/basics/name","value":"{index}"}}]"#);
                    patch_resume(&path, patch.as_bytes(), &before, false)
                })
            })
            .collect();
        let results: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| result
                    .as_ref()
                    .err()
                    .is_some_and(|error| error.code == "REVISION_CONFLICT"))
                .count(),
            1
        );
    }
}
