use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
    time::SystemTime,
};

use resume_one_core::export::write_export_unnamed;
use serde_json::{Value, json};

use crate::{
    CACHE_BYTES, Catalog, GithubSource, MAX_FILE_BYTES, ThemeEntry, ThemeError, VerifiedTheme,
};

const SNAPSHOT_LIMIT: usize = MAX_FILE_BYTES * 6 + 2048;

#[derive(Clone, Debug)]
pub struct ThemeCache {
    root: PathBuf,
    capacity: u64,
}

fn io(error: std::io::Error) -> ThemeError {
    ThemeError::new("THEME_CACHE_IO", error.to_string())
}

impl ThemeCache {
    pub fn new(root: &Path) -> Result<Self, ThemeError> {
        Self::with_capacity(root, CACHE_BYTES)
    }

    fn with_capacity(root: &Path, capacity: u64) -> Result<Self, ThemeError> {
        if !root.is_absolute() {
            return Err(ThemeError::new("CACHE_PATH", "缓存目录必须是绝对路径"));
        }
        fs::create_dir_all(root).map_err(io)?;
        let root = root.canonicalize().map_err(io)?;
        for name in ["catalogs", "objects"] {
            let path = root.join(name);
            if !path.exists() {
                fs::create_dir(&path).map_err(io)?;
            }
            if !fs::symlink_metadata(&path)
                .map_err(io)?
                .file_type()
                .is_dir()
            {
                return Err(ThemeError::new(
                    "CACHE_PATH",
                    "缓存子目录不能是符号链接或文件",
                ));
            }
        }
        Ok(Self { root, capacity })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path(&self, kind: &str, key: &str) -> Result<PathBuf, ThemeError> {
        let directory = self.root.join(kind);
        if !fs::symlink_metadata(&directory)
            .map_err(io)?
            .file_type()
            .is_dir()
        {
            return Err(ThemeError::new(
                "CACHE_PATH",
                "缓存子目录已被替换，拒绝访问",
            ));
        }
        Ok(directory.join(format!("{key}.json")))
    }

    fn read(path: &Path, limit: usize) -> Result<Option<Vec<u8>>, ThemeError> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(io(error)),
        };
        if !metadata.file_type().is_file() || metadata.len() > limit as u64 {
            return Err(ThemeError::new(
                "CACHE_INVALID",
                "缓存对象不是普通文件或超过大小上限",
            ));
        }
        let file = File::open(path).map_err(io)?;
        let mut bytes = Vec::new();
        (&file)
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(io)?;
        if bytes.len() > limit {
            return Err(ThemeError::new("CACHE_INVALID", "缓存对象超过大小上限"));
        }
        let _ = file.set_modified(SystemTime::now());
        Ok(Some(bytes))
    }

    pub fn catalog(&self, source: &GithubSource) -> Result<Option<Catalog>, ThemeError> {
        let Some(bytes) = Self::read(&self.path("catalogs", &source.key())?, SNAPSHOT_LIMIT)?
        else {
            return Ok(None);
        };
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| ThemeError::new("CACHE_INVALID", "清单缓存 JSON 无效"))?;
        if value["source"] != source.as_json() {
            return Err(ThemeError::new("CACHE_INVALID", "清单缓存的来源不匹配"));
        }
        let commit = value["commit"]
            .as_str()
            .ok_or_else(|| ThemeError::new("CACHE_INVALID", "清单缓存缺少 commit"))?;
        let index = value["index"]
            .as_str()
            .ok_or_else(|| ThemeError::new("CACHE_INVALID", "清单缓存缺少原始 JSON"))?;
        let mut catalog = Catalog::parse(source.clone(), commit, index.as_bytes())?;
        catalog.etag = value["etag"]
            .as_str()
            .filter(|tag| safe_etag(tag))
            .map(str::to_owned);
        Ok(Some(catalog))
    }

    pub fn theme(
        &self,
        catalog: &Catalog,
        entry: &ThemeEntry,
    ) -> Result<Option<VerifiedTheme>, ThemeError> {
        let Some(bytes) = Self::read(
            &self.path("objects", &catalog.object_key(entry))?,
            MAX_FILE_BYTES,
        )?
        else {
            return Ok(None);
        };
        VerifiedTheme::parse(entry, &bytes).map(Some)
    }

    pub(crate) fn store_catalog(&self, catalog: &Catalog) -> Result<(), ThemeError> {
        let bytes = serde_json::to_vec(&json!({"source":catalog.source.as_json(),"commit":catalog.commit,
            "index":std::str::from_utf8(&catalog.bytes).expect("validated UTF-8"),"etag":catalog.etag})).expect("JSON value");
        self.store(&self.path("catalogs", &catalog.source.key())?, &bytes)
    }

    pub(crate) fn store_theme(
        &self,
        catalog: &Catalog,
        entry: &ThemeEntry,
        theme: &VerifiedTheme,
    ) -> Result<(), ThemeError> {
        self.store(
            &self.path("objects", &catalog.object_key(entry))?,
            &theme.bytes,
        )
    }

    fn store(&self, path: &Path, bytes: &[u8]) -> Result<(), ThemeError> {
        let lock_path = self.root.join("cache.lock");
        if let Ok(metadata) = fs::symlink_metadata(&lock_path)
            && !metadata.file_type().is_file()
        {
            return Err(ThemeError::new(
                "CACHE_PATH",
                "缓存锁不能是符号链接或特殊文件",
            ));
        }
        // Serialize quota accounting and atomic commits across GUI/CLI workers.
        // No network request is made while this lock is held.
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)
            .map_err(io)?;
        lock.lock().map_err(io)?;
        if let Ok(metadata) = fs::symlink_metadata(path)
            && !metadata.file_type().is_file()
        {
            return Err(ThemeError::new(
                "CACHE_PATH",
                "不能覆盖符号链接或特殊缓存文件",
            ));
        }
        self.make_room(path, bytes.len() as u64)?;
        write_export_unnamed(path, bytes, true)
            .map_err(|error| ThemeError::new(error.code, error.message))
    }

    /// Evict only our hash-named ordinary cache objects, never follow links or recurse.
    fn make_room(&self, target: &Path, incoming: u64) -> Result<(), ThemeError> {
        if incoming > self.capacity {
            return Err(ThemeError::new("CACHE_FULL", "主题超过缓存容量"));
        }
        let mut files = Vec::new();
        let mut total = incoming;
        for kind in ["catalogs", "objects"] {
            let directory = self
                .path(kind, "")?
                .parent()
                .expect("cache directory")
                .to_path_buf();
            for item in fs::read_dir(directory).map_err(io)? {
                let item = item.map_err(io)?;
                let path = item.path();
                if path == target {
                    continue;
                }
                let metadata = fs::symlink_metadata(&path).map_err(io)?;
                let filename = item.file_name();
                let managed = filename
                    .to_str()
                    .and_then(|name| name.strip_suffix(".json"))
                    .is_some_and(|key| {
                        key.len() == 64 && key.bytes().all(|byte| byte.is_ascii_hexdigit())
                    });
                if managed && metadata.file_type().is_file() {
                    total = total.saturating_add(metadata.len());
                    files.push((
                        metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                        path,
                        metadata.len(),
                    ));
                }
            }
        }
        files.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        for (_, path, length) in files {
            if total <= self.capacity {
                break;
            }
            match fs::remove_file(path) {
                Ok(()) => total = total.saturating_sub(length),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    total = total.saturating_sub(length)
                }
                Err(error) => return Err(io(error)),
            }
        }
        if total > self.capacity {
            return Err(ThemeError::new("CACHE_FULL", "缓存空间不足"));
        }
        Ok(())
    }
}

