//! Bounded, non-executing extraction of candidate tokens from third-party theme packages.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const MAX_FILE_BYTES: u64 = 524_288;
const MAX_TOTAL_BYTES: usize = 4_194_304;
const MAX_STYLE_FILES: usize = 64;
const MODULES: [&str; 12] = [
    "basics",
    "work",
    "volunteer",
    "education",
    "awards",
    "certificates",
    "publications",
    "skills",
    "languages",
    "interests",
    "references",
    "projects",
];

#[derive(Debug)]
pub struct StaticError {
    pub code: &'static str,
    pub message: String,
}

fn issue(code: &'static str, message: impl Into<String>) -> StaticError {
    StaticError {
        code,
        message: message.into(),
    }
}

pub struct StaticCandidate {
    pub manifest: Value,
    pub source_sha256: String,
    pub suggested_license: Option<String>,
    pub report: Value,
}

fn read_limited(path: &Path, max: u64) -> Result<Vec<u8>, StaticError> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(max + 1).read_to_end(&mut bytes))
        .map_err(|error| issue("SOURCE_IO", error.to_string()))?;
    if bytes.len() as u64 > max {
        return Err(issue(
            "SOURCE_TOO_LARGE",
            format!("文件超过 {max} 字节静态读取上限"),
        ));
    }
    Ok(bytes)
}

fn is_style_file(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some(
            "css"
                | "scss"
                | "less"
                | "html"
                | "htm"
                | "hbs"
                | "handlebars"
                | "ejs"
                | "njk"
                | "vue"
                | "js"
                | "jsx"
                | "ts"
                | "tsx"
        )
    )
}

fn collect_files(
    root: &Path,
    folder: &Path,
    depth: usize,
    entries_seen: &mut usize,
    files: &mut Vec<PathBuf>,
    skipped: &mut Vec<String>,
) -> Result<(), StaticError> {
    let mut entries = fs::read_dir(folder)
        .map_err(|error| issue("SOURCE_IO", error.to_string()))?
        .take(1025)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| issue("SOURCE_IO", error.to_string()))?;
    if entries.len() > 1024 {
        return Err(issue(
            "SCAN_LIMIT",
            "单个目录条目超过 1024 个；请缩小来源目录",
        ));
    }
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        *entries_seen += 1;
        if *entries_seen > 4096 {
            return Err(issue("SCAN_LIMIT", "来源目录条目总数超过 4096 个"));
        }
        let file_type = entry
            .file_type()
            .map_err(|error| issue("SOURCE_IO", error.to_string()))?;
        let relative = entry
            .path()
            .strip_prefix(root)
            .expect("inside scan root")
            .to_path_buf();
        let relative_text = relative.to_string_lossy().into_owned();
        if file_type.is_symlink() {
            skipped.push(format!("跳过符号链接：{relative_text}"));
            continue;
        }
        if file_type.is_dir() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if depth < 2
                && !name.starts_with('.')
                && !matches!(
                    name.as_ref(),
                    "node_modules" | "target" | "vendor" | "test" | "tests"
                )
            {
                collect_files(root, &entry.path(), depth + 1, entries_seen, files, skipped)?;
            }
        } else if file_type.is_file() && is_style_file(&entry.path()) {
            files.push(entry.path());
            if files.len() > MAX_STYLE_FILES {
                return Err(issue(
                    "SCAN_LIMIT",
                    "候选样式文件超过 64 个；请缩小来源目录",
                ));
            }
        }
    }
    Ok(())
}

fn hex_digit(byte: u8) -> bool {
    byte.is_ascii_hexdigit()
}

fn color_terminator(byte: u8) -> bool {
    !byte.is_ascii_alphanumeric() && byte != b'_'
}

