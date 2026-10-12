//! Separate, data-only theme migrator. This binary never opens a resume or executes theme code.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use resume_one_core::export::write_export;
use resume_one_core::{Diagnostic, parse_theme};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

mod static_source;

const MAX_SOURCE_BYTES: u64 = 1_048_576;

#[derive(Parser)]
#[command(
    name = "resume-one-theme-migrate",
    version,
    about = "以纯静态方式生成待审核的 Resume One v2 主题候选文件"
)]
struct Cli {
    /// v1 声明式主题文件、第三方主题目录或静态源码文件的绝对路径
    #[arg(long)]
    source: PathBuf,
    /// 新 v2 主题文件的绝对路径；不得覆盖来源
    #[arg(long)]
    out: PathBuf,
    /// 来源许可声明；迁移器不会自行核验许可
    #[arg(long)]
    license: Option<String>,
    /// 来源的 HTTPS 地址（仅用于出处记录，不会发起请求）
    #[arg(long)]
    upstream_url: Option<String>,
    /// 来源版本、提交哈希或标签
    #[arg(long)]
    upstream_revision: Option<String>,
    /// 显式允许替换已有目标文件，永不替换来源文件
    #[arg(long)]
    overwrite: bool,
    /// 只校验并输出候选与审核报告，不写文件
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug)]
struct AppError {
    exit: u8,
    code: &'static str,
    message: String,
    diagnostics: Vec<Value>,
}

impl AppError {
    fn new(exit: u8, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            exit,
            code,
            message: message.into(),
            diagnostics: Vec::new(),
        }
    }

    fn from_theme(issues: Vec<Diagnostic>) -> Self {
        Self {
            exit: 3,
            code: "THEME_INVALID",
            message: "主题文件未通过声明式主题规范校验".into(),
            diagnostics: issues
                .into_iter()
                .map(|issue| {
                    json!({"code": issue.code, "path": issue.path, "message": issue.message})
                })
                .collect(),
        }
    }

    fn json(&self) -> Value {
        let mut output = json!({
            "protocolVersion": 1,
            "ok": false,
            "error": {"code": self.code, "message": self.message}
        });
        if !self.diagnostics.is_empty() {
            output["error"]["diagnostics"] = json!(self.diagnostics);
        }
        output
    }
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) {
                let _ = error.print();
                return ExitCode::SUCCESS;
            }
            return emit(Err(AppError::new(2, "ARGUMENT_ERROR", error.to_string())));
        }
    };
    emit(migrate(cli))
}

fn emit(result: Result<Value, AppError>) -> ExitCode {
    match result {
        Ok(value) => {
            println!(
                "{}",
                json!({"protocolVersion": 1, "ok": true, "result": value})
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            println!("{}", error.json());
            ExitCode::from(error.exit)
        }
    }
}

fn absolute(path: &Path) -> Result<(), AppError> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(AppError::new(
            2,
            "PATH_NOT_ABSOLUTE",
            "来源和目标必须使用绝对路径",
        ))
    }
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

struct SourceCandidate {
    manifest: Value,
    source_sha256: String,
    source_version: Option<u64>,
    source_kind: &'static str,
    fidelity: &'static str,
    extraction: Option<Value>,
    suggested_license: Option<String>,
}

