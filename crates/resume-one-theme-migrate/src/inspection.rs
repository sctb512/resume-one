//! Source-complete package inventory and syntax evidence, not a visual certificate.

use std::collections::BTreeSet;
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::{AppError, source_bundle, syntax};

const STYLE_EXTENSIONS: &[&str] = &[
    "js",
    "jsx",
    "mjs",
    "cjs",
    "ts",
    "tsx",
    "css",
    "scss",
    "sass",
    "less",
    "html",
    "htm",
    "hbs",
    "handlebars",
    "ejs",
    "njk",
    "vue",
    "svelte",
];

fn blocker(code: &str, file: Option<&str>, message: &str) -> Value {
    json!({"code":code,"file":file,"message":message})
}

pub(crate) fn inspect(root: &Path) -> Result<Value, AppError> {
    let snapshot =
        source_bundle::snapshot(root).map_err(|e| AppError::new(3, e.code, e.message))?;
    let started = Instant::now();
    let mut files = Vec::new();
    let mut blockers = Vec::new();
    let mut properties = BTreeSet::new();
    let mut external = BTreeSet::new();
    let mut license_files = Vec::new();
    let mut styles = 0usize;
    let mut declarations = 0usize;
    let mut expressions = 0usize;
    let mut package = Value::Null;
    let mut evidence_bytes = 0usize;
    for file in &snapshot.files {
        if started.elapsed() > Duration::from_secs(30) {
            return Err(AppError::new(
                3,
                "INSPECTION_LIMIT",
                "包的语法审计超过 30 秒，未标成完整",
            ));
        }
        let path = &file.relative_path;
        let basename = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
        if basename.starts_with("license")
            || basename.starts_with("notice")
            || basename.starts_with("ofl")
            || basename == "copying"
        {
            license_files.push(path.clone());
        }
        if path == "package.json" {
            package = serde_json::from_slice(&file.bytes)
                .map_err(|e| AppError::new(3, "PACKAGE_JSON", e.to_string()))?;
            if !package.is_object() {
                return Err(AppError::new(3, "PACKAGE_JSON", "package.json 必须是对象"));
            }
        }
        let extension = path
            .rsplit('.')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        let analysis =
            syntax::analyze(path, &file.bytes).map_err(|e| AppError::new(3, e.code, e.message))?;
        let Some(analysis) = analysis else {
            if STYLE_EXTENSIONS.contains(&extension.as_str()) {
                blockers.push(blocker(
                    "FORMAT_ADAPTER_REQUIRED",
                    Some(path),
                    "此源码格式尚无语法迁移适配器，不能按未使用内容跳过",
                ));
            }
            continue;
        };
        evidence_bytes = evidence_bytes
            .saturating_add(serde_json::to_vec(&analysis).map_or(usize::MAX, |b| b.len()));
        if evidence_bytes > 16 * 1024 * 1024 {
            return Err(AppError::new(
                3,
                "INSPECTION_LIMIT",
                "累计语法证据超过 16 MiB，未继续生成不完整报告",
            ));
        }
        if analysis["format"] == "css" {
            collect_css(
                &analysis["styles"],
                path,
                &mut blockers,
                &mut properties,
                &mut declarations,
            );
        } else {
            if analysis["syntaxValid"] != true {
                blockers.push(blocker(
                    "JAVASCRIPT_SYNTAX",
                    Some(path),
                    "当前解析器未能完整解析源码，或 JSX 标签不匹配；须解决格式/语法兼容性缺口才能完整迁移",
                ));
            }
            for import in analysis["imports"].as_array().into_iter().flatten() {
                if let Some(module) = import["module"].as_str() {
                    if module.starts_with('.') {
                        if !relative_module_exists(path, module, &snapshot.files) {
                            blockers.push(json!({"code":"LOCAL_DEPENDENCY_UNRESOLVED","file":path,"module":module,"span":import["span"],"message":"本地依赖未包含在来源快照中，未联网或执行解析"}));
                        }
                    } else {
                        external.insert(module.to_owned());
                    }
                } else {
                    blockers.push(json!({"code":"IMPORT_EXPRESSION_UNRESOLVED","file":path,"span":import["span"],"message":"动态或转义导入路径尚未静态解析，未执行表达式"}));
                }
            }
            for style in analysis["styledComponents"]
                .as_array()
                .into_iter()
                .flatten()
            {
                styles += 1;
                if !style["dynamicExpressions"]
                    .as_array()
                    .is_some_and(|v| v.is_empty())
                {
                    blockers.push(json!({"code":"DYNAMIC_STYLE_UNRESOLVED","file":path,"component":style["name"],"span":style["span"],"message":"样式含模板表达式；必须由受限适配器转换，不能执行或丢弃"}));
                }
                if style["templateEscapesRequireDecoding"] == true {
                    blockers.push(json!({"code":"TEMPLATE_ESCAPE_UNRESOLVED","file":path,"component":style["name"],"span":style["templateSpan"],"message":"模板转义须在静态字面层准确解码，不能直接当成 CSS"}));
                }
                if style["topLevelBinding"] != true {
                    blockers.push(json!({"code":"STYLE_SCOPE_UNRESOLVED","file":path,"span":style["span"],"message":"非顶层样式绑定尚未完成作用域及继承解析"}));
                }
                if !style["css"].is_null() {
                    collect_css(
                        &style["css"],
                        path,
                        &mut blockers,
                        &mut properties,
                        &mut declarations,
                    );
                }
            }
            expressions += analysis["jsxExpressions"].as_array().map_or(0, Vec::len);
            if analysis["unclassifiedTemplates"]
                .as_array()
                .is_some_and(|v| !v.is_empty())
            {
                blockers.push(blocker(
                    "TEMPLATE_ADAPTER_REQUIRED",
                    Some(path),
                    "存在未分类的模板字符串，不能假定其中没有布局、样式或内容逻辑",
                ));
            }
        }
        files.push(analysis);
    }
    if license_files.is_empty() {
        blockers.push(blocker(
            "LICENSE_EVIDENCE_MISSING",
            None,
            "包中未找到许可正文；package.json 的 license 声明不等于许可核验",
        ));
    }
    if !external.is_empty() {
        blockers.push(json!({"code":"EXTERNAL_DEPENDENCIES_UNRESOLVED","modules":external,"message":"外部共享组件、格式化工具和字体依赖须固定来源并纳入后续解析；本次不会安装、下载或执行"}));
    }
    // Structural syntax extraction cannot justify any fidelity percentage.
    // These blockers intentionally survive a perfectly parsed static package.
    blockers.push(blocker(
        "DECLARATIVE_COMPILER_REQUIRED",
        None,
        "语法证据已保留；完整的结构、继承、字段和共享排版构件编译仍须实现并验证",
    ));
    blockers.push(blocker(
        "VISUAL_ACCEPTANCE_REQUIRED",
        None,
        "尚未对固定字体、纸张及公开中英文/缺字段/多页样例完成原版逐项视觉验收",
    ));
    if blockers.len() > 4096 {
        return Err(AppError::new(
            3,
            "INSPECTION_LIMIT",
            "迁移阻断项超过 4096 个，未输出不完整验收报告",
        ));
    }
    let result = json!({"inspectionVersion":1,"source":root,"sourceSnapshot":snapshot.report(),
        "parsers":{"javascript":"tree-sitter-javascript@0.25.0","javascriptPatch":"resume-one-jsx-ampersand-v1","css":"tree-sitter-css@0.25.0","engine":"tree-sitter@0.25.10"},
        "package":{"name":package["name"],"version":package["version"],"licenseDeclaration":package["license"]},
        "statistics":{"sourceFiles":snapshot.files.len(),"syntaxFiles":files.len(),"styledComponents":styles,"cssDeclarations":declarations,"jsxExpressions":expressions},
        "cssProperties":properties,"externalImports":external,"licenseFiles":license_files,"syntaxEvidence":files,
        "migrationComplete":false,"visualVerified":false,"licenseVerified":false,"executedSourceCode":false,"sourceModified":false,"blockers":blockers});
    if serde_json::to_vec(&result).map_or(true, |b| b.len() > 16 * 1024 * 1024) {
        return Err(AppError::new(
            3,
            "INSPECTION_LIMIT",
            "结构化迁移报告超过 16 MiB，未截断或标成完整",
        ));
    }
    Ok(result)
}

