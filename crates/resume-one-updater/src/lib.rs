//! Update only a tagged, packaged installation from the public release repository.
use std::{
    collections::HashSet,
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use semver::Version;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use zip::ZipArchive;

const API: &str = "https://api.github.com/repos/sctb512/resume-one/releases?per_page=20";
const DOWNLOAD: &str = "https://github.com/sctb512/resume-one/releases/download";
const MAX_ARCHIVE: u64 = 768 * 1024 * 1024;
const MAX_EXTRACTED: u64 = 2 * 1024 * 1024 * 1024;
pub const APPLY_ARG: &str = "--internal-apply-staged-update";
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Platform {
    Mac,
    Windows,
}

impl Platform {
    fn current() -> Option<Self> {
        if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            Some(Self::Mac)
        } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            Some(Self::Windows)
        } else {
            None
        }
    }

    fn archive(self) -> &'static str {
        match self {
            Self::Mac => "Resume-One-macOS-arm64-unsigned.zip",
            Self::Windows => "Resume-One-Windows-x64-unsigned.zip",
        }
    }

    fn archive_root(self) -> &'static str {
        match self {
            Self::Mac => "Resume One.app",
            Self::Windows => "Resume One",
        }
    }

    fn app(self) -> &'static str {
        match self {
            Self::Mac => "Contents/MacOS/resume-one-app",
            Self::Windows => "resume-one-app.exe",
        }
    }

    fn cli(self) -> &'static str {
        match self {
            Self::Mac => "Contents/MacOS/resume-one",
            Self::Windows => "resume-one.exe",
        }
    }

    fn marker(self) -> &'static str {
        match self {
            Self::Mac => "Contents/Resources/release-tag.txt",
            Self::Windows => "release-tag.txt",
        }
    }
}

fn version(tag: &str) -> Option<Version> {
    let suffix = tag.strip_prefix('v')?;
    let version = Version::parse(suffix).ok()?;
    (suffix.len() <= 80 && version.build.is_empty()).then_some(version)
}

#[derive(Clone)]
pub struct Installation {
    root: PathBuf,
    platform: Platform,
    current: Version,
}

impl Installation {
    pub fn detect() -> Option<Self> {
        let platform = Platform::current()?;
        let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
        let parent = exe.parent()?;
        let root = match platform {
            Platform::Mac => parent.parent()?.parent()?,
            Platform::Windows => parent,
        };
        if root.join(platform.app()).canonicalize().ok()? != exe {
            return None;
        }
        Self::open(root, platform).ok()
    }

    fn open(root: &Path, platform: Platform) -> Result<Self> {
        if !root.is_absolute() || root.parent().is_none() {
            return Err("安装目录无效".into());
        }
        let expected_name = match platform {
            Platform::Mac => root.extension().is_some_and(|ext| ext == "app"),
            Platform::Windows => root.file_name().is_some_and(|name| name == "Resume One"),
        };
        if !expected_name || !regular_directory(root) {
            return Err("请从解压后的 Resume One 应用目录运行更新".into());
        }
        for file in [platform.app(), platform.cli(), platform.marker()] {
            if !regular_file(&root.join(file)) {
                return Err("安装目录缺少应用、CLI 或版本标记".into());
            }
        }
        let marker = fs::read_to_string(root.join(platform.marker()))?;
        let current = version(marker.trim()).ok_or("安装版本标记无效")?;
        Ok(Self {
            root: root.into(),
            platform,
            current,
        })
    }

    pub fn version(&self) -> String {
        format!("v{}", self.current)
    }

    pub fn contains(&self, path: &Path) -> bool {
        path.canonicalize()
            .unwrap_or_else(|_| path.to_owned())
            .starts_with(&self.root)
    }

    pub fn check(&self) -> Result<Option<Release>> {
        let mut reply = agent(Duration::from_secs(15), 0)
            .get(API)
            .header("Accept", "application/vnd.github+json")
            .call()?;
        let mut data = Vec::new();
        reply
            .body_mut()
            .as_reader()
            .take(2 * 1024 * 1024 + 1)
            .read_to_end(&mut data)?;
        if data.len() > 2 * 1024 * 1024 {
            return Err("更新列表超过大小限制".into());
        }
        select_release(
            &serde_json::from_slice(&data)?,
            &self.current,
            self.platform,
        )
    }
}

fn regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_file())
}

fn regular_directory(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_dir())
}

fn agent(timeout: Duration, redirects: u32) -> ureq::Agent {
    ureq::Agent::config_builder()
        .https_only(true)
        .max_redirects(redirects)
        .timeout_global(Some(timeout))
        .user_agent("Resume-One-Updater/0.1")
        .build()
        .into()
}

