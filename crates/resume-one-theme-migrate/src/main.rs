//! Separate, data-only theme migrator. Theme source code is never executed.
//! Only explicit --fixture input enables bounded unstyled content evaluation;
//! package examples otherwise remain inert assets in the package digest.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use resume_one_core::export::write_export;
use resume_one_core::{Diagnostic, parse_theme};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

mod adapt_fonts;
mod adapt_style;
mod adaptation;
mod constant_expr;
mod content_eval;
mod dependency_bundle;
mod embedded_css;
mod external_styles;
mod inspection;
mod module_symbols;
mod primitive_contacts;
mod primitive_dates;
mod section_adapter;
mod source_bundle;
mod static_source;
mod style_link;
mod symbol_link;
mod syntax;
mod template_ir;
mod utility_style;

const MAX_SOURCE_BYTES: u64 = 1_048_576;

#[derive(Parser)]
#[command(
    name = "resume-one-theme-migrate",
    version,
    about = "静态检查完整主题源码，或生成明确标为待审核的声明式主题候选"
)]
struct Cli {
    /// v1 声明式主题文件、第三方主题目录或静态源码文件的绝对路径
    #[arg(long)]
    source: PathBuf,
    /// 新 v2 主题文件的绝对路径；不得覆盖来源
    #[arg(long, required_unless_present = "inspect", conflicts_with = "inspect")]
    out: Option<PathBuf>,
    /// 完整读取有界源码包并输出语法/依赖/阻断报告，不生成近似主题
    #[arg(long,conflicts_with_all=["dry_run","overwrite","license","upstream_url","upstream_revision"])]
    inspect: bool,
    /// 显式选择源码包内的 JSX 入口，生成受限的纯数据模板 IR；可重复，最多 8 个
    #[arg(long, requires = "inspect")]
    template_entry: Vec<String>,
    /// 显式读取共享依赖包的绝对目录并锁定来源；可重复，最多 8 个，不安装依赖
    #[arg(long, requires = "inspect")]
    dependency: Vec<PathBuf>,
    /// 显式读取 JSON Resume 样例，求值模板内容；输出会含样例文本，不生成主题
    #[arg(long, requires_all = ["inspect", "template_entry"])]
    fixture: Option<PathBuf>,
    /// 严格要求完整迁移；不完整时失败且不写入候选，供 Agent/批处理使用
    #[arg(long)]
    require_complete: bool,
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
    /// 显式生成尽量还原的 v2 适配版；不声称与原版像素一致
    #[arg(long, conflicts_with_all = ["inspect", "require_complete"])]
    adapt: bool,
    /// 将 --source 作为批量来源清单，--out 作为新目录；逐项迁移并报告失败
    #[arg(long, requires = "adapt", conflicts_with_all = ["dry_run", "overwrite"])]
    batch: bool,
    /// 批量并行数（1 至 4），报告保持清单顺序
    #[arg(long,default_value_t=1,value_parser=clap::value_parser!(u8).range(1..=4),requires="batch")]
    jobs: u8,
    /// 显式读取公开样本进行布局验收，可重复；报告只记录指纹和计数
    #[arg(long, requires = "adapt")]
    verify_fixture: Vec<PathBuf>,
    /// 验证 PDF 与 DOCX 序列化（不证明 Microsoft Word 或原版视觉等价）
    #[arg(long, requires = "verify_fixture")]
    verify_exports: bool,
    /// 显式本地静态字体与许可资料库，不联网或安装字体
    #[arg(long, requires = "adapt")]
    font_library: Option<PathBuf>,
}

#[derive(Debug)]
struct AppError {
    exit: u8,
    code: &'static str,
    message: String,
    diagnostics: Vec<Value>,
    inspection: Option<Box<Value>>,
}

