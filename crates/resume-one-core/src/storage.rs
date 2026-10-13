//! Revision-guarded, atomic JSON Patch for GUI and CLI.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

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

fn check_revision(if_match: &str) -> Result<(), StorageError> {
    if !if_match.starts_with("sha256:")
        || if_match.len() != 71
        || !if_match[7..].bytes().all(|c| c.is_ascii_hexdigit())
    {
        return Err(error(
            "INVALID_REVISION",
            "if-match 必须是 sha256: 后接 64 位十六进制摘要",
        ));
    }
    Ok(())
}

// Every writer uses the same sidecar lock, including the CLI patch writer.
fn lock_target(path: &Path, dry_run: bool) -> Result<(PathBuf, Option<File>), StorageError> {
    let target = fs::canonicalize(path).map_err(|err| error("RESUME_IO", err.to_string()))?;
    if !target.is_file() {
        return Err(error("RESUME_IO", "目标不是普通文件"));
    }
    let lock = if dry_run {
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
    Ok((target, lock))
}

fn ensure_revision(path: &Path, expected: &str) -> Result<Vec<u8>, StorageError> {
    let source = read_limited(path, 16 * 1024 * 1024)?;
    let actual = revision(&source);
    if actual != expected {
        return Err(StorageError {
            code: "REVISION_CONFLICT",
            message: "文件已被其他进程或同步服务修改；未覆盖外部内容".into(),
            current_revision: Some(actual),
        });
    }
    Ok(source)
}

fn commit_source(path: &Path, source: &[u8], expected: &str) -> Result<(), StorageError> {
    // Sync agents do not obey our lock. This is a best-effort recheck, not a
    // filesystem CAS: an external writer can still race the atomic replacement.
    ensure_revision(path, expected)?;
    let mut writer =
        AtomicWriteFile::open(path).map_err(|err| error("RESUME_IO", err.to_string()))?;
    writer
        .write_all(source)
        .map_err(|err| error("RESUME_IO", err.to_string()))?;
    writer
        .commit()
        .map_err(|err| error("RESUME_IO", err.to_string()))
}

/// Save the user's exact UTF-8 JSON bytes, without adding formatting or metadata.
/// Only existing files are supported; creation/Save As has a separate contract.
/// The original may be invalid JSON, so a validated repair can replace it.
pub fn replace_resume(path: &Path, source: &[u8], if_match: &str) -> Result<String, StorageError> {
    check_revision(if_match)?;
    parse_resume(source).map_err(|issues| {
        error(
            "RESUME_INVALID",
            format!("简历未保存：{}", issues[0].message),
        )
    })?;
    let (target, _lock) = lock_target(path, false)?;
    let original = ensure_revision(&target, if_match)?;
    let next_revision = revision(source);
    if original != source {
        commit_source(&target, source, if_match)?;
    }
    Ok(next_revision)
}

/// Save As creates a new file only, never overwriting even a symlink.
pub fn create_resume(path: &Path, source: &[u8]) -> Result<String, StorageError> {
    parse_resume(source).map_err(|issues| {
        error(
            "RESUME_INVALID",
            format!("简历未保存：{}", issues[0].message),
        )
    })?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(parent)
        .map_err(|err| error("RESUME_IO", err.to_string()))?;
    file.write_all(source)
        .and_then(|_| file.as_file().sync_all())
        .map_err(|err| error("RESUME_IO", err.to_string()))?;
    file.persist_noclobber(path).map_err(|err| {
        if err.error.kind() == std::io::ErrorKind::AlreadyExists {
            error("FILE_EXISTS", "文件已经存在；请选择其他名称，原文件未修改")
        } else {
            error("RESUME_IO", err.error.to_string())
        }
    })?;
    Ok(revision(source))
}

/// Apply an RFC 6902 patch only if the exact file bytes still match `if_match`.
/// Dry-runs perform all checks but never write the resume.
pub fn patch_resume(
    path: &Path,
    patch_bytes: &[u8],
    if_match: &str,
    dry_run: bool,
) -> Result<PatchOutcome, StorageError> {
    check_revision(if_match)?;
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

    // A dry-run is strictly read-only, including sidecar creation.
    let (target, _lock) = lock_target(path, dry_run)?;
    let original = ensure_revision(&target, if_match)?;
    let actual = revision(&original);
    let mut document = parse_resume(&original).map_err(|issues| {
        error(
            "RESUME_INVALID",
            format!("原简历无效：{}", issues[0].message),
        )
    })?;
    let prior_value = document.value.clone();
    json_patch::patch(&mut document.value, &patch)
        .map_err(|_| error("PATCH_FAILED", "补丁未应用；请检查操作路径和 test 条件"))?;
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

    commit_source(&target, &next, &outcome.previous_revision)?;
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

    #[test]
    fn replacement_keeps_exact_text_and_rejects_invalid_input_before_writing() {
        let (_root, path, before) = fixture();
        let original = fs::read(&path).unwrap();
        assert_eq!(
            replace_resume(&path, br#"{"basics":{"name":42}}"#, &before)
                .unwrap_err()
                .code,
            "RESUME_INVALID"
        );
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
        assert_eq!(
            replace_resume(&path, b"{}", "bad").unwrap_err().code,
            "INVALID_REVISION"
        );
        let next = "{ \"basics\": {\"name\": \"乙😀\"}, \"custom\": {\"keep\": true} }\r\n";
        assert_eq!(
            replace_resume(&path, next.as_bytes(), &before).unwrap(),
            revision(next.as_bytes())
        );
        assert_eq!(fs::read(&path).unwrap(), next.as_bytes());
    }

    #[test]
    fn replacement_cannot_overwrite_external_change_or_recreate_deleted_file() {
        let (_root, path, before) = fixture();
        let external = b"{broken external input";
        fs::write(&path, external).unwrap();
        let conflict = replace_resume(&path, b"{}", &before).unwrap_err();
        assert_eq!(conflict.code, "REVISION_CONFLICT");
        assert_eq!(conflict.current_revision, Some(revision(external)));
        assert_eq!(fs::read(&path).unwrap(), external);
        fs::remove_file(&path).unwrap();
        assert_eq!(
            replace_resume(&path, b"{}", &before).unwrap_err().code,
            "RESUME_IO"
        );
        assert!(!path.exists());
    }

    #[test]
    fn editor_and_cli_cannot_both_commit_the_same_baseline() {
        let (_root, path, before) = fixture();
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let patch_worker = {
            let path = path.clone();
            let before = before.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                patch_resume(
                    &path,
                    br#"[{"op":"replace","path":"/basics/name","value":"CLI"}]"#,
                    &before,
                    false,
                )
                .map(|outcome| outcome.revision)
            })
        };
        let replace_worker = std::thread::spawn(move || {
            barrier.wait();
            replace_resume(
                &path,
                br#"{"basics":{"name":"editor"},"custom":{"keep":true}}"#,
                &before,
            )
        });
        let results = [patch_worker.join().unwrap(), replace_worker.join().unwrap()];
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| result
                    .as_ref()
                    .is_err_and(|err| err.code == "REVISION_CONFLICT"))
                .count(),
            1
        );
    }

    #[test]
    fn save_as_creates_exact_source_and_never_clobbers_existing_file() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("新简历.json");
        let source = "{ \"basics\": {\"name\": \"李明😀\"} }\r\n";
        assert_eq!(
            create_resume(&path, source.as_bytes()).unwrap(),
            revision(source.as_bytes())
        );
        assert_eq!(fs::read(&path).unwrap(), source.as_bytes());
        assert_eq!(create_resume(&path, b"{}").unwrap_err().code, "FILE_EXISTS");
        assert_eq!(fs::read(&path).unwrap(), source.as_bytes());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[test]
    fn patching_known_field_preserves_unknown_high_precision_numbers() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("precision.json");
        let source = br#"{"basics":{"name":"A"},"extra":{"big":184467440737095516170123,"decimal":0.12345678901234567890123456789}}"#;
        fs::write(&path, source).unwrap();
        let before = load_resume(&path).unwrap();
        patch_resume(
            &path,
            br#"[{"op":"replace","path":"/basics/name","value":"B"}]"#,
            &before.revision,
            false,
        )
        .unwrap();
        let after = load_resume(&path).unwrap();
        assert_eq!(after.value["basics"]["name"], "B");
        assert_eq!(after.value["extra"], before.value["extra"]);
        let text = String::from_utf8(after.source).unwrap();
        assert!(text.contains("184467440737095516170123"));
        assert!(text.contains("0.12345678901234567890123456789"));
    }

    #[test]
    fn invalid_save_as_creates_no_artifacts() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("invalid.json");
        assert_eq!(
            create_resume(&path, b"{").unwrap_err().code,
            "RESUME_INVALID"
        );
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }

    #[test]
    fn competing_save_as_writers_cannot_overwrite_each_other() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("resume.json");
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let workers: Vec<_> = (0..2)
            .map(|index| {
                let path = path.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    create_resume(
                        &path,
                        format!(r#"{{"basics":{{"name":"{index}"}}}}"#).as_bytes(),
                    )
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
                .filter(|result| result.as_ref().is_err_and(|err| err.code == "FILE_EXISTS"))
                .count(),
            1
        );
        assert!(load_resume(&path).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn save_as_does_not_follow_even_dangling_symlinks() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("target.json");
        let link = root.path().join("link.json");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert_eq!(create_resume(&link, b"{}").unwrap_err().code, "FILE_EXISTS");
        assert!(link.is_symlink());
        assert!(!target.exists());
    }

    #[cfg(unix)]
    #[test]
    fn replacement_preserves_permissions_and_symlink_identity() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let (root, path, before) = fixture();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        let link = root.path().join("linked.json");
        symlink(&path, &link).unwrap();
        replace_resume(&link, b"{}", &before).unwrap();
        assert!(link.is_symlink());
        assert_eq!(fs::read(&path).unwrap(), b"{}");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }
}