#[derive(Clone, Debug)]
pub struct Release {
    pub tag: String,
    url: String,
    digest: String,
    size: u64,
}

fn select_release(list: &Value, current: &Version, platform: Platform) -> Result<Option<Release>> {
    let list = list.as_array().ok_or("GitHub 更新列表格式无效")?;
    Ok(list
        .iter()
        .filter_map(|entry| {
            let tag = entry["tag_name"].as_str()?;
            let next = version(tag)?;
            let prerelease = entry["prerelease"].as_bool()?;
            if entry["draft"].as_bool()?
                || &next <= current
                || (current.pre.is_empty() && prerelease)
            {
                return None;
            }
            let asset = entry["assets"]
                .as_array()?
                .iter()
                .find(|asset| asset["name"] == platform.archive())?;
            let url = format!("{DOWNLOAD}/{tag}/{}", platform.archive());
            if asset["browser_download_url"].as_str()? != url {
                return None;
            }
            let digest = asset["digest"].as_str()?.strip_prefix("sha256:")?;
            if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return None;
            }
            let size = asset["size"].as_u64()?;
            if size == 0 || size > MAX_ARCHIVE {
                return None;
            }
            Some((
                next,
                Release {
                    tag: tag.into(),
                    url,
                    digest: digest.to_ascii_lowercase(),
                    size,
                },
            ))
        })
        .max_by(|(a, _), (b, _)| a.cmp(b))
        .map(|(_, release)| release))
}

pub struct Prepared {
    directory: TempDir,
    installation: Installation,
    tag: String,
}

impl Release {
    pub fn prepare(self, installation: Installation) -> Result<Prepared> {
        let parent = installation.root.parent().ok_or("安装目录缺少父目录")?;
        let directory = tempfile::Builder::new()
            .prefix(".resume-one-update-")
            .tempdir_in(parent)?;
        let archive_path = directory.path().join("update.zip");
        let mut reply = agent(Duration::from_secs(600), 4).get(&self.url).call()?;
        let mut archive = File::create_new(&archive_path)?;
        let mut digest = Sha256::new();
        let mut remaining = reply.body_mut().as_reader().take(self.size + 1);
        let mut block = vec![0; 128 * 1024];
        let mut count = 0;
        loop {
            let length = remaining.read(&mut block)?;
            if length == 0 {
                break;
            }
            count += length as u64;
            if count > self.size {
                return Err("更新包长度不匹配".into());
            }
            digest.update(&block[..length]);
            archive.write_all(&block[..length])?;
        }
        if count != self.size || format!("{:x}", digest.finalize()) != self.digest {
            return Err("更新包 SHA-256 校验失败".into());
        }
        archive.sync_all()?;
        drop(archive);
        unpack(&archive_path, directory.path(), installation.platform)?;
        let staged = directory.path().join(installation.platform.archive_root());
        let updated = Installation::open(&staged, installation.platform)?;
        if updated.version() != self.tag || updated.current <= installation.current {
            return Err("更新包版本与发布版本不匹配".into());
        }
        #[cfg(target_os = "macos")]
        if !Command::new("/usr/bin/codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(&staged)
            .output()?
            .status
            .success()
        {
            return Err("更新后的 macOS 应用封签校验失败".into());
        }
        fs::remove_file(archive_path)?;
        Ok(Prepared {
            directory,
            installation,
            tag: self.tag,
        })
    }
}

fn unpack(file: &Path, directory: &Path, platform: Platform) -> Result<()> {
    let mut archive = ZipArchive::new(File::open(file)?)?;
    if archive.is_empty() || archive.len() > 8_000 {
        return Err("更新包文件数量无效".into());
    }
    let mut names = HashSet::new();
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let path = entry.enclosed_name().ok_or("更新包包含越界路径")?;
        let mut components = path.components();
        let root = components.next();
        let resource =
            platform == Platform::Mac && root == Some(Component::Normal("__MACOSX".as_ref()));
        if (!resource && root != Some(Component::Normal(platform.archive_root().as_ref())))
            || path.components().any(|part| !matches!(part, Component::Normal(name) if name.to_str().is_some_and(|name| !name.contains([':', '\\']) && !name.ends_with([' ', '.']))))
            || !names.insert(path.to_string_lossy().to_ascii_lowercase())
            || entry.unix_mode().is_some_and(|mode| !matches!(mode & 0o170000, 0 | 0o040000 | 0o100000))
        {
            return Err("更新包包含非法、重复或非普通路径".into());
        }
        total = total
            .checked_add(entry.size())
            .ok_or("更新包展开大小超限")?;
        if total > MAX_EXTRACTED {
            return Err("更新包展开大小超限".into());
        }
        if platform == Platform::Windows {
            let target = directory.join(&path);
            if entry.is_dir() {
                fs::create_dir_all(&target)?;
            } else {
                fs::create_dir_all(target.parent().ok_or("更新包缺少目标目录")?)?;
                let mut output = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(target)?;
                if io::copy(&mut entry, &mut output)? != entry.size() {
                    return Err("更新包文件长度不匹配".into());
                }
            }
        }
    }
    if platform == Platform::Mac
        && !Command::new("/usr/bin/ditto")
            .args(["-x", "-k"])
            .arg(file)
            .arg(directory)
            .output()?
            .status
            .success()
    {
        return Err("无法解压 macOS 更新包".into());
    }
    Ok(())
}