impl AppError {
    fn new(exit: u8, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            exit,
            code,
            message: message.into(),
            diagnostics: Vec::new(),
            inspection: None,
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
            inspection: None,
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
        if let Some(inspection) = &self.inspection {
            output["error"]["inspection"] = (**inspection).clone();
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
    if cli.adapt {
        emit(if cli.batch {
            adaptation::batch(&cli)
        } else {
            adaptation::single(&cli)
        })
    } else if cli.inspect {
        let result =
            inspection::inspect_dependencies(&cli.source, &cli.template_entry, &cli.dependency)
                .and_then(|mut report| {
                    if let Some(path) = &cli.fixture {
                        evaluate_fixture(&mut report, path)?;
                    }
                    if cli.require_complete && report["migrationComplete"] != true {
                        let mut error = AppError::new(
                            3,
                            "MIGRATION_INCOMPLETE",
                            "存在未迁移能力或未完成的视觉验收；没有生成主题",
                        );
                        error.diagnostics =
                            report["blockers"].as_array().cloned().unwrap_or_default();
                        error.inspection = Some(Box::new(report));
                        Err(error)
                    } else {
                        Ok(report)
                    }
                });
        emit(result)
    } else {
        emit(migrate(cli))
    }
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

fn evaluate_fixture(report: &mut Value, path: &Path) -> Result<(), AppError> {
    absolute(path)?;
    if path.components().any(|c| {
        matches!(
            c,
            std::path::Component::ParentDir | std::path::Component::CurDir
        )
    }) {
        return Err(AppError::new(2, "FIXTURE_PATH", "样例路径不能包含 . 或 .."));
    }
    for ancestor in path.ancestors() {
        let metadata = fs::symlink_metadata(ancestor)
            .map_err(|e| AppError::new(3, "FIXTURE_IO", e.to_string()))?;
        if metadata.file_type().is_symlink() {
            return Err(AppError::new(
                3,
                "FIXTURE_SYMLINK",
                "样例及其父路径不能是符号链接",
            ));
        }
    }
    let before = fs::metadata(path).map_err(|e| AppError::new(3, "FIXTURE_IO", e.to_string()))?;
    if !before.is_file() || before.len() > MAX_SOURCE_BYTES {
        return Err(AppError::new(
            3,
            "FIXTURE_LIMIT",
            "样例必须为不超过 1 MiB 的普通文件",
        ));
    }
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|f| f.take(MAX_SOURCE_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|e| AppError::new(3, "FIXTURE_IO", e.to_string()))?;
    let after = fs::metadata(path).map_err(|e| AppError::new(3, "FIXTURE_IO", e.to_string()))?;
    if bytes.len() > MAX_SOURCE_BYTES as usize
        || before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
    {
        return Err(AppError::new(
            3,
            "FIXTURE_CHANGED",
            "读取期间样例发生变化或超限",
        ));
    }
    let document = resume_one_core::resume::parse_resume(&bytes).map_err(|issues| {
        let mut error = AppError::new(3, "FIXTURE_INVALID", "样例未通过 JSON Resume 校验");
        error.diagnostics = issues
            .iter()
            .map(|i| json!({"code":i.code,"path":i.path,"message":i.message}))
            .collect();
        error
    })?;
    let mut evaluation = content_eval::evaluate(report, &document.value)
        .map_err(|e| AppError::new(3, "CONTENT_EVALUATION_ERROR", e))?;
    evaluation["fixture"] =
        json!({"path":path,"sha256":sha256(&bytes),"bytes":bytes.len(),"sourceModified":false});
    let blockers = report["blockers"]
        .as_array_mut()
        .expect("inspection blockers");
    for entry in evaluation["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| e["contentEvaluated"] == false)
    {
        if blockers.len() >= 4096 {
            return Err(AppError::new(
                3,
                "INSPECTION_LIMIT",
                "内容求值阻断项超过 4096 个",
            ));
        }
        blockers.push(json!({"code":entry["error"]["code"],"file":entry["entryFile"],"stage":"content-evaluation","message":"样例内容求值未完成；没有用空内容替代失败节点"}));
    }
    report["contentEvaluation"] = evaluation;
    if serde_json::to_vec(report).map_or(true, |v| v.len() > 16 * 1024 * 1024) {
        return Err(AppError::new(
            3,
            "INSPECTION_LIMIT",
            "内容求值后的报告超过 16 MiB",
        ));
    }
    Ok(())
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
    let out = cli
        .out
        .as_ref()
        .ok_or_else(|| AppError::new(2, "ARGUMENT_ERROR", "生成候选需要 --out"))?;
    absolute(out)?;
    if !out
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
    if cli.require_complete {
        let mut error = AppError::new(
            3,
            "MIGRATION_INCOMPLETE",
            "当前候选转换不构成完整视觉迁移；未写入目标。请先用 --inspect 查看阻断项",
        );
        if cli.source.is_dir() {
            let report = inspection::inspect(&cli.source)?;
            error.diagnostics = report["blockers"].as_array().cloned().unwrap_or_default();
            error.inspection = Some(Box::new(report));
        }
        return Err(error);
    }
    let source_path = fs::canonicalize(&cli.source)
        .map_err(|error| AppError::new(5, "SOURCE_IO", error.to_string()))?;
    if out.exists() {
        let output_path = fs::canonicalize(out)
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
        write_export(&source_path, out, &serialized, cli.overwrite)
            .map_err(|error| AppError::new(5, error.code, error.message))?;
    }
    Ok(json!({
        "source": cli.source,
        "out": out,
        "sourceKind": source.source_kind,
        "sourceVersion": source.source_version,
        "targetVersion": 2,
        "sourceSha256": source.source_sha256,
        "candidateSha256": sha256(&serialized),
        "written": !cli.dry_run,
        "licenseDeclared": candidate["license"] != "NOASSERTION",
        "licenseVerified": false,
        "visualVerified": false,
        "migrationComplete": false,
        "requiresReview": true,
        "reviewChecklist": ["人工核验来源许可与再分发条件", "使用中英文、短长简历和多页样例比对视觉效果", "第三方主题需人工重建未映射的布局、字体与模块构件"],
        "extraction": source.extraction,
        "candidate": candidate
    }))
}