fn source_candidate(path: &Path) -> Result<SourceCandidate, AppError> {
    if path.is_dir() {
        let extracted = static_source::extract(path)
            .map_err(|error| AppError::new(3, error.code, error.message))?;
        return Ok(SourceCandidate {
            manifest: extracted.manifest,
            source_sha256: extracted.source_sha256,
            source_version: None,
            source_kind: "third-party-static",
            fidelity: "inspired",
            extraction: Some(extracted.report),
            suggested_license: extracted.suggested_license,
        });
    }
    if !path.is_file() {
        return Err(AppError::new(2, "SOURCE_TYPE", "来源必须是主题文件或目录"));
    }
    let theme_file = path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".theme.json") || name == "theme.json");
    if !theme_file {
        let extracted = static_source::extract(path)
            .map_err(|error| AppError::new(3, error.code, error.message))?;
        return Ok(SourceCandidate {
            manifest: extracted.manifest,
            source_sha256: extracted.source_sha256,
            source_version: None,
            source_kind: "third-party-static",
            fidelity: "inspired",
            extraction: Some(extracted.report),
            suggested_license: extracted.suggested_license,
        });
    }
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(MAX_SOURCE_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|error| AppError::new(5, "SOURCE_IO", error.to_string()))?;
    let theme = parse_theme(&bytes).map_err(AppError::from_theme)?;
    if theme.source_version != 1 {
        return Err(AppError::new(
            3,
            "ALREADY_V2",
            "来源已经是 v2 主题，无需迁移",
        ));
    }
    Ok(SourceCandidate {
        manifest: theme.normalized,
        source_sha256: sha256(&bytes),
        source_version: Some(1),
        source_kind: "resume-one-v1",
        fidelity: "adapted",
        extraction: None,
        suggested_license: None,
    })
}

fn migrate(cli: Cli) -> Result<Value, AppError> {
    absolute(&cli.source)?;
    absolute(&cli.out)?;
    if !cli
        .out
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".theme.json") || name == "theme.json")
    {
        return Err(AppError::new(
            2,
            "OUTPUT_NAME",
            "目标文件名必须以 .theme.json 结尾",
        ));
    }
    let source_path = fs::canonicalize(&cli.source)
        .map_err(|error| AppError::new(5, "SOURCE_IO", error.to_string()))?;
    if cli.out.exists() {
        let output_path = fs::canonicalize(&cli.out)
            .map_err(|error| AppError::new(5, "OUTPUT_IO", error.to_string()))?;
        if source_path == output_path {
            return Err(AppError::new(2, "SAME_FILE", "目标不能是来源文件"));
        }
    }
    let source = source_candidate(&source_path)?;
    let mut candidate = source.manifest;
    if let Some(license) = &cli.license {
        candidate["license"] = json!(license);
    }
    let mut provenance = Map::new();
    provenance.insert("migratedBy".into(), json!("resume-one-theme-migrate"));
    provenance.insert("fidelity".into(), json!(source.fidelity));
    if let Some(license) = &source.suggested_license {
        provenance.insert("upstreamLicense".into(), json!(license));
    }
    if let Some(url) = &cli.upstream_url {
        provenance.insert("upstreamUrl".into(), json!(url));
    }
    if let Some(revision) = &cli.upstream_revision {
        provenance.insert("upstreamRevision".into(), json!(revision));
    }
    candidate["provenance"] = Value::Object(provenance);
    parse_theme(candidate.to_string().as_bytes()).map_err(AppError::from_theme)?;
    let mut serialized = serde_json::to_vec_pretty(&candidate)
        .map_err(|error| AppError::new(3, "SERIALIZE", error.to_string()))?;
    serialized.push(b'\n');
    if !cli.dry_run {
        write_export(&source_path, &cli.out, &serialized, cli.overwrite)
            .map_err(|error| AppError::new(5, error.code, error.message))?;
    }
    Ok(json!({
        "source": cli.source,
        "out": cli.out,
        "sourceKind": source.source_kind,
        "sourceVersion": source.source_version,
        "targetVersion": 2,
        "sourceSha256": source.source_sha256,
        "candidateSha256": sha256(&serialized),
        "written": !cli.dry_run,
        "licenseDeclared": candidate["license"] != "NOASSERTION",
        "licenseVerified": false,
        "visualVerified": false,
        "requiresReview": true,
        "reviewChecklist": ["人工核验来源许可与再分发条件", "使用中英文、短长简历和多页样例比对视觉效果", "第三方主题需人工重建未映射的布局、字体与模块构件"],
        "extraction": source.extraction,
        "candidate": candidate
    }))
}
