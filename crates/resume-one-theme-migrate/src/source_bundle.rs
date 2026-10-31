//! Bounded, data-only snapshots of a local upstream package.
//!
//! Paths are sorted slash-separated relative paths. Portable components use
//! printable ASCII, excluding Windows-forbidden characters, device names,
//! trailing dots/spaces and case-insensitive aliases. Contents need not be text.
//! Root is depth zero; every visited entry must be within 16 components of it.
//! Excluded directory trees are disclosed, not traversed or included in the hash.
//! Metadata guards detect ordinary source changes, not an atomic/adversarial
//! filesystem snapshot. No source code is executed and no files are written.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fmt;
use std::fs::{self, File, Metadata};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const MAX_DEPTH: usize = 16;
const MAX_FILES: usize = 1024;
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ENTRIES: usize = 4096;
const EXCLUDED_DIRECTORY_NAMES: [&str; 3] = [".git", "node_modules", "target"];
// v1: domain, then (u64 big-endian UTF-8 path length, path,
// u64 big-endian content length, content) for each sorted included file.
const DIGEST_DOMAIN: &[u8] = b"resume-one-source-bundle\0v1\0";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BundleError {
    pub code: &'static str,
    pub message: String,
}

impl BundleError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn io(action: &str, path: &Path, error: std::io::Error) -> Self {
        Self::new("SOURCE_IO", format!("{action} {}: {error}", path.display()))
    }
}

impl fmt::Display for BundleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for BundleError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceFile {
    pub relative_path: String,
    pub bytes: Vec<u8>,
    pub sha256: String,
}

impl SourceFile {
    /// Borrow UTF-8 for a static parser, without discarding binary source files.
    pub fn text(&self) -> Option<&str> {
        std::str::from_utf8(&self.bytes).ok()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub files: Vec<SourceFile>,
    pub source_sha256: String,
    pub excluded_directories: Vec<String>,
}

impl Snapshot {
    /// Report metadata only: neither text nor binary contents are serialized.
    pub fn report(&self) -> Value {
        json!({
            "bundleFormat": "resume-one-source-bundle-v1",
            "complete": true,
            "coverage": "all regular files except the explicitly excluded directory trees",
            "sourceSha256": self.source_sha256,
            "files": self.files.iter().map(|file| json!({
                "path": file.relative_path,
                "sha256": file.sha256,
                "bytes": file.bytes.len(),
                "utf8": file.text().is_some()
            })).collect::<Vec<_>>(),
            "excludedDirectoryNames": EXCLUDED_DIRECTORY_NAMES,
            "excludedDirectories": self.excluded_directories
        })
    }
}

struct PendingFile {
    relative_path: String,
    path: PathBuf,
    metadata: Metadata,
}

struct Watch {
    path: PathBuf,
    metadata: Metadata,
    // Ancestors and excluded directories are identity-only: sibling/excluded
    // content changes must not invalidate the included package's snapshot.
    contents_metadata: bool,
}

#[derive(Default)]
struct Inventory {
    files: Vec<PendingFile>,
    excluded: Vec<String>,
    watches: Vec<Watch>,
    portable_paths: BTreeSet<String>,
    entries: usize,
    bytes: u64,
}

/// Return a complete bounded inventory or an error; never a partial snapshot.
pub(crate) fn snapshot(root: &Path) -> Result<Snapshot, BundleError> {
    let mut inventory = Inventory::default();
    check_root(root, &mut inventory.watches)?;
    scan_directory(root, "", 0, &mut inventory)?;
    inventory
        .files
        .sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    inventory.excluded.sort();

    let mut files = Vec::with_capacity(inventory.files.len());
    for pending in inventory.files {
        let file = read_file(&pending)?;
        inventory.watches.push(Watch {
            path: pending.path,
            metadata: pending.metadata,
            contents_metadata: true,
        });
        files.push(file);
    }
    // Also catch changes to an early file while later files were being read,
    // or directory membership changes after that directory was enumerated.
    for watch in &inventory.watches {
        let actual = observed_metadata(&watch.path)?;
        let unchanged = if watch.contents_metadata {
            same_metadata(&watch.metadata, &actual)
        } else {
            same_identity(&watch.metadata, &actual)
        };
        if !unchanged {
            return Err(changed(&watch.path));
        }
    }
    Ok(Snapshot {
        source_sha256: bundle_digest(&files),
        files,
        excluded_directories: inventory.excluded,
    })
}

fn check_root(root: &Path, watches: &mut Vec<Watch>) -> Result<(), BundleError> {
    if !root.is_absolute() {
        return Err(BundleError::new(
            "SOURCE_NOT_ABSOLUTE",
            "source directory must be absolute",
        ));
    }
    let mut current = PathBuf::new();
    for component in root.components() {
        match component {
            Component::ParentDir => {
                return Err(BundleError::new(
                    "SOURCE_ROOT_PATH",
                    "source root cannot contain '..'",
                ));
            }
            Component::CurDir => continue,
            _ => current.push(component.as_os_str()),
        }
        // A Windows drive prefix alone is not an absolute directory yet.
        if !current.is_absolute() {
            continue;
        }
        let metadata = observed_metadata(&current)?;
        if !metadata.is_dir() {
            return Err(BundleError::new(
                "SOURCE_NOT_DIRECTORY",
                format!(
                    "source root/ancestor is not a directory: {}",
                    current.display()
                ),
            ));
        }
        watches.push(Watch {
            path: current.clone(),
            metadata,
            contents_metadata: false,
        });
    }
    Ok(())
}

fn observed_metadata(path: &Path) -> Result<Metadata, BundleError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| BundleError::io("cannot inspect", path, error))?;
    if metadata.file_type().is_symlink() {
        return Err(BundleError::new(
            "SOURCE_SYMLINK",
            format!("symbolic links are not allowed: {}", path.display()),
        ));
    }
    Ok(metadata)
}

