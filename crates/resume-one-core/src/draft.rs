//! Private local recovery files for editor text that has not reached the resume.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const MAX_DRAFT_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryDraft {
    pub source: String,
    pub if_match: String,
}

/// Each document has one fixed recovery slot. Clones of a store serialize
/// background writes so an older queued task cannot undo a newer operation.
#[derive(Clone, Debug)]
pub struct DraftStore {
    root: PathBuf,
    file_name: String,
    generation: Arc<Mutex<u64>>,
}

impl DraftStore {
    pub fn new(root: PathBuf, document_path: Option<PathBuf>) -> Self {
        let mut hasher = Sha256::new();
        if let Some(path) = &document_path {
            hasher.update(b"file\0");
            hasher.update(path.as_os_str().as_encoded_bytes());
        } else {
            hasher.update(b"unsaved\0");
        }
        let digest = hasher.finalize();
        Self {
            root,
            file_name: format!("draft-{digest:x}.json"),
            generation: Arc::new(Mutex::new(0)),
        }
    }

    pub fn path(&self) -> PathBuf {
        self.root.join(&self.file_name)
    }

    pub fn load(&self) -> Result<Option<RecoveryDraft>, String> {
        let mut source = Vec::new();
        let file = match fs::File::open(self.path()) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("无法读取恢复草稿：{error}")),
        };
        file.take(MAX_DRAFT_BYTES as u64 * 6 + 4096)
            .read_to_end(&mut source)
            .map_err(|error| format!("无法读取恢复草稿：{error}"))?;
        if source.len() >= MAX_DRAFT_BYTES * 6 + 4096 {
            return Err("恢复草稿过大".into());
        }
        let value: Value =
            serde_json::from_slice(&source).map_err(|error| format!("恢复草稿损坏：{error}"))?;
        if value.get("version").and_then(Value::as_u64) != Some(1)
            || value.get("documentId").and_then(Value::as_str) != Some(self.file_name.as_str())
        {
            return Err("恢复草稿身份不匹配".into());
        }
        let text = value
            .get("source")
            .and_then(Value::as_str)
            .ok_or("恢复草稿缺少原文")?;
        let if_match = value
            .get("ifMatch")
            .and_then(Value::as_str)
            .ok_or("恢复草稿缺少文件版本")?;
        if text.len() > MAX_DRAFT_BYTES || !valid_revision(if_match) {
            return Err("恢复草稿内容或版本无效".into());
        }
        Ok(Some(RecoveryDraft {
            source: text.into(),
            if_match: if_match.into(),
        }))
    }

    pub fn write_if_newer(&self, generation: u64, draft: &RecoveryDraft) -> Result<bool, String> {
        if draft.source.len() > MAX_DRAFT_BYTES || !valid_revision(&draft.if_match) {
            return Err("恢复草稿内容或版本无效".into());
        }
        let mut latest = self.generation.lock().map_err(|_| "恢复草稿锁已失效")?;
        if generation <= *latest {
            return Ok(false);
        }
        fs::create_dir_all(&self.root).map_err(|error| format!("无法创建恢复目录：{error}"))?;
        restrict_directory(&self.root).map_err(|error| format!("无法保护恢复目录：{error}"))?;
        let mut file = tempfile::NamedTempFile::new_in(&self.root)
            .map_err(|error| format!("无法创建恢复文件：{error}"))?;
        let value = json!({
            "version": 1,
            "documentId": self.file_name,
            "ifMatch": draft.if_match,
            "source": draft.source,
        });
        serde_json::to_writer(&mut file, &value)
            .map_err(|error| format!("无法写入恢复草稿：{error}"))?;
        file.flush()
            .and_then(|_| file.as_file().sync_all())
            .map_err(|error| format!("无法同步恢复草稿：{error}"))?;
        file.persist(self.path())
            .map_err(|error| format!("无法提交恢复草稿：{}", error.error))?;
        *latest = generation;
        Ok(true)
    }

    pub fn clear_if_newer(&self, generation: u64) -> Result<bool, String> {
        let mut latest = self.generation.lock().map_err(|_| "恢复草稿锁已失效")?;
        if generation <= *latest {
            return Ok(false);
        }
        match fs::remove_file(self.path()) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("无法清除已保存草稿：{error}")),
        }
        *latest = generation;
        Ok(true)
    }
}

