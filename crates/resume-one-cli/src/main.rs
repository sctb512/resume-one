use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use resume_one_core::Diagnostic;
use resume_one_core::content_edit::{TextEdit, apply_text_edit};
use resume_one_core::docx;
use resume_one_core::export::write_export;
use resume_one_core::load_theme;
use resume_one_core::pdf;
use resume_one_core::render::{Language, markdown, plain_text};
use resume_one_core::resume::{
    JSON_RESUME_SCHEMA, JSON_RESUME_SCHEMA_VERSION, load_resume, read_pointer,
};
use resume_one_core::semantic::project;
use resume_one_core::storage::patch_resume;
use resume_one_core::storage::replace_resume;
use resume_one_core::structure::{StructureEdit, apply_structure};
use resume_one_themes::{GithubClient, GithubSource, ThemeCache, audit_release_source};
use serde_json::{Value, json};
use tempfile::Builder;

const INITIAL_RESUME: &[u8] = b"{\n  \"basics\": {}\n}\n";

#[derive(Parser)]
#[command(name = "resume-one", version, about = "本地 JSON Resume 命令行工具")]
struct Cli {
    /// 输出一行 JSON，便于脚本和 Agent 解析
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 创建最小有效简历，不覆盖已有文件
    Init {
        #[arg(long)]
        file: PathBuf,
    },
    /// 列出目录中的 JSON 候选文件（不递归、不读取内容）
    List {
        #[arg(long)]
        dir: PathBuf,
    },
    /// 返回 revision 和模块概况，不返回正文
    Inspect {
        #[arg(long)]
        file: PathBuf,
    },
    /// 按内置 JSON Resume Schema 校验
    Validate {
        #[arg(long)]
        file: PathBuf,
    },
    /// 显式读取完整简历或一个 JSON Pointer
    Read {
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        pointer: Option<String>,
    },
    /// 使用 revision 守卫应用 RFC 6902 补丁
    Patch {
        #[arg(long)]
        file: PathBuf,
        /// 补丁文件的绝对路径，或 - 表示标准输入
        #[arg(long)]
        patch: String,
        #[arg(long = "if-match")]
        if_match: String,
        #[arg(long)]
        dry_run: bool,
    },
    /// 按模块和条目安全修改结构，不读取或输出简历正文
    Structure {
        #[arg(long)]
        file: PathBuf,
        #[arg(long = "if-match")]
        if_match: String,
        #[arg(long)]
        dry_run: bool,
        #[command(subcommand)]
        operation: StructureCommand,
    },
    /// 安全设置或清除标准文本字段；空字符串表示移除字段
    SetField {
        #[arg(long)]
        file: PathBuf,
        #[arg(long = "if-match")]
        if_match: String,
        #[arg(long)]
        section: String,
        /// 非 basics 模块的零基条目序号
        #[arg(long)]
        index: Option<usize>,
        #[arg(long)]
        field: String,
        #[arg(long)]
        value: String,
        #[arg(long)]
        dry_run: bool,
    },
    /// 导出 PDF、DOCX、UTF-8 纯文本或 Markdown
    Export {
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, value_enum)]
        format: ExportFormat,
        #[arg(long, value_enum, default_value = "zh")]
        lang: ExportLanguage,
        /// PDF / DOCX 主题路径，原版主题仅适用于 PDF
        #[arg(long)]
        theme: Option<PathBuf>,
        /// 独立外观设置的绝对路径；只影响本次 PDF / DOCX，不改写主题或简历
        #[arg(long, requires = "theme")]
        appearance: Option<PathBuf>,
        #[arg(long)]
        overwrite: bool,
    },
    /// 输出当前内置 Schema 的身份和内容
    Schema,
    /// 查询可用字体或显式安装到简里的本机字体库；不读取简历
    Fonts {
        #[command(subcommand)]
        operation: FontCommand,
    },
    /// 离线检查待发布默认主题源的完整性与人工验收声明；不证明人工判断真实有效
    VerifyThemeSource {
        /// 已检出的源仓库根目录，包含 themes/index.json 和 themes/review.json
        #[arg(long)]
        dir: PathBuf,
    },
    /// 读取 GitHub 主题源或下载主题；不读取简历
    Themes {
        /// 省略时使用简里的原版 JSON Resume 主题源
        #[arg(long)]
        repository: Option<String>,
        #[arg(long = "ref", default_value = "main")]
        reference: String,
        #[arg(long = "index")]
        index_path: Option<String>,
        /// 仅存主题数据；与简历和本地安装目录分离
        #[arg(long)]
        cache: PathBuf,
        #[arg(long)]
        offline: bool,
        #[command(subcommand)]
        operation: ThemeCommand,
    },
}