fn portable_component(name: &str) -> Result<(), BundleError> {
    let invalid = name.is_empty()
        || matches!(name, "." | "..")
        || name.ends_with('.')
        || name.ends_with(' ')
        || name
            .bytes()
            .any(|byte| !(0x20..=0x7e).contains(&byte) || b"<>:\"/\\|?*".contains(&byte));
    let stem = name
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(*prefix).is_some_and(|suffix| {
                suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9')
            })
        });
    if invalid || device {
        return Err(BundleError::new(
            "SOURCE_NON_PORTABLE_PATH",
            format!("not a portable relative path component: {name:?}"),
        ));
    }
    Ok(())
}

fn validated_component(name: &OsStr) -> Result<&str, BundleError> {
    let name = name.to_str().ok_or_else(|| {
        BundleError::new("SOURCE_PATH_NOT_UTF8", "source relative path is not UTF-8")
    })?;
    portable_component(name)?;
    Ok(name)
}

fn scan_directory(
    path: &Path,
    relative: &str,
    depth: usize,
    inventory: &mut Inventory,
) -> Result<(), BundleError> {
    let metadata = observed_metadata(path)?;
    if !metadata.is_dir() {
        return Err(changed(path));
    }
    inventory.watches.push(Watch {
        path: path.to_owned(),
        metadata,
        contents_metadata: true,
    });
    let mut entries = Vec::new();
    for entry in fs::read_dir(path).map_err(|error| BundleError::io("cannot list", path, error))? {
        let entry = entry.map_err(|error| BundleError::io("cannot list entry in", path, error))?;
        inventory.entries += 1;
        if inventory.entries > MAX_ENTRIES {
            return Err(BundleError::new(
                "SOURCE_ENTRY_LIMIT",
                "source exceeds 4096 visited entries",
            ));
        }
        entries.push(entry);
    }
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        if depth + 1 > MAX_DEPTH {
            return Err(BundleError::new(
                "SOURCE_DEPTH_LIMIT",
                "source exceeds depth 16",
            ));
        }
        let name = entry.file_name();
        let name = validated_component(&name)?;
        let relative_path = if relative.is_empty() {
            name.to_owned()
        } else {
            format!("{relative}/{name}")
        };
        if !inventory
            .portable_paths
            .insert(relative_path.to_ascii_lowercase())
        {
            return Err(BundleError::new(
                "SOURCE_PATH_COLLISION",
                format!("case-insensitive relative path collision: {relative_path}"),
            ));
        }
        let path = entry.path();
        let metadata = observed_metadata(&path)?;
        if metadata.is_dir() {
            if EXCLUDED_DIRECTORY_NAMES.contains(&name) {
                inventory.excluded.push(relative_path);
                inventory.watches.push(Watch {
                    path,
                    metadata,
                    contents_metadata: false,
                });
            } else {
                scan_directory(&path, &relative_path, depth + 1, inventory)?;
            }
        } else if metadata.is_file() {
            if inventory.files.len() >= MAX_FILES {
                return Err(BundleError::new(
                    "SOURCE_FILE_LIMIT",
                    "source exceeds 1024 files",
                ));
            }
            if metadata.len() > MAX_FILE_BYTES {
                return Err(BundleError::new(
                    "SOURCE_FILE_TOO_LARGE",
                    format!("file exceeds 8 MiB: {relative_path}"),
                ));
            }
            inventory.bytes += metadata.len();
            if inventory.bytes > MAX_TOTAL_BYTES {
                return Err(BundleError::new(
                    "SOURCE_TOTAL_TOO_LARGE",
                    "source exceeds 64 MiB",
                ));
            }
            inventory.files.push(PendingFile {
                relative_path,
                path,
                metadata,
            });
        } else {
            return Err(BundleError::new(
                "SOURCE_SPECIAL_FILE",
                format!("only ordinary files/directories are allowed: {relative_path}"),
            ));
        }
    }
    Ok(())
}