pub(crate) fn valid_revision(revision: &str) -> bool {
    revision.starts_with("sha256:")
        && revision.len() == 71
        && revision[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(unix)]
fn restrict_directory(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn restrict_directory(_: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resume::revision;

    #[test]
    fn restart_round_trip_keeps_invalid_unicode_source_and_baseline() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("résumé.json");
        let store = DraftStore::new(temp.path().join("drafts"), Some(path.clone()));
        assert_eq!(store.load().unwrap(), None);
        let draft = RecoveryDraft {
            source: "{ \"name\": \"李明😀\"".into(),
            if_match: revision(b"{}"),
        };
        assert!(store.write_if_newer(1, &draft).unwrap());
        let reopened = DraftStore::new(temp.path().join("drafts"), Some(path));
        assert_eq!(reopened.load().unwrap(), Some(draft));
    }

    #[test]
    fn delayed_write_cannot_resurrect_cleared_or_newer_draft() {
        let temp = tempfile::tempdir().unwrap();
        let store = DraftStore::new(temp.path().join("drafts"), None);
        let first = RecoveryDraft {
            source: "{old".into(),
            if_match: revision(b"{}"),
        };
        let next = RecoveryDraft {
            source: "{new".into(),
            if_match: revision(b"{}"),
        };
        store.write_if_newer(2, &next).unwrap();
        assert!(!store.write_if_newer(1, &first).unwrap());
        assert_eq!(store.load().unwrap(), Some(next));
        store.clear_if_newer(3).unwrap();
        assert!(!store.write_if_newer(2, &first).unwrap());
        assert_eq!(store.load().unwrap(), None);
    }

    #[test]
    fn different_documents_have_independent_slots() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("drafts");
        let a = DraftStore::new(root.clone(), Some(temp.path().join("a.json")));
        let b = DraftStore::new(root, Some(temp.path().join("b.json")));
        assert_ne!(a.path(), b.path());
        a.write_if_newer(
            1,
            &RecoveryDraft {
                source: "{a".into(),
                if_match: revision(b"a"),
            },
        )
        .unwrap();
        assert_eq!(b.load().unwrap(), None);
    }

    #[test]
    fn changing_document_id_inside_a_slot_rejects_recovery() {
        let temp = tempfile::tempdir().unwrap();
        let store = DraftStore::new(temp.path().join("drafts"), None);
        store
            .write_if_newer(
                1,
                &RecoveryDraft {
                    source: "{broken".into(),
                    if_match: revision(b"{}"),
                },
            )
            .unwrap();
        let path = store.path();
        let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        value["documentId"] = json!("someone-else");
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(store.load().unwrap_err().contains("身份"));
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_file_names_do_not_collide() {
        use std::os::unix::ffi::OsStrExt;
        let temp = tempfile::tempdir().unwrap();
        let path_a = temp
            .path()
            .join(std::ffi::OsStr::from_bytes(b"file-\xff.json"));
        let path_b = temp
            .path()
            .join(std::ffi::OsStr::from_bytes(b"file-\xfe.json"));
        let a = DraftStore::new(temp.path().to_path_buf(), Some(path_a));
        let b = DraftStore::new(temp.path().to_path_buf(), Some(path_b));
        assert_ne!(a.path(), b.path());
    }

    #[cfg(unix)]
    #[test]
    fn draft_directory_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let store = DraftStore::new(temp.path().join("drafts"), None);
        store
            .write_if_newer(
                1,
                &RecoveryDraft {
                    source: "{}".into(),
                    if_match: revision(b"x"),
                },
            )
            .unwrap();
        assert_eq!(
            fs::metadata(store.root).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
}