#[derive(Subcommand)]
enum ThemeCommand {
    /// 列出清单元数据（不表示逐项 manifest 已通过校验）
    List,
    /// 校验主题后显式下载到所选本地目录，不覆盖同名文件
    Download {
        id: String,
        #[arg(long)]
        dir: PathBuf,
    },
}

#[derive(Subcommand)]
enum FontCommand {
    List,
    Install {
        #[arg(long)]
        file: PathBuf,
        /// 省略时安装到桌面应用默认字体库；显式目录不会自动加入字体搜索路径
        #[arg(long)]
        dir: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ExportFormat {
    Txt,
    Md,
    Docx,
    Pdf,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ExportLanguage {
    Zh,
    En,
}

#[derive(Subcommand)]
enum StructureCommand {
    /// 在指定模块前插入空模块；未指定位置时追加到末尾
    AddModule {
        id: String,
        #[arg(long)]
        before: Option<String>,
    },
    /// 移除整个模块及其条目
    RemoveModule { id: String },
    /// 在零基位置插入空条目
    AddItem {
        id: String,
        #[arg(long)]
        index: usize,
    },
    /// 移除零基位置的一个条目
    RemoveItem {
        id: String,
        #[arg(long)]
        index: usize,
    },
}

#[derive(Debug)]
struct AppError {
    exit: u8,
    code: &'static str,
    message: String,
    file: Option<String>,
    revision: Option<String>,
    diagnostics: Vec<Value>,
}

impl AppError {
    fn new(exit: u8, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            exit,
            code,
            message: message.into(),
            file: None,
            revision: None,
            diagnostics: Vec::new(),
        }
    }

    fn file(mut self, path: &Path) -> Self {
        self.file = Some(path.to_string_lossy().into_owned());
        self
    }

    fn diagnostics(mut self, items: &[Diagnostic]) -> Self {
        self.diagnostics = items
            .iter()
            .map(|item| json!({"code": item.code, "path": item.path, "message": item.message}))
            .collect();
        self
    }

    fn envelope(&self) -> Value {
        let mut result = json!({
            "protocolVersion": 1,
            "ok": false,
            "error": {"code": self.code, "message": self.message}
        });
        if let Some(file) = &self.file {
            result["file"] = json!(file);
        }
        if let Some(revision) = &self.revision {
            result["revision"] = json!(revision);
        }
        if !self.diagnostics.is_empty() {
            result["error"]["diagnostics"] = json!(self.diagnostics);
        }
        result
    }
}

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().collect();
    if args
        .get(1)
        .is_some_and(|arg| arg == resume_one_updater::APPLY_ARG)
    {
        return match resume_one_updater::apply_from_helper(args.into_iter().skip(2)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        };
    }
    let json_requested = args.iter().any(|arg| arg == "--json");
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => {
            if error.kind() == clap::error::ErrorKind::DisplayHelp
                || error.kind() == clap::error::ErrorKind::DisplayVersion
            {
                let _ = error.print();
                return ExitCode::SUCCESS;
            }
            return emit(
                json_requested,
                Err(AppError::new(2, "ARGUMENT_ERROR", error.to_string())),
            );
        }
    };
    emit(cli.json, execute(cli.command))
}

fn emit(json_mode: bool, result: Result<Value, AppError>) -> ExitCode {
    match result {
        Ok(value) => {
            let envelope = json!({"protocolVersion": 1, "ok": true, "result": value});
            if json_mode {
                println!("{envelope}");
            } else {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&value).expect("JSON value")
                );
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            if json_mode {
                println!("{}", error.envelope());
            } else {
                eprintln!("{}: {}", error.code, error.message);
                for diagnostic in &error.diagnostics {
                    eprintln!("  {}: {}", diagnostic["path"], diagnostic["message"]);
                }
            }
            ExitCode::from(error.exit)
        }
    }
}

fn absolute(path: &Path) -> Result<(), AppError> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(AppError::new(2, "PATH_NOT_ABSOLUTE", "路径必须是绝对路径").file(path))
    }
}

fn load(path: &Path) -> Result<resume_one_core::resume::ResumeDocument, AppError> {
    absolute(path)?;
    load_resume(path).map_err(|issues| from_diagnostics(path, &issues))
}

