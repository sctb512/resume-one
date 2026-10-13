use std::io::Read;
use std::path::{Path, PathBuf};

use resume_one_core::editor::ResumeBuffer;
use resume_one_core::resume::{MAX_RESUME_BYTES, ResumeDocument};
use resume_one_core::semantic::{ResumeContent, project};

pub enum OpenedPath {
    File(Box<OpenedFile>),
    Folder(LocalFolder),
}

impl OpenedPath {
    pub fn read(path: &Path) -> Result<Self, String> {
        if path.is_dir() {
            LocalFolder::scan(path).map(Self::Folder)
        } else {
            OpenedFile::read(path).map(Box::new).map(Self::File)
        }
    }
}

pub struct OpenedFile {
    pub path: PathBuf,
    pub source: Vec<u8>,
    pub document: Option<ResumeDocument>,
    pub content: Option<ResumeContent>,
}

impl OpenedFile {
    pub fn read(path: &Path) -> Result<Self, String> {
        if !is_json(path) {
            return Err("请选择 .json 格式的简历文件。".into());
        }
        let path = path
            .canonicalize()
            .map_err(|error| format!("无法访问文件：{error}"))?;
        if !path.is_file() {
            return Err("选择的路径不是文件。".into());
        }
        let mut source = Vec::new();
        std::fs::File::open(&path)
            .and_then(|file| {
                file.take(MAX_RESUME_BYTES as u64 + 1)
                    .read_to_end(&mut source)
            })
            .map_err(|error| format!("无法读取文件：{error}"))?;
        if source.len() > MAX_RESUME_BYTES {
            return Err("简历文件超过 16 MiB 上限".into());
        }
        let buffer = ResumeBuffer::new(source.clone()).map_err(|issue| issue.message)?;
        let document = buffer.last_valid().cloned();
        let content = document.as_ref().map(project);
        Ok(Self {
            path,
            source,
            document,
            content,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalFolder {
    pub path: PathBuf,
    pub files: Vec<PathBuf>,
}

impl LocalFolder {
    pub fn scan(path: &Path) -> Result<Self, String> {
        let path = path
            .canonicalize()
            .map_err(|error| format!("无法访问目录：{error}"))?;
        let entries = std::fs::read_dir(&path).map_err(|error| format!("无法读取目录：{error}"))?;
        let mut files = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| format!("无法读取目录中的文件：{error}"))?;
            if entry
                .file_type()
                .map_err(|error| format!("无法读取文件信息：{error}"))?
                .is_file()
                && is_json(&entry.path())
            {
                files.push(entry.path());
                if files.len() > 10_000 {
                    return Err("目录中 JSON 文件过多，请选择更小的目录。".into());
                }
            }
        }
        files.sort_by_cached_key(|path| {
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase()
        });
        Ok(Self { path, files })
    }
}

fn is_json(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_lists_only_immediate_regular_json_files_without_reading_content() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("B.JSON"), b"invalid JSON").unwrap();
        std::fs::write(directory.path().join("a.json"), b"{}").unwrap();
        std::fs::write(directory.path().join("notes.txt"), b"private").unwrap();
        std::fs::create_dir(directory.path().join("nested.json")).unwrap();
        std::fs::write(directory.path().join("nested.json/hidden.json"), b"{}").unwrap();
        let folder = LocalFolder::scan(directory.path()).unwrap();
        let names: Vec<_> = folder
            .files
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, ["a.json", "B.JSON"]);
        assert!(folder.path.is_absolute());
    }

    #[test]
    fn unicode_file_is_loaded_without_changing_its_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("我的 简历.JSON");
        let source = br#"{ "basics": { "name": "User" }, "custom": [1, 2] }"#;
        std::fs::write(&path, source).unwrap();
        let opened = OpenedFile::read(&path).unwrap();
        assert_eq!(opened.content.unwrap().name, "User");
        assert_eq!(opened.document.unwrap().source, source);
        assert_eq!(opened.source, source);
        assert_eq!(std::fs::read(&path).unwrap(), source);
        assert!(opened.path.is_absolute());
    }

    #[test]
    fn empty_folder_and_failures_are_distinct() {
        let directory = tempfile::tempdir().unwrap();
        assert!(
            LocalFolder::scan(directory.path())
                .unwrap()
                .files
                .is_empty()
        );
        assert!(LocalFolder::scan(&directory.path().join("missing")).is_err());
        let invalid = directory.path().join("invalid.json");
        std::fs::write(&invalid, b"{\"work\":[{\"name\":42}]}").unwrap();
        let opened = OpenedFile::read(&invalid).unwrap();
        assert!(opened.document.is_none());
        assert!(opened.content.is_none());
        assert_eq!(opened.source, b"{\"work\":[{\"name\":42}]}");
        assert!(OpenedFile::read(directory.path()).is_err());
        assert!(OpenedFile::read(&directory.path().join("missing.json")).is_err());
    }
}
