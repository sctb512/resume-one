use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use resume_one_core::Diagnostic;
use resume_one_core::export::write_export;
use resume_one_core::render::{Language, markdown, plain_text};
use resume_one_core::resume::{
    JSON_RESUME_SCHEMA, JSON_RESUME_SCHEMA_VERSION, load_resume, read_pointer,
};
use resume_one_core::semantic::project;
use resume_one_core::storage::patch_resume;
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
    /// 导出 UTF-8 纯文本或 Markdown
    Export {
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, value_enum)]
        format: TextFormat,
        #[arg(long, value_enum, default_value = "zh")]
        lang: ExportLanguage,
        #[arg(long)]
        overwrite: bool,
    },
    /// 输出当前内置 Schema 的身份和内容
    Schema,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum TextFormat {
    Txt,
    Md,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ExportLanguage {
    Zh,
    En,
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
                    "RESUME_IO" => 5,
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
        Command::Export {
            file,
            out,
            format,
            lang,
            overwrite,
        } => {
            absolute(&out)?;
            let document = load(&file)?;
            let content = project(&document);
            let language = match lang {
                ExportLanguage::Zh => Language::Zh,
                ExportLanguage::En => Language::En,
            };
            let (format_name, rendered) = match format {
                TextFormat::Txt => ("txt", plain_text(&content, language)),
                TextFormat::Md => ("md", markdown(&content, language)),
            };
            write_export(&file, &out, rendered.as_bytes(), overwrite)
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