fn read_file(pending: &PendingFile) -> Result<SourceFile, BundleError> {
    let before = observed_metadata(&pending.path)?;
    if !same_metadata(&pending.metadata, &before) {
        return Err(changed(&pending.path));
    }
    let mut file = File::open(&pending.path)
        .map_err(|error| BundleError::io("cannot open", &pending.path, error))?;
    let opened = file
        .metadata()
        .map_err(|error| BundleError::io("cannot inspect open file", &pending.path, error))?;
    if !same_metadata(&before, &opened) {
        return Err(changed(&pending.path));
    }
    let mut bytes = Vec::with_capacity(before.len() as usize);
    (&mut file)
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| BundleError::io("cannot read", &pending.path, error))?;
    let after = file
        .metadata()
        .map_err(|error| BundleError::io("cannot inspect read file", &pending.path, error))?;
    let path_after = observed_metadata(&pending.path)?;
    if bytes.len() as u64 != before.len()
        || !same_metadata(&before, &after)
        || !same_metadata(&before, &path_after)
    {
        return Err(changed(&pending.path));
    }
    Ok(SourceFile {
        relative_path: pending.relative_path.clone(),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        bytes,
    })
}

fn changed(path: &Path) -> BundleError {
    BundleError::new(
        "SOURCE_CHANGED",
        format!("source changed during snapshot: {}", path.display()),
    )
}

fn same_identity(before: &Metadata, after: &Metadata) -> bool {
    if before.is_dir() != after.is_dir()
        || before.is_file() != after.is_file()
        || before.created().ok() != after.created().ok()
    {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if before.dev() != after.dev() || before.ino() != after.ino() {
            return false;
        }
    }
    true
}

fn same_metadata(before: &Metadata, after: &Metadata) -> bool {
    if !same_identity(before, after)
        || before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
        || before.permissions().readonly() != after.permissions().readonly()
    {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if before.ctime() != after.ctime() || before.ctime_nsec() != after.ctime_nsec() {
            return false;
        }
    }
    true
}