pub(crate) fn safe_etag(tag: &str) -> bool {
    !tag.is_empty() && tag.len() <= 256 && tag.bytes().all(|byte| (32..=126).contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_eviction_is_bounded_and_does_not_remove_unmanaged_files() {
        let root = tempfile::tempdir().unwrap();
        let cache = ThemeCache::with_capacity(root.path(), 10).unwrap();
        let first = cache.path("objects", &"1".repeat(64)).unwrap();
        let second = cache.path("objects", &"2".repeat(64)).unwrap();
        let note = root.path().join("objects/note.txt");
        fs::write(&note, b"preserve").unwrap();
        cache.store(&first, b"123456").unwrap();
        cache.store(&second, b"abcdef").unwrap();
        assert!(!first.exists());
        assert_eq!(fs::read(second).unwrap(), b"abcdef");
        assert_eq!(fs::read(note).unwrap(), b"preserve");
    }

    #[test]
    fn concurrent_writers_obey_the_shared_disk_quota() {
        let root = tempfile::tempdir().unwrap();
        let cache = ThemeCache::with_capacity(root.path(), 10).unwrap();
        std::thread::scope(|scope| {
            for index in 0..8 {
                let cache = &cache;
                scope.spawn(move || {
                    let path = cache.path("objects", &crate::sha256(&[index])).unwrap();
                    cache.store(&path, b"123456").unwrap();
                });
            }
        });
        let total: u64 = fs::read_dir(root.path().join("objects"))
            .unwrap()
            .map(|entry| entry.unwrap().metadata().unwrap().len())
            .sum();
        assert!(total <= 10);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_cache_directories_and_objects() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        symlink(external.path(), root.path().join("objects")).unwrap();
        assert!(ThemeCache::new(root.path()).is_err());
        fs::remove_file(root.path().join("objects")).unwrap();
        let cache = ThemeCache::new(root.path()).unwrap();
        let file = external.path().join("private.json");
        fs::write(&file, b"private").unwrap();
        let target = cache.path("objects", &"1".repeat(64)).unwrap();
        symlink(&file, &target).unwrap();
        assert!(ThemeCache::read(&target, 100).is_err());
        assert!(cache.store(&target, b"overwrite").is_err());
        assert_eq!(fs::read(file).unwrap(), b"private");
    }
}