fn colors_from_text(bytes: &[u8], counts: &mut HashMap<String, usize>) {
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'#' {
            index += 1;
            continue;
        }
        let rest = &bytes[index + 1..];
        let long = rest.len() >= 6
            && rest[..6].iter().all(|byte| hex_digit(*byte))
            && rest.get(6).is_none_or(|byte| color_terminator(*byte));
        let short = rest.len() >= 3
            && rest[..3].iter().all(|byte| hex_digit(*byte))
            && rest.get(3).is_none_or(|byte| color_terminator(*byte));
        let color = if long {
            Some(format!(
                "#{}",
                String::from_utf8_lossy(&rest[..6]).to_ascii_uppercase()
            ))
        } else if short {
            let digits = &rest[..3];
            Some(format!(
                "#{0}{0}{1}{1}{2}{2}",
                (digits[0] as char).to_ascii_uppercase(),
                (digits[1] as char).to_ascii_uppercase(),
                (digits[2] as char).to_ascii_uppercase()
            ))
        } else {
            None
        };
        if let Some(color) = color {
            *counts.entry(color).or_default() += 1;
        }
        index += 1;
    }
}

fn style_hints_from_text(
    bytes: &[u8],
    fonts: &mut HashMap<String, usize>,
    signals: &mut std::collections::BTreeSet<&'static str>,
) {
    let lower = bytes.to_ascii_lowercase();
    for (needle, signal) in [
        (b"grid-template-columns".as_slice(), "grid-columns"),
        (b"column-count".as_slice(), "multi-column"),
        (b"@page".as_slice(), "print-page"),
        (b"@font-face".as_slice(), "font-face"),
    ] {
        if lower.windows(needle.len()).any(|part| part == needle) {
            signals.insert(signal);
        }
    }
    for (needle, signal) in [
        (b"display:grid".as_slice(), "grid"),
        (b"display: grid".as_slice(), "grid"),
        (b"display:flex".as_slice(), "flex"),
        (b"display: flex".as_slice(), "flex"),
        (b"position:absolute".as_slice(), "absolute-position"),
        (b"position: absolute".as_slice(), "absolute-position"),
    ] {
        if lower.windows(needle.len()).any(|part| part == needle) {
            signals.insert(signal);
        }
    }

    const PROPERTY: &[u8] = b"font-family";
    let mut index = 0;
    while index + PROPERTY.len() < lower.len() {
        if lower[index..].starts_with(PROPERTY)
            && (index == 0 || !lower[index - 1].is_ascii_alphanumeric())
        {
            let mut value_start = index + PROPERTY.len();
            while lower.get(value_start).is_some_and(u8::is_ascii_whitespace) {
                value_start += 1;
            }
            if lower.get(value_start) == Some(&b':') {
                value_start += 1;
                let mut value_end = value_start;
                while value_end < bytes.len()
                    && value_end - value_start <= 160
                    && !matches!(bytes[value_end], b';' | b'}' | b'\n' | b'\r')
                {
                    value_end += 1;
                }
                if value_end - value_start <= 160 {
                    let value = String::from_utf8_lossy(&bytes[value_start..value_end]);
                    let value = value.trim();
                    if !value.is_empty() && !value.contains('\u{fffd}') {
                        *fonts.entry(value.to_owned()).or_default() += 1;
                    }
                }
                index = value_end;
                continue;
            }
        }
        index += 1;
    }
}

fn accent_score(hex: &str, count: usize) -> i32 {
    let channel = |start| u8::from_str_radix(&hex[start..start + 2], 16).unwrap() as i32;
    let (r, g, b) = (channel(1), channel(3), channel(5));
    let chroma = r.max(g).max(b) - r.min(g).min(b);
    let average = (r + g + b) / 3;
    if !(28..=220).contains(&average) || chroma < 20 {
        return -1;
    }
    count as i32 * 100 + chroma - (average - 110).abs() / 2
}

fn slug(value: &str) -> String {
    let basename = value.rsplit('/').next().unwrap_or(value);
    let basename = basename
        .strip_prefix("jsonresume-theme-")
        .unwrap_or(basename);
    let mut result = String::new();
    for character in basename.chars() {
        if character.is_ascii_alphanumeric() {
            result.push(character.to_ascii_lowercase());
        } else if !result.is_empty() && !result.ends_with('-') {
            result.push('-');
        }
    }
    let result = result.trim_matches('-');
    if result.is_empty() {
        "migrated-theme".into()
    } else {
        result
            .chars()
            .take(64)
            .collect::<String>()
            .trim_end_matches('-')
            .to_owned()
    }
}