fn bundle_digest(files: &[SourceFile]) -> String {
    let mut digest = Sha256::new();
    digest.update(DIGEST_DOMAIN);
    for file in files {
        digest.update((file.relative_path.len() as u64).to_be_bytes());
        digest.update(file.relative_path.as_bytes());
        digest.update((file.bytes.len() as u64).to_be_bytes());
        digest.update(&file.bytes);
    }
    format!("{:x}", digest.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct TestDir(PathBuf);
    impl TestDir {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            // macOS temp_dir can contain a /var symlink, which production must reject.
            let parent = std::env::temp_dir().canonicalize().unwrap();
            loop {
                let path = parent.join(format!(
                    "rsb-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("cannot create test directory: {error}"),
                }
            }
        }
        fn write(&self, path: &str, bytes: &[u8]) {
            let path = self.0.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
    }
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn error(root: &Path, code: &str) {
        assert_eq!(snapshot(root).unwrap_err().code, code);
    }

    #[test]
    fn sorts_all_files_retains_deep_sources_binary_fonts_and_licenses_without_report_contents() {
        let root = TestDir::new();
        root.write("z.json", b"{\"last\":true}");
        root.write(
            "src/deep/nested/components/Resume.jsx",
            b"export default 'never execute';",
        );
        let binary = b"\0\x01\0\0\xff\xfeFONT";
        root.write("fonts/Original[wght].ttf", binary);
        root.write("LICENSE.txt", b"Full license and copyright text\n");
        let result = snapshot(&root.0).unwrap();
        assert_eq!(
            result
                .files
                .iter()
                .map(|file| file.relative_path.as_str())
                .collect::<Vec<_>>(),
            [
                "LICENSE.txt",
                "fonts/Original[wght].ttf",
                "src/deep/nested/components/Resume.jsx",
                "z.json"
            ]
        );
        assert_eq!(result.files[1].bytes.as_slice(), binary);
        assert!(result.files[1].text().is_none());
        assert_eq!(
            result.files[0].text(),
            Some("Full license and copyright text\n")
        );
        assert_eq!(
            result.files[1].sha256,
            format!("{:x}", Sha256::digest(binary))
        );
        let report = result.report();
        assert_eq!(report["complete"], true);
        assert_eq!(report["files"][1]["bytes"], binary.len());
        assert!(report["files"][0].get("text").is_none());
        assert!(report["files"][0].get("contents").is_none());
        assert!(
            !report
                .to_string()
                .contains("Full license and copyright text")
        );
    }

    #[test]
    fn digest_is_order_independent_but_content_and_relative_path_sensitive() {
        let a = TestDir::new();
        let b = TestDir::new();
        a.write("b.txt", b"B");
        a.write("a.txt", b"A");
        b.write("a.txt", b"A");
        b.write("b.txt", b"B");
        let original = snapshot(&a.0).unwrap();
        assert_eq!(
            original.source_sha256,
            snapshot(&b.0).unwrap().source_sha256
        );
        b.write("a.txt", b"changed");
        assert_ne!(
            original.source_sha256,
            snapshot(&b.0).unwrap().source_sha256
        );
        b.write("a.txt", b"A");
        fs::rename(b.0.join("a.txt"), b.0.join("renamed.txt")).unwrap();
        assert_ne!(
            original.source_sha256,
            snapshot(&b.0).unwrap().source_sha256
        );
        let mut expected = Sha256::new();
        expected.update(b"resume-one-source-bundle\0v1\0");
        for (path, bytes) in [
            (b"a.txt".as_slice(), b"A".as_slice()),
            (b"b.txt".as_slice(), b"B".as_slice()),
        ] {
            expected.update((path.len() as u64).to_be_bytes());
            expected.update(path);
            expected.update((bytes.len() as u64).to_be_bytes());
            expected.update(bytes);
        }
        assert_eq!(original.source_sha256, format!("{:x}", expected.finalize()));
    }

    #[test]
    fn ignored_names_apply_only_to_directories_and_are_fully_disclosed() {
        let root = TestDir::new();
        root.write("target/ignored", b"not hashed");
        root.write(".git/config", b"not hashed");
        root.write("src/node_modules/package.json", b"not hashed");
        for name in EXCLUDED_DIRECTORY_NAMES {
            root.write(&format!("kept/{name}"), b"ordinary file");
        }
        let result = snapshot(&root.0).unwrap();
        assert_eq!(
            result.excluded_directories,
            [".git", "src/node_modules", "target"]
        );
        assert_eq!(
            result
                .files
                .iter()
                .map(|file| file.relative_path.as_str())
                .collect::<Vec<_>>(),
            ["kept/.git", "kept/node_modules", "kept/target"]
        );
        root.write(
            "target/ignored",
            b"excluded changes do not alter the included digest",
        );
        assert_eq!(
            result.source_sha256,
            snapshot(&root.0).unwrap().source_sha256
        );
        assert_eq!(
            result.report()["excludedDirectories"],
            json!([".git", "src/node_modules", "target"])
        );
    }

    #[test]
    fn rejects_relative_roots_regular_file_roots_and_nonportable_components() {
        error(Path::new("relative"), "SOURCE_NOT_ABSOLUTE");
        let root = TestDir::new();
        root.write("plain.txt", b"file");
        error(&root.0.join("plain.txt"), "SOURCE_NOT_DIRECTORY");
        error(&root.0.join(".."), "SOURCE_ROOT_PATH");
        for name in [
            "",
            ".",
            "..",
            "bad\\name",
            "bad:name",
            "bad?name",
            "bad\nname",
            "trailing.",
            "trailing ",
            "CON",
            "nul.txt",
            "COM1.ttf",
            "LPT9",
            "CON .txt",
            "非portable",
        ] {
            assert_eq!(
                portable_component(name).unwrap_err().code,
                "SOURCE_NON_PORTABLE_PATH",
                "{name:?}"
            );
        }
        for name in [
            ".git",
            "RobotoSlab[wght].ttf",
            "Open Font License.txt",
            "component.jsx",
        ] {
            portable_component(name).unwrap();
        }
    }

    #[test]
    fn enforces_depth_and_file_size_boundaries() {
        let root = TestDir::new();
        let allowed = format!("{}file.bin", "nested/".repeat(MAX_DEPTH - 1));
        root.write(&allowed, b"deep ordinary file");
        assert_eq!(snapshot(&root.0).unwrap().files[0].relative_path, allowed);
        fs::create_dir_all(root.0.join("too-deep/".repeat(MAX_DEPTH + 1))).unwrap();
        error(&root.0, "SOURCE_DEPTH_LIMIT");
        let root = TestDir::new();
        let file = File::create(root.0.join("font.ttf")).unwrap();
        file.set_len(MAX_FILE_BYTES).unwrap();
        assert_eq!(
            snapshot(&root.0).unwrap().files[0].bytes.len() as u64,
            MAX_FILE_BYTES
        );
        file.set_len(MAX_FILE_BYTES + 1).unwrap();
        error(&root.0, "SOURCE_FILE_TOO_LARGE");
    }

    #[test]
    fn enforces_file_count_total_bytes_and_entry_limits() {
        let root = TestDir::new();
        for index in 0..MAX_FILES {
            root.write(&format!("{index:04}.txt"), b"");
        }
        assert_eq!(snapshot(&root.0).unwrap().files.len(), MAX_FILES);
        root.write("overflow.txt", b"");
        error(&root.0, "SOURCE_FILE_LIMIT");
        let root = TestDir::new();
        for index in 0..(MAX_TOTAL_BYTES / MAX_FILE_BYTES) {
            File::create(root.0.join(format!("{index}.bin")))
                .unwrap()
                .set_len(MAX_FILE_BYTES)
                .unwrap();
        }
        let accepted = snapshot(&root.0).unwrap();
        assert_eq!(
            accepted
                .files
                .iter()
                .map(|file| file.bytes.len() as u64)
                .sum::<u64>(),
            MAX_TOTAL_BYTES
        );
        drop(accepted);
        root.write("overflow.bin", b"x");
        error(&root.0, "SOURCE_TOTAL_TOO_LARGE");
        let root = TestDir::new();
        for index in 0..MAX_ENTRIES {
            fs::create_dir(root.0.join(format!("dir-{index:04}"))).unwrap();
        }
        assert!(snapshot(&root.0).unwrap().files.is_empty());
        fs::create_dir(root.0.join("overflow")).unwrap();
        error(&root.0, "SOURCE_ENTRY_LIMIT");
    }

    #[test]
    fn detects_obvious_changes_between_inventory_and_read() {
        let root = TestDir::new();
        root.write("source.jsx", b"before");
        let path = root.0.join("source.jsx");
        let pending = PendingFile {
            relative_path: "source.jsx".into(),
            metadata: observed_metadata(&path).unwrap(),
            path,
        };
        root.write("source.jsx", b"after with different size");
        assert_eq!(read_file(&pending).unwrap_err().code, "SOURCE_CHANGED");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_roots_ancestors_files_directories_and_exclusion_aliases() {
        use std::os::unix::fs::symlink;
        let base = TestDir::new();
        let real = TestDir::new();
        real.write("inner/source.jsx", b"source");
        symlink(&real.0, base.0.join("root-link")).unwrap();
        error(&base.0.join("root-link"), "SOURCE_SYMLINK");
        error(&base.0.join("root-link/inner"), "SOURCE_SYMLINK");
        for name in ["file-link", "directory-link", "node_modules"] {
            let source = TestDir::new();
            symlink(
                if name == "file-link" {
                    real.0.join("inner/source.jsx")
                } else {
                    real.0.join("inner")
                },
                source.0.join(name),
            )
            .unwrap();
            error(&source.0, "SOURCE_SYMLINK");
        }
    }

    #[cfg(unix)]
    #[test]
    fn rejects_non_utf8_os_str_without_requiring_filesystem_support() {
        use std::os::unix::ffi::OsStringExt;
        // APFS rejects these bytes at creation time; exercise the same production
        // validator directly instead of relying on the filesystem accepting them.
        let invalid = std::ffi::OsString::from_vec(vec![0xff]);
        assert_eq!(
            validated_component(&invalid).unwrap_err().code,
            "SOURCE_PATH_NOT_UTF8"
        );
        assert_eq!(
            validated_component(OsStr::new("source.jsx")).unwrap(),
            "source.jsx"
        );
        assert_eq!(
            validated_component(OsStr::new("bad:name"))
                .unwrap_err()
                .code,
            "SOURCE_NON_PORTABLE_PATH"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn rejects_non_utf8_names_on_disk_when_filesystem_supports_them() {
        use std::os::unix::ffi::OsStringExt;
        let root = TestDir::new();
        root.write("valid-name", b"filesystem writable");
        if let Err(error) = fs::write(
            root.0.join(std::ffi::OsString::from_vec(vec![0xff])),
            b"data",
        ) {
            // Some Linux-mounted filesystems also reject invalid UTF-8. Other
            // I/O failures remain test failures, rather than hiding fixture bugs.
            if error.kind() == std::io::ErrorKind::InvalidInput || error.raw_os_error() == Some(84)
            // Linux EILSEQ
            {
                eprintln!("filesystem rejected non-UTF-8 fixture name: {error}");
                return;
            }
            panic!("cannot create non-UTF-8 fixture: {error}");
        }
        error(&root.0, "SOURCE_PATH_NOT_UTF8");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_special_files() {
        use std::os::unix::net::UnixListener;
        let root = TestDir::new();
        let _socket = UnixListener::bind(root.0.join("socket")).unwrap();
        error(&root.0, "SOURCE_SPECIAL_FILE");
    }
}
