//! Bounded local font installation and native system-family resolution.
//! No font paths or font bytes are written into resumes or themes.
use font_kit::{
    family_name::FamilyName,
    handle::Handle,
    properties::{Properties, Style, Weight},
    source::SystemSource,
};
use sha2::{Digest, Sha256};
use std::{
    borrow::Cow,
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};
use ttf_parser::{Face, Permissions};

const MAX_SOURCE: u64 = 256 * 1024 * 1024;
const MAX_FACE: usize = 64 * 1024 * 1024;
const MAX_CACHED: usize = 256;
const MAX_CACHE_BYTES: usize = 512 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct LocalFont {
    pub family: String,
    pub bytes: Arc<Vec<u8>>,
    pub weight: u16,
    pub requested_weight: u16,
    /// Actual italic/oblique face, never a synthetic shear.
    pub italic: bool,
    pub subset_allowed: bool,
    pub pdf_embed: bool,
    pub editable_embed: bool,
}

#[derive(Default)]
struct Registry {
    entries: Vec<LocalFont>,
    keys: BTreeMap<(String, u16, bool), u16>,
    bytes: usize,
}
fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(Default::default)
}
pub fn library_dir() -> Result<PathBuf, String> {
    dirs::data_local_dir()
        .map(|root| root.join("Resume One/fonts"))
        .ok_or("无法定位本机字体库".into())
}
fn read_font(path: &Path) -> Result<Vec<u8>, String> {
    let mut file = fs::File::open(path).map_err(|e| format!("读取字体失败：{e}"))?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() > MAX_SOURCE {
        return Err("字体须为不超过 256 MiB 的普通文件".into());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_SOURCE + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_SOURCE {
        return Err("字体文件超出大小限制".into());
    }
    Ok(bytes)
}
fn face_family(face: &Face<'_>) -> Result<String, String> {
    for name_id in [
        ttf_parser::name_id::TYPOGRAPHIC_FAMILY,
        ttf_parser::name_id::FAMILY,
    ] {
        if let Some(name) = face
            .names()
            .into_iter()
            .filter(|n| n.name_id == name_id)
            .find_map(|n| n.to_string())
            && valid_family(&name)
        {
            return Ok(name);
        }
    }
    Err("字体没有有效的名称".into())
}
pub fn valid_family(family: &str) -> bool {
    !family.trim().is_empty()
        && family.len() <= 256
        && !family.chars().any(|c| {
            c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
}
fn face_count(data: &[u8]) -> Result<u32, String> {
    let count = ttf_parser::fonts_in_collection(data).unwrap_or(1);
    if count == 0 || count > 128 {
        return Err("字体集合中的字面数量超出限制".into());
    }
    Ok(count)
}
fn installed_files(root: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let kind = entry.file_type().ok()?;
            let path = entry.path();
            (kind.is_file()
                && matches!(
                    path.extension().and_then(|s| s.to_str()),
                    Some("ttf" | "otf" | "ttc" | "otc")
                ))
            .then_some(path)
        })
        .take(256)
        .collect();
    paths.sort();
    paths
}
pub fn available_families() -> Result<Vec<String>, String> {
    let mut names = SystemSource::new()
        .all_families()
        .map_err(|e| format!("无法读取系统字体：{e:?}"))?;
    if let Ok(root) = library_dir() {
        for path in installed_files(&root) {
            if let Ok(bytes) = read_font(&path) {
                for index in 0..face_count(&bytes).unwrap_or(0) {
                    if let Ok(face) = Face::parse(&bytes, index)
                        && let Ok(name) = face_family(&face)
                    {
                        names.push(name);
                    }
                }
            }
        }
    }
    names.retain(|name| valid_family(name));
    names.sort_by_cached_key(|name| name.to_lowercase());
    names.dedup();
    Ok(names)
}
/// Install into this application's private library, never into a system font directory.
pub fn install(path: &Path, root: &Path) -> Result<Vec<String>, String> {
    let bytes = read_font(path)?;
    let mut families = Vec::new();
    for index in 0..face_count(&bytes)? {
        let face = Face::parse(&bytes, index).map_err(|_| "无法解析字体文件")?;
        families.push(face_family(&face)?);
        extract_face(&bytes, index)?;
    }
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    if fs::symlink_metadata(root)
        .map_err(|e| e.to_string())?
        .file_type()
        .is_symlink()
    {
        return Err("字体库不能为符号链接".into());
    }
    let files = installed_files(root);
    let total = files
        .iter()
        .filter_map(|p| fs::metadata(p).ok())
        .map(|m| m.len())
        .sum::<u64>();
    let suffix = if bytes.starts_with(b"ttcf") {
        "ttc"
    } else if bytes.starts_with(b"OTTO") {
        "otf"
    } else {
        "ttf"
    };
    let destination = root.join(format!("{:x}.{suffix}", Sha256::digest(&bytes)));
    if destination.exists() {
        if fs::symlink_metadata(&destination)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_symlink()
            || read_font(&destination)? != bytes
        {
            return Err("字体目标已存在且内容不一致，未覆盖".into());
        }
    } else {
        if files.len() >= 128 || total.saturating_add(bytes.len() as u64) > 1024 * 1024 * 1024 {
            return Err("本机字体库已达到 128 个文件或 1 GiB 上限".into());
        }
        crate::export::write_export_unnamed(&destination, &bytes, false)
            .map_err(|error| error.message)?;
    }
    if let Ok(mut registry) = registry().lock() {
        registry
            .keys
            .retain(|(name, _, _), _| !families.contains(name));
    }
    families.sort();
    families.dedup();
    Ok(families)
}

/// Convert one TTC/OTC face into a standalone SFNT without native font execution.
pub fn extract_face(data: &[u8], index: u32) -> Result<Vec<u8>, String> {
    let face = Face::parse(data, index).map_err(|_| "无效的 OpenType 字体")?;
    let _ = face_family(&face)?;
    if !data.starts_with(b"ttcf") {
        if data.len() > MAX_FACE {
            return Err("单个字面超出 64 MiB 限制".into());
        }
        return Ok(data.to_vec());
    }
    fn u32_at(data: &[u8], at: usize) -> Result<u32, String> {
        Ok(u32::from_be_bytes(
            data.get(at..at + 4)
                .ok_or("字体目录越界")?
                .try_into()
                .unwrap(),
        ))
    }
    let base = u32_at(data, 12 + index as usize * 4)? as usize;
    let header = data.get(base..base + 12).ok_or("字体目录无效")?;
    let count = u16::from_be_bytes([header[4], header[5]]) as usize;
    if count == 0 || count > 4095 {
        return Err("字体表数量无效".into());
    }
    let directory = data
        .get(base + 12..base + 12 + count * 16)
        .ok_or("字体表越界")?;
    let mut out = header.to_vec();
    out.resize(12 + count * 16, 0);
    let mut head = None;
    for (i, table) in directory.as_chunks::<16>().0.iter().enumerate() {
        let start = u32::from_be_bytes(table[8..12].try_into().unwrap()) as usize;
        let len = u32::from_be_bytes(table[12..16].try_into().unwrap()) as usize;
        let bytes = data
            .get(start..start.checked_add(len).ok_or("字体表溢出")?)
            .ok_or("字体表越界")?;
        if out.len().saturating_add(len).saturating_add(3) > MAX_FACE {
            return Err("单个字面超出 64 MiB 限制".into());
        }
        let offset = out.len();
        out.extend_from_slice(bytes);
        while out.len() % 4 != 0 {
            out.push(0);
        }
        let entry = 12 + i * 16;
        out[entry..entry + 16].copy_from_slice(table);
        out[entry + 8..entry + 12].copy_from_slice(&(offset as u32).to_be_bytes());
        if &table[..4] == b"head" && len >= 12 {
            head = Some(offset);
        }
    }
    if let Some(head) = head {
        out[head + 8..head + 12].fill(0);
        let sum = out.as_chunks::<4>().0.iter().fold(0u32, |sum, chunk| {
            sum.wrapping_add(u32::from_be_bytes(*chunk))
        });
        out[head + 8..head + 12].copy_from_slice(&0xB1B0AFBAu32.wrapping_sub(sum).to_be_bytes());
    }
    Face::parse(&out, 0).map_err(|_| "集合字面提取失败")?;
    Ok(out)
}

pub fn resolve(family: &str, weight: u16) -> Result<u16, String> {
    resolve_style(family, weight, false)
}

/// Resolve a real requested slant; refusing a missing face avoids silently
/// synthesizing italic in preview while printing unrelated upright outlines.
pub fn resolve_style(family: &str, weight: u16, italic: bool) -> Result<u16, String> {
    if !valid_family(family) || !(100..=900).contains(&weight) {
        return Err("字体名称或字重无效".into());
    }
    let key = (family.to_owned(), weight, italic);
    if let Some(id) = registry()
        .lock()
        .map_err(|_| "字体缓存不可用")?
        .keys
        .get(&key)
    {
        return Ok(*id);
    }
    let mut found = None;
    if let Ok(root) = library_dir() {
        for path in installed_files(&root) {
            let Ok(bytes) = read_font(&path) else {
                continue;
            };
            for index in 0..face_count(&bytes).unwrap_or(0) {
                if let Ok(face) = Face::parse(&bytes, index)
                    && face_family(&face).is_ok_and(|name| name == family)
                {
                    let distance = (
                        (face.is_italic() || face.is_oblique()) != italic,
                        (i32::from(face.weight().to_number()) - i32::from(weight)).abs(),
                    );
                    if found.as_ref().is_none_or(|(best, _)| distance < *best) {
                        found = Some((distance, extract_face(&bytes, index)?));
                    }
                }
            }
        }
    }
    let bytes = if let Some((_, bytes)) = found {
        bytes
    } else {
        let handle = SystemSource::new()
            .select_best_match(
                &[FamilyName::Title(family.to_owned())],
                Properties::new()
                    .weight(Weight(weight as f32))
                    .style(if italic { Style::Italic } else { Style::Normal }),
            )
            .map_err(|_| format!("字体「{family}」当前不可用，请安装后再试"))?;
        match handle {
            Handle::Path { path, font_index } => extract_face(&read_font(&path)?, font_index)?,
            Handle::Memory { bytes, font_index } => extract_face(&bytes, font_index)?,
            handle @ Handle::Native { .. } => {
                let font = handle
                    .load()
                    .map_err(|e| format!("无法读取系统字体：{e:?}"))?;
                if let Some(bytes) = font.copy_font_data() {
                    extract_face(&bytes, 0)?
                } else {
                    native_font_data(&font)?
                }
            }
        }
    };
    let face = Face::parse(&bytes, 0).map_err(|_| "无法解析请求的字体")?;
    if (face.is_italic() || face.is_oblique()) != italic {
        return Err(format!(
            "字体「{family}」没有请求的{}字面，未使用合成倾斜",
            if italic { "斜体" } else { "正体" }
        ));
    }
    register(key, bytes)
}
/// Register a verified theme-local static face without adding it to the user's
/// font library or making an unrelated system family resolve to these bytes.
pub(crate) fn register_asset(bytes: Vec<u8>) -> Result<u16, String> {
    if bytes.len() > MAX_FACE
        || (!bytes.starts_with(b"\x00\x01\x00\x00") && !bytes.starts_with(b"OTTO"))
    {
        return Err("主题字体须为独立的静态 TTF/OTF 字面".into());
    }
    let face = Face::parse(&bytes, 0).map_err(|_| "主题字体无法解析")?;
    if face.is_variable() {
        return Err("主题字体须先转换为指定字重的静态字面，不能静默忽略可变轴".into());
    }
    let key = (
        format!("@theme:{:x}", Sha256::digest(&bytes)),
        face.weight().to_number(),
        face.is_italic() || face.is_oblique(),
    );
    register(key, bytes)
}

fn register(key: (String, u16, bool), bytes: Vec<u8>) -> Result<u16, String> {
    let face = Face::parse(&bytes, 0).map_err(|_| "字体无法解析")?;
    // GPUI requires this glyph when loading text families. Refuse explicitly
    // rather than letting the screen silently use an unrelated fallback face.
    if face.glyph_index('m').is_none() {
        return Err("该字面缺少文本引擎必需的基本拉丁字形 m，不能用作简历正文字体".into());
    }
    let permissions = face.permissions();
    let font = LocalFont {
        family: face_family(&face)?,
        weight: face.weight().to_number(),
        requested_weight: key.1,
        italic: face.is_italic() || face.is_oblique(),
        subset_allowed: face.is_subsetting_allowed(),
        pdf_embed: face.is_outline_embedding_allowed()
            && matches!(
                permissions,
                Some(
                    Permissions::Installable | Permissions::Editable | Permissions::PreviewAndPrint
                )
            ),
        editable_embed: face.is_outline_embedding_allowed()
            && matches!(
                permissions,
                Some(Permissions::Installable | Permissions::Editable)
            ),
        bytes: Arc::new(bytes),
    };
    let mut registry = registry().lock().map_err(|_| "字体缓存不可用")?;
    if let Some(id) = registry.keys.get(&key) {
        return Ok(*id);
    }
    let builtin = match font.family.as_str() {
        "Noto Sans SC" => Some(crate::layout::SANS_FONT_BYTES),
        "Noto Serif SC" => Some(crate::layout::SERIF_FONT_BYTES),
        _ => None,
    };
    if builtin.is_some_and(|data| data != font.bytes.as_slice())
        || registry.entries.iter().any(|previous| {
            previous.family == font.family
                && previous.weight == font.weight
                && previous.italic == font.italic
                && previous.bytes != font.bytes
        })
    {
        return Err(format!(
            "字体「{}」与已加载的同名字族/字重内容不同，无法保证预览一致；请为迁移字体使用独立字族名称",
            font.family
        ));
    }
    if registry.entries.len() >= MAX_CACHED
        || registry.bytes.saturating_add(font.bytes.len()) > MAX_CACHE_BYTES
    {
        return Err("已达到本次运行的字体缓存上限，请重新启动后再试".into());
    }
    let id = registry.entries.len() as u16;
    registry.bytes += font.bytes.len();
    registry.entries.push(font);
    registry.keys.insert(key, id);
    Ok(id)
}
pub fn get(id: u16) -> LocalFont {
    registry().lock().expect("font registry").entries[id as usize].clone()
}
pub fn bytes(id: u16) -> Cow<'static, [u8]> {
    Cow::Owned(get(id).bytes.as_ref().clone())
}