fn valid_version(value: &str) -> bool {
    value.split('.').count() == 3
        && value.split('.').all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (part == "0" || !part.starts_with('0'))
        })
}

fn fingerprint_file(fingerprint: &mut Sha256, name: &str, bytes: &[u8]) {
    fingerprint.update((name.len() as u64).to_le_bytes());
    fingerprint.update(name.as_bytes());
    fingerprint.update((bytes.len() as u64).to_le_bytes());
    fingerprint.update(bytes);
}

pub fn extract(path: &Path) -> Result<StaticCandidate, StaticError> {
    let root = if path.is_dir() {
        path
    } else {
        path.parent()
            .ok_or_else(|| issue("SOURCE_IO", "来源缺少父目录"))?
    };
    let package_path = root.join("package.json");
    let package = if path.is_dir() && package_path.is_file() && !package_path.is_symlink() {
        let bytes = read_limited(&package_path, MAX_FILE_BYTES)?;
        Some((
            serde_json::from_slice::<Value>(&bytes)
                .map_err(|error| issue("PACKAGE_JSON", error.to_string()))?,
            bytes,
        ))
    } else {
        None
    };
    let package_value = package
        .as_ref()
        .map(|(value, _)| value)
        .unwrap_or(&Value::Null);
    let mut files = Vec::new();
    let mut skipped = Vec::new();
    if path.is_dir() {
        let mut entries_seen = 0;
        collect_files(root, root, 0, &mut entries_seen, &mut files, &mut skipped)?;
    } else if is_style_file(path) {
        files.push(path.to_path_buf());
    } else {
        return Err(issue(
            "SOURCE_TYPE",
            "静态迁移只支持主题目录或 CSS/HTML/JS/TS 样式源码",
        ));
    }
    if files.is_empty() {
        return Err(issue("NO_STYLE_FILES", "来源中没有可读取的静态样式源码"));
    }
    files.sort();
    let mut total_bytes = package.as_ref().map_or(0, |(_, bytes)| bytes.len());
    let mut fingerprint = Sha256::new();
    if let Some((_, bytes)) = &package {
        fingerprint_file(&mut fingerprint, "package.json", bytes);
    }
    let mut colors = HashMap::new();
    let mut fonts = HashMap::new();
    let mut layout_signals = std::collections::BTreeSet::new();
    let mut scanned = Vec::new();
    let mut executable_source_present = false;
    for file in files {
        let relative = file.strip_prefix(root).expect("inside scan root");
        let name = relative.to_string_lossy().into_owned();
        let resolved =
            fs::canonicalize(&file).map_err(|error| issue("SOURCE_IO", error.to_string()))?;
        if !resolved.starts_with(root) {
            skipped.push(format!("跳过越界文件：{name}"));
            continue;
        }
        let metadata =
            fs::metadata(&file).map_err(|error| issue("SOURCE_IO", error.to_string()))?;
        if metadata.len() > MAX_FILE_BYTES {
            skipped.push(format!("跳过超限文件：{name}"));
            continue;
        }
        let bytes = read_limited(&file, MAX_FILE_BYTES)?;
        total_bytes += bytes.len();
        if total_bytes > MAX_TOTAL_BYTES {
            return Err(issue("SCAN_LIMIT", "来源静态读取总量超过 4 MiB"));
        }
        fingerprint_file(&mut fingerprint, &name, &bytes);
        colors_from_text(&bytes, &mut colors);
        style_hints_from_text(&bytes, &mut fonts, &mut layout_signals);
        executable_source_present |= matches!(
            file.extension().and_then(|ext| ext.to_str()),
            Some("js" | "jsx" | "ts" | "tsx")
        );
        scanned.push(name);
    }
    if scanned.is_empty() {
        return Err(issue("NO_STYLE_FILES", "所有候选文件都超过静态读取上限"));
    }
    let mut ranked = colors.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut ranked_fonts = fonts.into_iter().collect::<Vec<_>>();
    ranked_fonts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let accent = ranked
        .iter()
        .max_by_key(|(hex, count)| accent_score(hex, *count))
        .filter(|(hex, count)| accent_score(hex, *count) >= 0)
        .map_or_else(|| "#52756B".to_owned(), |(hex, _)| hex.clone());
    let package_name = package_value.get("name").and_then(Value::as_str);
    let source_name = package_name
        .or_else(|| path.file_stem().and_then(|name| name.to_str()))
        .unwrap_or("migrated-theme");
    let id = slug(source_name);
    let basename = source_name.rsplit('/').next().unwrap_or(source_name);
    let title = basename
        .strip_prefix("jsonresume-theme-")
        .unwrap_or(basename);
    let title = if title.is_empty() {
        "Migrated Theme"
    } else {
        title
    };
    let title = title.chars().take(80).collect::<String>();
    let version = package_value
        .get("version")
        .and_then(Value::as_str)
        .filter(|value| valid_version(value))
        .unwrap_or("0.1.0");
    let suggested_license = package_value
        .get("license")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let description =
        format!("从第三方主题 {title} 静态提取的候选；布局与视觉效果仍需人工迁移和验收。");
    let manifest = json!({
        "formatVersion": 2,
        "id": id,
        "name": title,
        "description": description,
        "version": version,
        "license": suggested_license.as_deref().unwrap_or("NOASSERTION"),
        "layout": "classic",
        "colors": {"accent": accent, "seal": "#A14F40", "paper": "#FFFFFF", "canvas": "#F5F3EE"},
        "modules": MODULES
    });
    let report = json!({
        "confidence": "low",
        "sourceKind": "third-party-static",
        "filesScanned": scanned,
        "filesSkipped": skipped,
        "colorsFound": ranked.into_iter().take(20).map(|(hex, count)| json!({"hex": hex, "occurrences": count})).collect::<Vec<_>>(),
        "fontFamiliesFound": ranked_fonts.into_iter().take(20).map(|(value, count)| json!({"declaration": value, "occurrences": count})).collect::<Vec<_>>(),
        "layoutSignals": layout_signals,
        "selectedAccent": accent,
        "executableSourcePresent": executable_source_present,
        "executedSourceCode": false,
        "unmapped": ["HTML/CSS/JS 布局与条件逻辑", "主题字体与图标", "分页和模块构件", "原主题视觉保真度"],
        "note": "字体声明和布局线索只来自静态文本匹配，可能包含注释或脚本字符串，不会自动写进主题配置；候选不是原主题的等效移植，禁止未经人工许可与视觉验收直接发布。"
    });
    Ok(StaticCandidate {
        manifest,
        source_sha256: format!("{:x}", fingerprint.finalize()),
        suggested_license,
        report,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_extraction_is_bounded_and_normalized() {
        let mut counts = HashMap::new();
        colors_from_text(
            b"color:#abc; background:#123456; bad:#1234567; color:#ABC; {{#each items}}",
            &mut counts,
        );
        assert_eq!(counts["#AABBCC"], 2);
        assert_eq!(counts["#123456"], 1);
        assert_eq!(counts.len(), 2);
    }

    #[test]
    fn slug_and_version_are_schema_compatible() {
        assert_eq!(slug("@example/jsonresume-theme-Qing-He"), "qing-he");
        assert_eq!(slug("中文"), "migrated-theme");
        assert!(valid_version("1.2.3"));
        assert!(!valid_version("1.2.3-beta"));
        assert!(!valid_version("01.2.3"));
    }

    #[test]
    fn static_style_hints_are_bounded_and_do_not_become_theme_rules() {
        let mut fonts = HashMap::new();
        let mut signals = std::collections::BTreeSet::new();
        style_hints_from_text(
            "@page { size: A4 } h1 { FONT-FAMILY : '思源宋体', serif; } .body { font-family: '思源宋体', serif; display: flex; column-count: 2; }".as_bytes(),
            &mut fonts,
            &mut signals,
        );
        assert_eq!(fonts["'思源宋体', serif"], 2);
        assert_eq!(
            signals.into_iter().collect::<Vec<_>>(),
            ["flex", "multi-column", "print-page"]
        );
        let mut too_long = HashMap::new();
        style_hints_from_text(
            format!("font-family: {};", "x".repeat(161)).as_bytes(),
            &mut too_long,
            &mut std::collections::BTreeSet::new(),
        );
        assert!(too_long.is_empty());
    }
}