impl Prepared {
    /// Start the helper from a sibling directory so Windows can rename the app after it exits.
    pub fn handoff(self, reopen: Option<&Path>) -> Result<()> {
        let helper =
            self.directory
                .path()
                .join(if self.installation.platform == Platform::Windows {
                    "apply-update.exe"
                } else {
                    "apply-update"
                });
        fs::copy(
            self.installation
                .root
                .join(self.installation.platform.cli()),
            &helper,
        )?;
        let log = File::create_new(self.directory.path().join("update.log"))?;
        let mut command = Command::new(&helper);
        command
            .arg(APPLY_ARG)
            .arg(std::process::id().to_string())
            .arg(&self.installation.root)
            .arg(&self.tag);
        if let Some(reopen) =
            reopen.filter(|path| path.is_absolute() && !self.installation.contains(path))
        {
            command.arg(reopen);
        }
        command
            .current_dir(self.directory.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        command.spawn()?;
        let _ = self.directory.keep();
        Ok(())
    }
}

pub fn apply_from_helper(arguments: impl IntoIterator<Item = OsString>) -> Result<()> {
    let mut args = arguments.into_iter();
    let pid = args
        .next()
        .and_then(|value| value.into_string().ok())
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|pid| (1..=i32::MAX as u32).contains(pid))
        .ok_or("更新进程 ID 无效")?;
    let root = PathBuf::from(args.next().ok_or("缺少应用目录")?);
    let tag = args
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or("缺少发布版本")?;
    let reopen = args.next().map(PathBuf::from);
    if args.next().is_some() || reopen.as_deref().is_some_and(|path| !path.is_absolute()) {
        return Err("更新参数无效".into());
    }
    let platform = Platform::current().ok_or("当前平台不支持自动更新")?;
    let helper = std::env::current_exe()?.canonicalize()?;
    let directory = helper.parent().ok_or("更新目录无效")?;
    if directory.parent() != root.parent()
        || !directory
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(".resume-one-update-"))
    {
        return Err("更新目录须位于原应用旁边".into());
    }
    wait_for_exit(pid)?;
    let lock_path = root
        .parent()
        .ok_or("安装目录缺少父目录")?
        .join(".resume-one-update.lock");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)?;
    lock.try_lock().map_err(|_| "此目录已有简里更新正在安装")?;
    let install = Installation::open(&root, platform)?;
    let staged = Installation::open(&directory.join(platform.archive_root()), platform)?;
    if staged.version() != tag || staged.current <= install.current {
        return Err("更新版本与本机安装版本不匹配".into());
    }
    let backup = directory.join("previous");
    let launch = || {
        let mut command = Command::new(install.root.join(platform.app()));
        if let Some(path) = reopen.as_deref().filter(|path| !install.contains(path)) {
            command.arg(path);
        }
        command.current_dir(&install.root).spawn().map(|_| ())
    };
    let result = replace_installation(&install.root, &staged.root, &backup, launch);
    if result.is_err() && install.root.is_dir() {
        let _ = launch();
    }
    result
}