fn collect_css(
    css: &Value,
    path: &str,
    blockers: &mut Vec<Value>,
    properties: &mut BTreeSet<String>,
    count: &mut usize,
) {
    if css["syntaxValid"] != true {
        blockers.push(blocker(
            "CSS_SYNTAX",
            Some(path),
            "CSS 语法树包含错误，未静默跳过失败规则",
        ));
    }
    for declaration in css["declarations"].as_array().into_iter().flatten() {
        *count += 1;
        if let Some(property) = declaration["property"].as_str() {
            properties.insert(property.to_owned());
        }
    }
    for directive in css["directives"].as_array().into_iter().flatten() {
        if directive["kind"] == "import_statement" {
            blockers.push(json!({"code":"CSS_DEPENDENCY_UNRESOLVED","file":path,"span":directive["span"],"message":"CSS @import 须固定资源及依赖；没有发起网络请求"}));
        }
    }
}

fn relative_module_exists(
    current: &str,
    module: &str,
    files: &[source_bundle::SourceFile],
) -> bool {
    if module.contains(['\\', '?', '#']) {
        return false;
    }
    let mut parts = current.split('/').collect::<Vec<_>>();
    parts.pop();
    for part in module.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return false;
                }
            }
            _ => parts.push(part),
        }
    }
    let base = parts.join("/");
    // A lexical, bounded lookup, not Node's conditional/package resolver.
    [
        "",
        ".js",
        ".jsx",
        ".mjs",
        ".cjs",
        ".css",
        ".json",
        "/index.js",
        "/index.jsx",
    ]
    .iter()
    .any(|suffix| {
        files
            .iter()
            .any(|f| f.relative_path == format!("{base}{suffix}"))
    })
}