fn load_export_theme(path: &Path) -> Result<resume_one_core::ThemeManifest, AppError> {
    absolute(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| AppError::new(2, "THEME_PATH", "主题路径缺少目录"))?;
    let root = if path.file_name().is_some_and(|name| name == "theme.json") {
        parent
            .parent()
            .ok_or_else(|| AppError::new(2, "THEME_PATH", "主题路径缺少源目录"))?
    } else {
        parent
    };
    let relative = path.strip_prefix(root).expect("root is an ancestor");
    load_theme(root, relative).map_err(|issues| {
        AppError::new(3, "THEME_INVALID", "主题文件校验失败")
            .file(path)
            .diagnostics(&issues)
    })
}

fn from_diagnostics(path: &Path, issues: &[Diagnostic]) -> AppError {
    let io = issues
        .first()
        .is_some_and(|issue| issue.code == "RESUME_IO");
    AppError::new(
        if io { 5 } else { 3 },
        if io { "RESUME_IO" } else { "RESUME_INVALID" },
        issues.first().map_or("简历无效", |issue| &issue.message),
    )
    .file(path)
    .diagnostics(issues)
}

fn execute(command: Command) -> Result<Value, AppError> {
    match command {
        Command::Fonts { operation } => match operation {
            FontCommand::List => {
                let families = resume_one_core::fonts::available_families()
                    .map_err(|e| AppError::new(5, "FONT_LIST", e))?;
                Ok(
                    json!({"families":families,"library":resume_one_core::fonts::library_dir().ok()}),
                )
            }
            FontCommand::Install { file, dir } => {
                absolute(&file)?;
                let scope = if dir.is_some() {
                    "local-directory"
                } else {
                    "application-local"
                };
                let dir = dir
                    .map_or_else(resume_one_core::fonts::library_dir, Ok)
                    .map_err(|e| AppError::new(5, "FONT_LIBRARY", e))?;
                absolute(&dir)?;
                let families = resume_one_core::fonts::install(&file, &dir)
                    .map_err(|e| AppError::new(5, "FONT_INSTALL", e).file(&file))?;
                Ok(json!({"families":families,"directory":dir,"scope":scope}))
            }
        },
        Command::VerifyThemeSource { dir } => {
            absolute(&dir)?;
            let audit = audit_release_source(&dir)
                .map_err(|error| AppError::new(7, error.code, error.message).file(&dir))?;
            Ok(audit.as_json())
        }
        Command::Themes {
            repository,
            reference,
            index_path,
            cache,
            offline,
            operation,
        } => {
            absolute(&cache)?;
            if let ThemeCommand::Download { dir, .. } = &operation {
                absolute(dir)?;
                if !dir.is_dir() {
                    return Err(AppError::new(
                        5,
                        "THEME_DIRECTORY",
                        "本地主题目录不存在或不是目录",
                    )
                    .file(dir));
                }
            }
            let issue =
                |error: resume_one_themes::ThemeError| AppError::new(7, error.code, error.message);
            let default = GithubSource::defaults()[1].clone();
            let index = index_path.as_deref().unwrap_or_else(|| {
                if repository.is_some() {
                    "themes/index.json"
                } else {
                    default.index_path()
                }
            });
            let repository = repository
                .as_deref()
                .unwrap_or_else(|| default.repository());
            let source = GithubSource::new(repository, &reference, index).map_err(issue)?;
            let client = GithubClient::new(ThemeCache::new(&cache).map_err(issue)?);
            let result = client.catalog(&source, offline).map_err(issue)?;
            let warning = result
                .warning
                .as_ref()
                .map(|error| json!({"code":error.code,"message":error.message}));
            let catalog = result.catalog;
            match operation {
                ThemeCommand::List => {
                    Ok(json!({"source":source.as_json(),"commit":catalog.commit(),
                    "stale":result.stale,"warning":warning,"manifestsValidated":false,
                    "themes":catalog.entries().iter().map(|entry| json!({"id":entry.id,"name":entry.name,
                        "manifest":entry.manifest,"sha256":entry.sha256})).collect::<Vec<_>>()}))
                }
                ThemeCommand::Download { id, dir } => {
                    let theme = client.theme(&catalog, &id, offline).map_err(issue)?;
                    let path = theme.install(&source, &dir).map_err(issue)?;
                    Ok(
                        json!({"source":source.as_json(),"commit":catalog.commit(),"stale":result.stale,
                        "warning":warning,"id":theme.id(),"sha256":theme.sha256(),"file":path,"verified":true}),
                    )
                }
            }
        }
        Command::Init { file } => {
            absolute(&file)?;
            let parent = file
                .parent()
                .ok_or_else(|| AppError::new(2, "ARGUMENT_ERROR", "文件缺少父目录"))?;
            let mut temp = Builder::new()
                .prefix(".resume-one-init-")
                .tempfile_in(parent)
                .map_err(|err| AppError::new(5, "RESUME_IO", err.to_string()).file(&file))?;
            temp.write_all(INITIAL_RESUME)
                .and_then(|()| temp.as_file().sync_all())
                .map_err(|err| AppError::new(5, "RESUME_IO", err.to_string()).file(&file))?;
            temp.persist_noclobber(&file).map_err(|err| {
                AppError::new(
                    5,
                    if err.error.kind() == io::ErrorKind::AlreadyExists {
                        "FILE_EXISTS"
                    } else {
                        "RESUME_IO"
                    },
                    err.error.to_string(),
                )
                .file(&file)
            })?;
            let document = load(&file)?;
            Ok(json!({"file": file, "revision": document.revision, "valid": true}))
        }
        Command::List { dir } => {
            absolute(&dir)?;
            let mut files = Vec::new();
            for entry in fs::read_dir(&dir)
                .map_err(|err| AppError::new(5, "RESUME_IO", err.to_string()).file(&dir))?
            {
                let entry = entry
                    .map_err(|err| AppError::new(5, "RESUME_IO", err.to_string()).file(&dir))?;
                let path = entry.path();
                if path
                    .extension()
                    .is_some_and(|extension| extension == "json")
                    && entry.file_type().is_ok_and(|kind| kind.is_file())
                {
                    files.push(json!({
                        "name": entry.file_name().to_string_lossy(),
                        "path": path
                    }));
                }
            }
            files.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
            Ok(json!({"dir": dir, "files": files}))
        }
        Command::Inspect { file } => {
            let document = load(&file)?;
            let sections: serde_json::Map<String, Value> = document
                .sections()
                .into_iter()
                .map(|section| (section.id, json!(section.count)))
                .collect();
            Ok(
                json!({"file": file, "revision": document.revision, "valid": true, "sections": sections}),
            )
        }
        Command::Validate { file } => {
            let document = load(&file)?;
            Ok(
                json!({"file": file, "revision": document.revision, "valid": true, "schemaVersion": JSON_RESUME_SCHEMA_VERSION}),
            )
        }
        Command::Read { file, pointer } => {
            let document = load(&file)?;
            let value = match pointer {
                None => &document.value,
                Some(ref pointer) => {
                    if !valid_pointer(pointer) {
                        return Err(AppError::new(2, "INVALID_POINTER", "JSON Pointer 语法无效")
                            .file(&file));
                    }
                    read_pointer(&document, pointer).ok_or_else(|| {
                        AppError::new(3, "POINTER_NOT_FOUND", "字段不存在").file(&file)
                    })?
                }
            };
            Ok(json!({"file": file, "revision": document.revision, "value": value}))
        }
        Command::Patch {
            file,
            patch,
            if_match,
            dry_run,
        } => {
            absolute(&file)?;
            let mut bytes = Vec::new();
            if patch == "-" {
                io::stdin()
                    .take(1_048_577)
                    .read_to_end(&mut bytes)
                    .map_err(|err| AppError::new(5, "PATCH_IO", err.to_string()))?;
            } else {
                let patch_path = Path::new(&patch);
                absolute(patch_path)?;
                File::open(patch_path)
                    .and_then(|file| file.take(1_048_577).read_to_end(&mut bytes))
                    .map_err(|err| {
                        AppError::new(5, "PATCH_IO", err.to_string()).file(patch_path)
                    })?;
            }
            let outcome = patch_resume(&file, &bytes, &if_match, dry_run).map_err(|err| {
                let exit = match err.code {
                    "REVISION_CONFLICT" => 4,
                    "RESUME_IO" | "RESUME_BUSY" => 5,
                    "INVALID_REVISION" => 2,
                    _ => 3,
                };
                let mut output = AppError::new(exit, err.code, err.message).file(&file);
                output.revision = err.current_revision;
                output
            })?;
            Ok(json!({
                "file": file,
                "previousRevision": outcome.previous_revision,
                "revision": outcome.revision,
                "changedPaths": outcome.changed_paths,
                "valid": true,
                "dryRun": outcome.dry_run
            }))
        }
        Command::Structure {
            file,
            if_match,
            dry_run,
            operation,
        } => {
            if !if_match.starts_with("sha256:")
                || if_match.len() != 71
                || !if_match[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(AppError::new(
                    2,
                    "INVALID_REVISION",
                    "if-match 必须是 sha256: 后接 64 位十六进制摘要",
                )
                .file(&file));
            }
            let document = load(&file)?;
            if document.revision != if_match {
                let mut error = AppError::new(
                    4,
                    "REVISION_CONFLICT",
                    "文件已被其他进程或同步服务修改；未覆盖外部内容",
                )
                .file(&file);
                error.revision = Some(document.revision);
                return Err(error);
            }
            let edit = match operation {
                StructureCommand::AddModule { id, before } => {
                    StructureEdit::InsertSection { id, before }
                }
                StructureCommand::RemoveModule { id } => StructureEdit::RemoveSection { id },
                StructureCommand::AddItem { id, index } => StructureEdit::InsertItem { id, index },
                StructureCommand::RemoveItem { id, index } => {
                    StructureEdit::RemoveItem { id, index }
                }
            };
            let changed_path = match &edit {
                StructureEdit::InsertSection { id, .. } | StructureEdit::RemoveSection { id } => {
                    format!("/{id}")
                }
                StructureEdit::InsertItem { id, index }
                | StructureEdit::RemoveItem { id, index } => format!("/{id}/{index}"),
            };
            let next = apply_structure(&document, &edit)
                .map_err(|error| AppError::new(3, error.code, error.message).file(&file))?;
            if !dry_run {
                replace_resume(&file, &next.source, &if_match).map_err(|error| {
                    let exit = match error.code {
                        "REVISION_CONFLICT" => 4,
                        "RESUME_IO" | "RESUME_BUSY" => 5,
                        "INVALID_REVISION" => 2,
                        _ => 3,
                    };
                    let mut output = AppError::new(exit, error.code, error.message).file(&file);
                    output.revision = error.current_revision;
                    output
                })?;
            }
            Ok(json!({
                "file": file,
                "previousRevision": document.revision,
                "revision": next.revision,
                "changedPaths": [changed_path],
                "valid": true,
                "dryRun": dry_run
            }))
        }
        Command::SetField {
            file,
            if_match,
            section,
            index,
            field,
            value,
            dry_run,
        } => {
            if !if_match.starts_with("sha256:")
                || if_match.len() != 71
                || !if_match[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(AppError::new(
                    2,
                    "INVALID_REVISION",
                    "if-match 必须是 sha256: 后接 64 位十六进制摘要",
                )
                .file(&file));
            }
            let document = load(&file)?;
            if document.revision != if_match {
                let mut error = AppError::new(
                    4,
                    "REVISION_CONFLICT",
                    "文件已被其他进程或同步服务修改；未覆盖外部内容",
                )
                .file(&file);
                error.revision = Some(document.revision);
                return Err(error);
            }
            let edit = TextEdit {
                section,
                index,
                field,
                value,
            };
            let next = apply_text_edit(&document, &edit)
                .map_err(|error| AppError::new(3, error.code, error.message).file(&file))?;
            if !dry_run && next.revision != document.revision {
                replace_resume(&file, &next.source, &if_match).map_err(|error| {
                    let exit = match error.code {
                        "REVISION_CONFLICT" => 4,
                        "RESUME_IO" | "RESUME_BUSY" => 5,
                        "INVALID_REVISION" => 2,
                        _ => 3,
                    };
                    let mut output = AppError::new(exit, error.code, error.message).file(&file);
                    output.revision = error.current_revision;
                    output
                })?;
            }
            Ok(json!({
                "file": file,
                "previousRevision": document.revision,
                "revision": next.revision,
                "changedPaths": if next.revision == document.revision { Vec::new() } else { vec![edit.pointer()] },
                "valid": true,
                "dryRun": dry_run
            }))
        }
        Command::Export {
            file,
            out,
            format,
            lang,
            theme,
            appearance,
            overwrite,
        } => {
            absolute(&out)?;
            if theme.is_some() && !matches!(format, ExportFormat::Pdf | ExportFormat::Docx) {
                return Err(AppError::new(
                    2,
                    "THEME_FORMAT_UNSUPPORTED",
                    "--theme 仅适用于 PDF / DOCX 导出",
                ));
            }
            let language = match lang {
                ExportLanguage::Zh => Language::Zh,
                ExportLanguage::En => Language::En,
            };
            if let Some(path) = theme.as_deref().filter(|path| {
                path.file_name().is_some_and(|name| {
                    name.to_string_lossy()
                        .ends_with(resume_one_themes::ORIGINAL_THEME_SUFFIX)
                })
            }) {
                absolute(path)?;
                if !matches!(format, ExportFormat::Pdf) || appearance.is_some() {
                    return Err(AppError::new(
                        2,
                        "ORIGINAL_THEME_EXPORT",
                        "原版主题仅支持 PDF，使用主题自身的外观",
                    )
                    .file(path));
                }
                let document = load(&file)?;
                let theme = resume_one_themes::OriginalTheme::load(path)
                    .map_err(|error| AppError::new(3, error.code, error.message).file(path))?;
                let bytes =
                    resume_one_themes::print_localized_original(&theme, &document.value, language)
                        .map_err(|error| AppError::new(6, error.code, error.message).file(&out))?;
                write_export(&file, &out, &bytes, overwrite)
                    .map_err(|error| AppError::new(6, error.code, error.message).file(&out))?;
                return Ok(
                    json!({"file":file,"revision":document.revision,"out":out,"format":"pdf","bytes":bytes.len()}),
                );
            }
            let document = load(&file)?;
            let content = project(&document);
            let mut effective_theme = theme.as_deref().map(load_export_theme).transpose()?;
            if let Some(path) = appearance {
                absolute(&path)?;
                let mut bytes = Vec::new();
                File::open(&path)
                    .and_then(|file| file.take(65_537).read_to_end(&mut bytes))
                    .map_err(|error| {
                        AppError::new(5, "APPEARANCE_IO", error.to_string()).file(&path)
                    })?;
                if bytes.len() > 65_536 {
                    return Err(
                        AppError::new(3, "APPEARANCE_INVALID", "外观设置超过 64 KiB").file(&path),
                    );
                }
                let value = serde_json::from_slice(&bytes).map_err(|error| {
                    AppError::new(3, "APPEARANCE_INVALID", error.to_string()).file(&path)
                })?;
                let prefs = resume_one_core::appearance::Appearance::from_json(&value)
                    .map_err(|error| AppError::new(3, "APPEARANCE_INVALID", error).file(&path))?;
                effective_theme = effective_theme.as_ref().map(|theme| prefs.apply(theme));
            }
            let (format_name, rendered) = match format {
                ExportFormat::Txt => ("txt", plain_text(&content, language).into_bytes()),
                ExportFormat::Md => ("md", markdown(&content, language).into_bytes()),
                ExportFormat::Docx => (
                    "docx",
                    match effective_theme.as_ref() {
                        Some(theme) => docx::render_with_theme(&content, language, theme),
                        None => docx::render(&content, language),
                    }
                    .map_err(|err| AppError::new(6, "EXPORT_RENDER", err).file(&out))?,
                ),
                ExportFormat::Pdf => (
                    "pdf",
                    match effective_theme.as_ref() {
                        Some(theme) => pdf::render_with_theme(&content, language, theme),
                        None => pdf::render(&content, language),
                    }
                    .map_err(|err| AppError::new(6, "EXPORT_RENDER", err).file(&out))?,
                ),
            };
            write_export(&file, &out, &rendered, overwrite)
                .map_err(|err| AppError::new(6, err.code, err.message).file(&out))?;
            Ok(
                json!({"file": file, "revision": document.revision, "out": out, "format": format_name, "bytes": rendered.len()}),
            )
        }
        Command::Schema => {
            let schema: Value =
                serde_json::from_str(JSON_RESUME_SCHEMA).expect("bundled schema JSON");
            Ok(json!({"schemaVersion": JSON_RESUME_SCHEMA_VERSION, "schema": schema}))
        }
    }
}

fn valid_pointer(pointer: &str) -> bool {
    if !pointer.is_empty() && !pointer.starts_with('/') {
        return false;
    }
    let mut bytes = pointer.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'~' && !matches!(bytes.next(), Some(b'0' | b'1')) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_escape_validation() {
        assert!(valid_pointer(""));
        assert!(valid_pointer("/a~1b/~0"));
        assert!(!valid_pointer("a/b"));
        assert!(!valid_pointer("/a~2b"));
        assert!(!valid_pointer("/a~"));
    }

    #[test]
    fn initial_document_is_valid() {
        assert!(resume_one_core::resume::parse_resume(INITIAL_RESUME).is_ok());
    }
}