fn replace_installation(
    root: &Path,
    stage: &Path,
    backup: &Path,
    launch: impl FnOnce() -> io::Result<()>,
) -> Result<()> {
    if fs::symlink_metadata(backup).is_ok() {
        return Err("更新备份已存在".into());
    }
    fs::rename(root, backup)?;
    if let Err(error) = fs::rename(stage, root) {
        fs::rename(backup, root).map_err(|rollback| {
            format!(
                "{error}；恢复失败 {rollback}，旧版位于 {}",
                backup.display()
            )
        })?;
        return Err(error.into());
    }
    if let Err(error) = launch() {
        fs::rename(root, stage)
            .and_then(|()| fs::rename(backup, root))
            .map_err(|rollback| {
                format!(
                    "{error}；恢复失败 {rollback}，旧版位于 {}",
                    backup.display()
                )
            })?;
        return Err(error.into());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn wait_for_exit(pid: u32) -> Result<()> {
    let deadline = std::time::Instant::now() + Duration::from_secs(90);
    while std::time::Instant::now() < deadline {
        // Signal 0 only checks process existence.
        if unsafe { libc::kill(pid as i32, 0) } < 0
            && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    Err("等待旧版退出超时".into())
}

#[cfg(target_os = "windows")]
fn wait_for_exit(pid: u32) -> Result<()> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, WAIT_OBJECT_0},
        System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
    };
    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if handle.is_null() {
        let error = io::Error::last_os_error();
        return if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
            Ok(())
        } else {
            Err(error.into())
        };
    }
    let result = unsafe { WaitForSingleObject(handle, 90_000) };
    unsafe { CloseHandle(handle) };
    if result == WAIT_OBJECT_0 {
        Ok(())
    } else {
        Err("等待旧版退出超时".into())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn wait_for_exit(_: u32) -> Result<()> {
    Err("当前平台不支持自动更新".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn release(tag: &str, prerelease: bool) -> Value {
        let name = Platform::Mac.archive();
        json!({"tag_name": tag, "prerelease": prerelease, "draft": false,
            "assets": [{"name":name, "browser_download_url":format!("{DOWNLOAD}/{tag}/{name}"),
                "digest":format!("sha256:{}", "a".repeat(64)), "size":1000}]})
    }

    #[test]
    fn selects_newer_verified_package_in_the_installed_channel() {
        let list = json!([
            release("v0.1.1-preview.2", true),
            release("v0.1.0-preview.2", true),
            release("v0.1.0", false)
        ]);
        let platform = Platform::Mac;
        assert!(
            select_release(&list, &Version::parse("0.1.0").unwrap(), platform)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            select_release(&list, &Version::parse("0.1.0-preview.1").unwrap(), platform)
                .unwrap()
                .unwrap()
                .tag,
            "v0.1.1-preview.2"
        );
        let mut wrong = release("v1.0.0", false);
        wrong["assets"][0]["browser_download_url"] = json!("https://example.com/update.zip");
        assert!(
            select_release(&json!([wrong]), &Version::parse("0.1.0").unwrap(), platform)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn validates_archive_paths_before_installing() {
        use zip::write::SimpleFileOptions;

        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("test.zip");
        let output = temp.path().join("output");
        fs::create_dir(&output).unwrap();
        let mut writer = zip::ZipWriter::new(File::create(&archive).unwrap());
        writer
            .start_file(
                "Resume One/resume-one-app.exe",
                SimpleFileOptions::default(),
            )
            .unwrap();
        writer.write_all(b"test").unwrap();
        writer
            .start_file("Resume One/../../escaped", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"outside").unwrap();
        writer.finish().unwrap();
        assert!(unpack(&archive, &output, Platform::Windows).is_err());
        assert!(!temp.path().join("escaped").exists());
    }

    #[test]
    #[ignore = "downloads the official update into a temporary installation"]
    fn downloads_and_installs_the_public_release_into_a_scratch_directory() {
        let platform = Platform::current().expect("native release platform");
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join(platform.archive_root());
        for relative in [platform.app(), platform.cli(), platform.marker()] {
            let path = root.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"v0.1.0-preview.1").unwrap();
        }
        let installation = Installation::open(&root, platform).unwrap();
        let release = installation.check().unwrap().expect("newer public release");
        let version = release.tag.clone();
        let prepared = release.prepare(installation).unwrap();
        let stage = prepared.directory.path().join(platform.archive_root());
        let backup = prepared.directory.path().join("previous");
        replace_installation(&root, &stage, &backup, || {
            if Command::new(root.join(platform.cli()))
                .arg("--version")
                .output()?
                .status
                .success()
            {
                Ok(())
            } else {
                Err(io::Error::other("Updated CLI did not start"))
            }
        })
        .unwrap();
        assert_eq!(
            Installation::open(&root, platform).unwrap().version(),
            version
        );
        assert_eq!(
            fs::read(backup.join(platform.marker())).unwrap(),
            b"v0.1.0-preview.1"
        );
    }

    #[test]
    fn package_replacement_retains_backup_and_restores_it_on_launch_failure() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("old");
        let stage = temp.path().join("staged");
        let backup = temp.path().join("previous");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&stage).unwrap();
        fs::write(root.join("previous.txt"), "old").unwrap();
        fs::write(stage.join("next.txt"), "new").unwrap();
        assert!(
            replace_installation(&root, &stage, &backup, || Err(io::Error::other(
                "launch failed"
            )))
            .is_err()
        );
        assert!(root.join("previous.txt").is_file());
        assert!(stage.join("next.txt").is_file());
        replace_installation(&root, &stage, &backup, || Ok(())).unwrap();
        assert!(root.join("next.txt").is_file());
        assert!(backup.join("previous.txt").is_file());
    }
}