fn native_font_data(font: &font_kit::font::Font) -> Result<Vec<u8>, String> {
    #[cfg(target_os = "macos")]
    {
        let native = font.native_font();
        let tags = native
            .get_available_font_tables()
            .ok_or("系统未提供字体表")?;
        let mut tables: Vec<_> = tags
            .iter()
            .filter_map(|tag| {
                let tag = *tag;
                // A digital signature cannot survive a new SFNT container.
                (tag != u32::from_be_bytes(*b"DSIG"))
                    .then(|| font.load_font_table(tag).map(|data| (tag, data)))
                    .flatten()
            })
            .collect();
        tables.sort_by_key(|(tag, _)| *tag);
        if tables.is_empty() || tables.len() > 4095 {
            return Err("系统字体表数量无效".into());
        }
        let count = tables.len() as u16;
        let power = 1u16 << (15 - count.leading_zeros());
        let cff = tables.iter().any(|(tag, _)| {
            *tag == u32::from_be_bytes(*b"CFF ") || *tag == u32::from_be_bytes(*b"CFF2")
        });
        let mut out = if cff {
            b"OTTO".to_vec()
        } else {
            0x00010000u32.to_be_bytes().to_vec()
        };
        out.extend_from_slice(&count.to_be_bytes());
        out.extend_from_slice(&(power * 16).to_be_bytes());
        out.extend_from_slice(&(power.trailing_zeros() as u16).to_be_bytes());
        out.extend_from_slice(&(count * 16 - power * 16).to_be_bytes());
        out.resize(12 + tables.len() * 16, 0);
        let mut head = None;
        for (i, (tag, data)) in tables.into_iter().enumerate() {
            if out.len().saturating_add(data.len() + 3) > MAX_FACE {
                return Err("系统字面超出 64 MiB 限制".into());
            }
            let start = out.len();
            let len = data.len();
            out.extend_from_slice(&data);
            while out.len() % 4 != 0 {
                out.push(0);
            }
            if tag == u32::from_be_bytes(*b"head") && len >= 12 {
                head = Some(start);
                out[start + 8..start + 12].fill(0);
            }
            let sum = out[start..]
                .as_chunks::<4>()
                .0
                .iter()
                .fold(0u32, |sum, c| sum.wrapping_add(u32::from_be_bytes(*c)));
            let at = 12 + i * 16;
            out[at..at + 4].copy_from_slice(&tag.to_be_bytes());
            out[at + 4..at + 8].copy_from_slice(&sum.to_be_bytes());
            out[at + 8..at + 12].copy_from_slice(&(start as u32).to_be_bytes());
            out[at + 12..at + 16].copy_from_slice(&(len as u32).to_be_bytes());
        }
        if let Some(head) = head {
            let sum = out
                .as_chunks::<4>()
                .0
                .iter()
                .fold(0u32, |sum, c| sum.wrapping_add(u32::from_be_bytes(*c)));
            out[head + 8..head + 12]
                .copy_from_slice(&0xB1B0AFBAu32.wrapping_sub(sum).to_be_bytes());
        }
        Face::parse(&out, 0).map_err(|_| "无法重建系统字面")?;
        Ok(out)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = font;
        Err("系统未提供可移植字体数据，请导入对应的字体文件".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn collection_extraction_rebases_offsets_and_preserves_glyphs() {
        let original = crate::layout::SANS_FONT_BYTES;
        let mut collection = b"ttcf\0\x01\0\0\0\0\0\x01\0\0\0\x10".to_vec();
        collection.extend_from_slice(original);
        let count = u16::from_be_bytes([original[4], original[5]]) as usize;
        for index in 0..count {
            let at = 16 + 12 + index * 16 + 8;
            let old = u32::from_be_bytes(collection[at..at + 4].try_into().unwrap());
            collection[at..at + 4].copy_from_slice(&(old + 16).to_be_bytes());
        }
        let extracted = extract_face(&collection, 0).unwrap();
        let face = Face::parse(&extracted, 0).unwrap();
        let before = Face::parse(original, 0).unwrap();
        for c in "Resume 简里中文".chars() {
            assert_eq!(face.glyph_index(c), before.glyph_index(c));
        }
        assert!(extract_face(&collection, 1).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn native_resolution_caches_real_regular_and_italic_separately() {
        let regular = resolve_style("Arial", 400, false).unwrap();
        let italic = resolve_style("Arial", 400, true).unwrap();
        assert_ne!(regular, italic);
        assert!(!get(regular).italic && get(italic).italic);
        assert_eq!(regular, resolve("Arial", 400).unwrap());
        assert_eq!(italic, resolve_style("Arial", 400, true).unwrap());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn system_font_has_shared_preview_pdf_docx_metrics_and_cjk_fallback() {
        use crate::{
            appearance::{Appearance, FontChoice},
            layout::{FontKind, LayoutElement},
            render::Language,
        };
        let names = available_families().unwrap();
        assert!(names.iter().any(|name| name == "Arial"));
        let id = resolve("Arial", 400).unwrap();
        let data = get(id);
        assert!(!data.bytes.starts_with(b"ttcf"));
        let source = br#"{"basics":{"name":"Ada","summary":"English \u7b80\u91cc"}}"#;
        let content = crate::semantic::project(&crate::resume::parse_resume(source).unwrap());
        let theme =
            crate::parse_theme(include_bytes!("../../../assets/themes/zhibai.theme.json")).unwrap();
        let mut appearance = Appearance::default();
        appearance.body_font = Some(FontChoice::Font("font:Arial".into()));
        appearance.heading_font = appearance.body_font.clone();
        let effective = appearance.apply(&theme);
        let layout = crate::layout::layout_with_theme(&content, Language::Zh, &effective).unwrap();
        assert!(layout.pages.iter().flatten().any(
            |element| matches!(element,LayoutElement::Text(text) if text.font==FontKind::Local(id))
        ));
        assert!(layout.pages.iter().flatten().any(|element|matches!(element,LayoutElement::Text(text) if text.font==FontKind::Sans && text.text.contains('简'))));
        if data.pdf_embed {
            assert!(
                crate::pdf::render_with_theme(&content, Language::Zh, &effective)
                    .unwrap()
                    .starts_with(b"%PDF")
            );
        }
        if data.editable_embed {
            let bytes = crate::docx::render_with_theme(&content, Language::Zh, &effective).unwrap();
            let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
            let mut xml = String::new();
            zip.by_name("word/fontTable.xml")
                .unwrap()
                .read_to_string(&mut xml)
                .unwrap();
            assert!(xml.contains("Arial"));
            assert!(zip.by_name("word/fonts/local0.odttf").is_ok());
        }
    }

    #[test]
    fn private_install_is_idempotent_and_validates_before_writing() {
        let root = tempfile::tempdir().unwrap();
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts/NotoSansSC-Regular.ttf");
        assert!(
            install(&path, root.path())
                .unwrap()
                .contains(&"Noto Sans SC".to_owned())
        );
        install(&path, root.path()).unwrap();
        assert_eq!(installed_files(root.path()).len(), 1);
        let invalid = root.path().join("bad.ttf");
        fs::write(&invalid, b"not a font").unwrap();
        assert!(install(&invalid, &root.path().join("bad-library")).is_err());
        assert!(!root.path().join("bad-library").exists());
        assert!(!valid_family("unsafe\nname"));
    }
    #[test]
    fn registered_font_has_real_metrics_and_embedding_flags() {
        let id = register(
            ("test-font".into(), 400, false),
            crate::layout::SANS_FONT_BYTES.to_vec(),
        )
        .unwrap();
        let font = get(id);
        assert!(font.pdf_embed && font.editable_embed && font.subset_allowed);
        assert!(
            Face::parse(&font.bytes, 0)
                .unwrap()
                .glyph_index('简')
                .is_some()
        );
    }

    #[test]
    fn theme_font_fixture_has_native_family_glyphs_and_regular_weight() {
        let bytes = include_bytes!("../tests/fixtures/theme-font.ttf");
        let font = font_kit::font::Font::from_bytes(Arc::new(bytes.to_vec()), 0).unwrap();
        assert_eq!(font.family_name(), "Resume One Font Fixture");
        assert!(font.glyph_for_char('m').is_some());
        assert_eq!(font.properties().weight.0, 400.0);
    }

    #[test]
    fn real_italic_fixture_keeps_its_slant_and_can_embed_editably() {
        let bytes = include_bytes!("../tests/fixtures/theme-italic-font.ttf");
        let id = register_asset(bytes.to_vec()).unwrap();
        let font = get(id);
        assert!(font.italic && font.pdf_embed && font.editable_embed);
        assert_eq!(font.weight, 400);
        assert_eq!(font.family, "Resume One Italic Font Fixture");
        let face = Face::parse(&font.bytes, 0).unwrap();
        assert!(face.is_italic() && !face.is_variable());
        assert!(face.glyph_index('m').is_some());
        assert!(face.glyph_index('中').is_none());
        assert!(face.italic_angle() < 0.0);
    }

    #[test]
    fn asset_identity_is_hash_scoped_and_conflicting_family_bytes_are_rejected() {
        let bytes = include_bytes!("../tests/fixtures/theme-font.ttf");
        let id = register_asset(bytes.to_vec()).unwrap();
        assert_eq!(id, register_asset(bytes.to_vec()).unwrap());
        let mut changed = bytes.to_vec();
        let count = u16::from_be_bytes([changed[4], changed[5]]) as usize;
        let head = (0..count)
            .find_map(|index| {
                let at = 12 + index * 16;
                (&changed[at..at + 4] == b"head").then(|| {
                    u32::from_be_bytes(changed[at + 8..at + 12].try_into().unwrap()) as usize
                })
            })
            .unwrap();
        changed[head + 35] ^= 1; // only the last byte of the modification timestamp
        assert!(Face::parse(&changed, 0).is_ok());
        assert!(register_asset(changed).unwrap_err().contains("同名字族"));
        assert_eq!(&*get(id).bytes, bytes);
        assert!(register_asset(b"wOF2fake".to_vec()).is_err());
    }
}
