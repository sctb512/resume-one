//! Explicit, offline font-library projection. No installation, native font
//! resolution, source execution, or file writes occur here. The caller must
//! write the returned basenames next to the candidate and run core::load_theme
//! (and layout validation) before accepting the package.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, Metadata};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use resume_one_core::{fonts, parse_theme, theme_style::ThemeStyle};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const MAX_LIBRARY: u64 = 1024 * 1024;
const MAX_FONT: u64 = 8 * 1024 * 1024;
// License bytes are preserved, including their encoding and line endings.
const MAX_LICENSE: u64 = 1024 * 1024;
const MAX_FAMILIES: usize = 16;
const ROLES: [&str; 8] = [
    "body",
    "name",
    "sectionTitle",
    "itemTitle",
    "label",
    "summary",
    "sideDate",
    "meta",
];

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct FaceKey {
    family: String,
    weight: u16,
    style: String,
}

struct Entry {
    key: FaceKey,
    // Explicit alias for licensed/renamed static faces. Never guessed from a filename.
    font_family: String,
    path: PathBuf,
    sha256: String,
    license: String,
    license_path: PathBuf,
}

struct Verified {
    bytes: Vec<u8>,
    license: Vec<u8>,
}

fn fail(code: &str, message: impl std::fmt::Display) -> String {
    format!("{code}: {message}")
}

fn field<'a>(value: &'a Value, name: &str) -> Result<&'a str, String> {
    value[name]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| fail("FONT_LIBRARY_INVALID", format!("missing/invalid {name}")))
}

fn key(value: &Value) -> Result<FaceKey, String> {
    let family = field(value, "family")?;
    if !fonts::valid_family(family) {
        return Err(fail("FONT_FAMILY_INVALID", family));
    }
    let weight = value["weight"]
        .as_u64()
        .filter(|w| (1..=1000).contains(w))
        .ok_or_else(|| {
            fail(
                "FONT_WEIGHT_INVALID",
                "weight must be an integer in 1..=1000",
            )
        })?;
    let style = field(value, "fontStyle")?;
    if !matches!(style, "normal" | "italic") {
        return Err(fail(
            "FONT_STYLE_INVALID",
            "fontStyle must be normal or italic",
        ));
    }
    Ok(FaceKey {
        family: family.into(),
        weight: weight as u16,
        style: style.into(),
    })
}

struct FontRequest {
    primary: FaceKey,
    families: Option<Vec<String>>,
}

impl FontRequest {
    fn parse(value: &Value) -> Result<Self, String> {
        let primary = key(value)?;
        let families = value
            .get("families")
            .map(|value| {
                let array = value
                    .as_array()
                    .filter(|a| !a.is_empty() && a.len() <= MAX_FAMILIES)
                    .ok_or_else(|| {
                        fail(
                            "FONT_REQUEST_INVALID",
                            "families must contain 1..=16 family strings",
                        )
                    })?;
                // Validate even the suffix after a generic sentinel. Use the
                // same 256-byte/control-character bounds as real family names.
                array
                    .iter()
                    .map(|family| {
                        family
                            .as_str()
                            .filter(|s| fonts::valid_family(s))
                            .map(str::to_owned)
                            .ok_or_else(|| fail("FONT_REQUEST_INVALID", "invalid families entry"))
                    })
                    .collect::<Result<Vec<_>, String>>()
            })
            .transpose()?;
        Ok(Self { primary, families })
    }

    fn exact_index(&self, entries: &[Entry]) -> Option<usize> {
        let Some(families) = &self.families else {
            // Requests without a stack retain the original exact-key behavior.
            return entries.iter().position(|e| e.key == self.primary);
        };
        families
            .iter()
            .take_while(|family| !generic_family(family))
            .find_map(|family| {
                entries.iter().position(|entry| {
                    entry.key.family == *family
                        && entry.key.weight == self.primary.weight
                        && entry.key.style == self.primary.style
                })
            })
    }
}

fn generic_family(family: &str) -> bool {
    // A generic/system choice cannot be resolved through this explicit offline
    // library. Stop here rather than guessing a face or skipping past it.
    [
        "serif",
        "sans-serif",
        "monospace",
        "cursive",
        "fantasy",
        "system-ui",
        "ui-serif",
        "ui-sans-serif",
        "ui-monospace",
        "ui-rounded",
        "emoji",
        "math",
        "fangsong",
        "inherit",
        "initial",
        "unset",
        "revert",
        "revert-layer",
        "cjk-sans",
        "cjk-serif",
        "system-sans",
        "system-serif",
        "-apple-system",
        "BlinkMacSystemFont",
    ]
    .iter()
    .any(|generic| family.trim().eq_ignore_ascii_case(generic))
}

fn absolute(path: &Path) -> Result<(), String> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err(fail(
            "FONT_PATH_INVALID",
            "all input paths must be absolute and contain no . or ..",
        ));
    }
    Ok(())
}

fn same_identity(a: &Metadata, b: &Metadata) -> bool {
    if a.is_file() != b.is_file()
        || a.is_dir() != b.is_dir()
        || a.created().ok() != b.created().ok()
    {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if a.dev() != b.dev() || a.ino() != b.ino() {
            return false;
        }
    }
    true
}

fn same_file(a: &Metadata, b: &Metadata) -> bool {
    if !same_identity(a, b)
        || a.len() != b.len()
        || a.modified().ok() != b.modified().ok()
        || a.permissions().readonly() != b.permissions().readonly()
    {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if a.ctime() != b.ctime() || a.ctime_nsec() != b.ctime_nsec() {
            return false;
        }
    }
    true
}

// Pre/post handle metadata and all ancestor identities detect ordinary changes,
// including same-length rewrites on Unix. This is not an atomic OS snapshot.
#[derive(Default)]
struct Snapshot(BTreeMap<PathBuf, Metadata>);

impl Snapshot {
    fn observe(&mut self, path: &Path, limit: u64) -> Result<Metadata, String> {
        absolute(path)?;
        for ancestor in path.ancestors() {
            let meta = fs::symlink_metadata(ancestor)
                .map_err(|e| fail("FONT_IO", format!("{}: {e}", ancestor.display())))?;
            if meta.file_type().is_symlink() {
                return Err(fail("FONT_SYMLINK", ancestor.display()));
            }
            if ancestor != path && !meta.is_dir() {
                return Err(fail("FONT_PATH_INVALID", ancestor.display()));
            }
            if let Some(old) = self.0.get(ancestor) {
                if !(if old.is_file() {
                    same_file(old, &meta)
                } else {
                    same_identity(old, &meta)
                }) {
                    return Err(fail("FONT_CHANGED", ancestor.display()));
                }
            } else {
                self.0.insert(ancestor.to_owned(), meta);
            }
        }
        let meta = self.0[path].clone();
        if !meta.is_file() || meta.len() > limit {
            return Err(fail(
                "FONT_FILE_LIMIT",
                format!("{} must be a regular file <= {limit} bytes", path.display()),
            ));
        }
        Ok(meta)
    }

    fn read(&mut self, path: &Path, limit: u64) -> Result<Vec<u8>, String> {
        let before = self.observe(path, limit)?;
        let mut file = File::open(path).map_err(|e| fail("FONT_IO", e))?;
        let opened = file.metadata().map_err(|e| fail("FONT_IO", e))?;
        if !same_file(&before, &opened) {
            return Err(fail("FONT_CHANGED", path.display()));
        }
        let mut bytes = Vec::new();
        (&mut file)
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| fail("FONT_IO", e))?;
        let after = file.metadata().map_err(|e| fail("FONT_IO", e))?;
        if bytes.len() as u64 != before.len()
            || bytes.len() as u64 > limit
            || !same_file(&before, &after)
        {
            return Err(fail("FONT_CHANGED", path.display()));
        }
        self.observe(path, limit)?;
        Ok(bytes)
    }

    fn verify(&self) -> Result<(), String> {
        for (path, before) in &self.0 {
            let after = fs::symlink_metadata(path).map_err(|e| fail("FONT_CHANGED", e))?;
            if after.file_type().is_symlink()
                || !(if before.is_file() {
                    same_file(before, &after)
                } else {
                    same_identity(before, &after)
                })
            {
                return Err(fail("FONT_CHANGED", path.display()));
            }
        }
        Ok(())
    }
}

fn library_entries(data: &[u8]) -> Result<Vec<Entry>, String> {
    let value: Value = serde_json::from_slice(data).map_err(|e| fail("FONT_LIBRARY_JSON", e))?;
    let array = if value.is_array() {
        value.as_array()
    } else {
        if value["formatVersion"].as_u64() != Some(1) {
            return Err(fail("FONT_LIBRARY_VERSION", "expected formatVersion: 1"));
        }
        value["fonts"].as_array()
    }
    .ok_or_else(|| {
        fail(
            "FONT_LIBRARY_INVALID",
            "expected an array or {formatVersion:1,fonts:[...]}",
        )
    })?;
    if array.len() > 128 {
        return Err(fail("FONT_LIBRARY_LIMIT", "at most 128 font entries"));
    }
    let mut entries = Vec::new();
    let mut keys = BTreeSet::new();
    for value in array {
        let key = key(value)?;
        if !keys.insert(key.clone()) {
            return Err(fail("FONT_LIBRARY_AMBIGUOUS", &key.family));
        }
        let path = PathBuf::from(field(value, "path")?);
        let license_path = PathBuf::from(field(value, "licensePath")?);
        absolute(&path)?;
        absolute(&license_path)?;
        if !matches!(
            path.extension()
                .and_then(|s| s.to_str())
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some("ttf" | "otf")
        ) {
            return Err(fail(
                "FONT_FORMAT_UNSUPPORTED",
                "only local .ttf/.otf files are supported",
            ));
        }
        let sha256 = field(value, "sha256")?;
        if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(fail(
                "FONT_HASH_INVALID",
                "sha256 must have 64 hexadecimal digits",
            ));
        }
        let license = field(value, "license")?;
        if license.chars().count() > 80 || license.chars().any(char::is_control) {
            return Err(fail(
                "FONT_LICENSE_INVALID",
                "license must be a nonempty declaration <= 80 characters",
            ));
        }
        let font_family = value["fontFamily"]
            .as_str()
            .unwrap_or(&key.family)
            .to_owned();
        if !fonts::valid_family(&font_family) {
            return Err(fail("FONT_FAMILY_INVALID", &font_family));
        }
        entries.push(Entry {
            key,
            font_family,
            path,
            sha256: sha256.to_ascii_lowercase(),
            license: license.into(),
            license_path,
        });
    }
    Ok(entries)
}

fn u16_at(bytes: &[u8], at: usize) -> Result<u16, String> {
    let part = bytes
        .get(at..at + 2)
        .ok_or_else(|| fail("FONT_INVALID", "truncated font table"))?;
    Ok(u16::from_be_bytes([part[0], part[1]]))
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32, String> {
    let part = bytes
        .get(at..at + 4)
        .ok_or_else(|| fail("FONT_INVALID", "truncated font table"))?;
    Ok(u32::from_be_bytes(part.try_into().unwrap()))
}

// Only metadata needed for an exact match is read here. Full font parsing stays
// in the existing core parser; no indirect dependency is imported or added.
fn verify_face(bytes: &[u8], declared: &FaceKey) -> Result<(), String> {
    if !(bytes.starts_with(b"\0\x01\0\0") || bytes.starts_with(b"OTTO")) {
        return Err(fail(
            "FONT_FORMAT_UNSUPPORTED",
            "expected a standalone static TTF/OTF face",
        ));
    }
    fonts::extract_face(bytes, 0).map_err(|e| fail("FONT_INVALID", e))?;
    let count = u16_at(bytes, 4)? as usize;
    let mut tables = BTreeMap::new();
    for i in 0..count {
        let at = 12 + 16 * i;
        let tag = bytes
            .get(at..at + 4)
            .ok_or_else(|| fail("FONT_INVALID", "truncated directory"))?;
        let start = u32_at(bytes, at + 8)? as usize;
        let end = start
            .checked_add(u32_at(bytes, at + 12)? as usize)
            .ok_or_else(|| fail("FONT_INVALID", "table overflow"))?;
        let table = bytes
            .get(start..end)
            .ok_or_else(|| fail("FONT_INVALID", "table outside file"))?;
        if tables.insert(tag, table).is_some() {
            return Err(fail("FONT_INVALID", "duplicate font table"));
        }
    }
    if tables.contains_key(b"fvar".as_slice()) {
        return Err(fail(
            "FONT_VARIABLE_UNSUPPORTED",
            "variable fonts must be supplied as real static faces",
        ));
    }
    let os2 = tables
        .get(b"OS/2".as_slice())
        .ok_or_else(|| fail("FONT_METADATA_MISSING", "OS/2 table required"))?;
    let version = u16_at(os2, 0)?;
    let minimum = match version {
        0 => 78,
        1 => 86,
        2..=4 => 96,
        5 => 100,
        _ => usize::MAX,
    };
    if os2.len() < minimum {
        return Err(fail("FONT_INVALID", "unsupported/truncated OS/2 table"));
    }
    let weight = u16_at(os2, 4)?;
    let selection = u16_at(os2, 62)?;
    let post_italic = if let Some(post) = tables.get(b"post".as_slice()) {
        if post.len() < 32 {
            return Err(fail("FONT_INVALID", "truncated post table"));
        }
        let version = u32_at(post, 0)?;
        matches!(
            version,
            0x00010000 | 0x00020000 | 0x00025000 | 0x00030000 | 0x00040000
        ) && u32_at(post, 4)? != 0
    } else {
        false
    };
    let italic = selection & 1 != 0 || (version >= 4 && selection & (1 << 9) != 0) || post_italic;
    let names = tables
        .get(b"name".as_slice())
        .ok_or_else(|| fail("FONT_METADATA_MISSING", "name table required"))?;
    let name_count = u16_at(names, 2)? as usize;
    let strings = u16_at(names, 4)? as usize;
    let mut family = None;
    // Match core's preferred typographic family, then legacy family, and its
    // Unicode name decoding. No aliases, fuzzy match, or weight substitutions.
    for id in [16, 1] {
        for i in 0..name_count {
            let at = 6 + i * 12;
            let platform = u16_at(names, at)?;
            let encoding = u16_at(names, at + 2)?;
            if u16_at(names, at + 6)? != id || !(platform == 0 || (platform == 3 && encoding <= 1))
            {
                continue;
            }
            let len = u16_at(names, at + 8)? as usize;
            let start = strings + u16_at(names, at + 10)? as usize;
            let text = names
                .get(start..start + len)
                .ok_or_else(|| fail("FONT_INVALID", "name outside table"))?;
            if !len.is_multiple_of(2) {
                return Err(fail("FONT_INVALID", "odd UTF-16 name length"));
            }
            let units = text
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect::<Vec<_>>();
            if let Ok(name) = String::from_utf16(&units)
                && fonts::valid_family(&name)
            {
                family = Some(name);
                break;
            }
        }
        if family.is_some() {
            break;
        }
    }
    if family.as_deref() != Some(declared.family.as_str())
        || weight != declared.weight
        || italic != (declared.style == "italic")
    {
        return Err(fail(
            "FONT_METADATA_MISMATCH",
            format!(
                "declared {declared:?}; actual family={family:?}, weight={weight}, italic={italic}"
            ),
        ));
    }
    Ok(())
}

fn candidate_error(issues: Vec<resume_one_core::Diagnostic>) -> String {
    fail(
        "FONT_CANDIDATE_INVALID",
        issues
            .into_iter()
            .map(|d| format!("{} {}: {}", d.code, d.path, d.message))
            .collect::<Vec<_>>()
            .join("; "),
    )
}

/// `requests` is [{role,family,weight,fontStyle,families?}], with the source's
/// primary family, never a replacement already put into the schema. An optional
/// decoded CSS stack is authoritative in its original order, up to a generic.
/// Candidate changes are transactional: every error leaves it untouched.
type FontProjection = (Value, Vec<(String, Vec<u8>)>);

pub(crate) fn apply(
    candidate: &mut Value,
    requests: &Value,
    library: &Path,
) -> Result<FontProjection, String> {
    let requests = requests
        .as_array()
        .ok_or_else(|| fail("FONT_REQUEST_INVALID", "requests must be an array"))?;
    if requests.len() > ROLES.len() {
        return Err(fail("FONT_REQUEST_LIMIT", "at most eight distinct roles"));
    }
    let mut wanted = BTreeMap::new();
    for request in requests {
        let role = field(request, "role")?;
        if !ROLES.contains(&role) || wanted.insert(role, FontRequest::parse(request)?).is_some() {
            return Err(fail("FONT_REQUEST_INVALID", "unknown or duplicate role"));
        }
    }
    let mut snapshot = Snapshot::default();
    let entries = library_entries(&snapshot.read(library, MAX_LIBRARY)?)?;
    let parsed =
        parse_theme(&serde_json::to_vec(candidate).map_err(|e| fail("FONT_CANDIDATE_INVALID", e))?)
            .map_err(candidate_error)?;
    let style = ThemeStyle::from_manifest(&parsed);
    let old = [
        &style.typography.body,
        &style.typography.name,
        &style.typography.section_title,
        &style.typography.item_title,
        &style.typography.label,
        &style.typography.summary,
        &style.typography.side_date,
        &style.typography.meta,
    ];
    let mut next = candidate.clone();
    let mut assets = candidate
        .pointer("/assets/fonts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut selected = BTreeMap::<usize, (String, String)>::new();
    let mut mappings = Vec::new();
    let mut differences = Vec::new();
    let mut files = Vec::new();
    let mut licenses = Vec::<(Vec<u8>, String)>::new();
    let mut verified = BTreeMap::new();
    // Verify every explicitly supplied entry, retaining bytes for requested
    // faces only. Memory is bounded by the four selected faces plus one read.
    let mut indices = BTreeMap::new();
    let mut selected_hashes = BTreeSet::new();
    let mut available = 4usize.saturating_sub(assets.len());
    for role in ROLES {
        if let Some(request) = wanted.get(role)
            && (100..=900).contains(&request.primary.weight)
            && request.primary.weight % 100 == 0
            && let Some(index) = request.exact_index(&entries)
        {
            let hash = &entries[index].sha256;
            if selected_hashes.contains(hash) || available > 0 {
                if selected_hashes.insert(hash.clone()) {
                    available -= 1;
                }
                indices.insert(role, index);
            }
        }
    }
    let keep = indices.values().copied().collect::<BTreeSet<_>>();
    for (index, entry) in entries.iter().enumerate() {
        let bytes = snapshot.read(&entry.path, MAX_FONT)?;
        if format!("{:x}", Sha256::digest(&bytes)) != entry.sha256 {
            return Err(fail("FONT_HASH_MISMATCH", entry.path.display()));
        }
        let mut actual_key = entry.key.clone();
        actual_key.family = entry.font_family.clone();
        verify_face(&bytes, &actual_key)?;
        let license = snapshot.read(&entry.license_path, MAX_LICENSE)?;
        if license.is_empty() {
            return Err(fail(
                "FONT_LICENSE_INVALID",
                "licensePath must contain the original nonempty license",
            ));
        }
        if keep.contains(&index) {
            verified.insert(index, Verified { bytes, license });
        }
    }
    let mut used_ids = assets
        .iter()
        .filter_map(|a| a["id"].as_str().map(str::to_owned))
        .collect::<BTreeSet<_>>();
    let mut used_names = assets
        .iter()
        .filter_map(|a| a["path"].as_str().map(str::to_owned))
        .collect::<BTreeSet<_>>();
    for (position, role) in ROLES.into_iter().enumerate() {
        let Some(request) = wanted.get(role) else {
            continue;
        };
        let primary = &request.primary;
        let before = old[position];
        let mut source = json!({"role":role,"family":primary.family,"weight":primary.weight,"fontStyle":primary.style});
        if let Some(families) = &request.families {
            source["families"] = json!(families);
        }
        let retained = json!({"family":before.family,"weight":before.weight,"fontStyle":if before.italic {"italic"} else {"normal"}});
        let Some(&index) = indices.get(role) else {
            let code = if !(100..=900).contains(&primary.weight) || primary.weight % 100 != 0 {
                "FONT_WEIGHT_UNREPRESENTABLE"
            } else if request.exact_index(&entries).is_some() {
                "FONT_ASSET_LIMIT"
            } else {
                "FONT_FACE_UNAVAILABLE"
            };
            differences.push(json!({"code":code,"source":source,"retained":retained,"message":"No permitted exact family/weight/slant face was applied; existing candidate font retained.","equivalent":false}));
            mappings.push(json!({"role":role,"requested":source,"status":"retained-fallback","applied":retained,"equivalent":false}));
            continue;
        };
        let entry = &entries[index];
        if let Some(families) = &request.families
            && entry.key.family != primary.family
        {
            let skipped = families
                .iter()
                .take_while(|family| family.as_str() != entry.key.family.as_str())
                .collect::<Vec<_>>();
            differences.push(json!({"code":"FONT_STACK_FALLBACK","source":source,
                "selectedFamily":entry.key.family,"skippedFamilies":skipped,
                "message":"A concrete family from the supplied CSS stack was selected instead of the primary family, with the exact requested weight/slant. This does not establish equivalence to the original primary family.","equivalent":false}));
        }
        if entry.font_family != entry.key.family {
            differences.push(json!({"code":"FONT_EXPLICIT_SOURCE_ALIAS","sourceFamily":entry.key.family,"fontFamily":entry.font_family,"sha256":entry.sha256,"message":"字体资料库显式指定了原 CSS 字族到已改名静态字面的对应关系；元数据和字重斜体已校验，但工具不据此声称字形来源等价。","equivalent":false}));
        }
        let existing = selected
            .iter()
            .find(|(other, _)| entries[**other].sha256 == entry.sha256)
            .map(|(_, value)| value.clone());
        let (id, license_name) = if let Some(value) = existing {
            value
        } else {
            let data = verified.remove(&index).expect("selected face was verified");
            let extension = entry
                .path
                .extension()
                .unwrap()
                .to_str()
                .unwrap()
                .to_ascii_lowercase();
            let (id, name) = (0usize..)
                .find_map(|n| {
                    let id = format!("font-{n}");
                    let name = format!("{id}.{extension}");
                    (!used_ids.contains(&id) && !used_names.contains(&name)).then_some((id, name))
                })
                .unwrap();
            used_ids.insert(id.clone());
            used_names.insert(name.clone());
            let license_name = if let Some((_, name)) =
                licenses.iter().find(|(bytes, _)| bytes == &data.license)
            {
                name.clone()
            } else {
                let name = (0usize..)
                    .map(|n| format!("font-license-{n}.txt"))
                    .find(|n| !used_names.contains(n))
                    .unwrap();
                used_names.insert(name.clone());
                files.push((name.clone(), data.license.clone()));
                licenses.push((data.license, name.clone()));
                name
            };
            files.push((name.clone(), data.bytes));
            assets.push(json!({"id":id,"path":name,"sha256":entry.sha256,"license":entry.license}));
            selected.insert(index, (id.clone(), license_name.clone()));
            (id, license_name)
        };
        if next["typography"].is_null() {
            next["typography"] = json!({});
        }
        if next["typography"][role].is_null() {
            next["typography"][role] = json!({});
        }
        next["typography"][role]["family"] = json!(format!("asset:{id}"));
        next["typography"][role]["weight"] = json!(primary.weight);
        next["typography"][role]["fontStyle"] = json!(primary.style);
        mappings.push(json!({"role":role,"requested":source,"status":"exact-static-face","assetId":id,
            "selectedFamily":entry.key.family,
            "applied":{"family":format!("asset:{id}"),"weight":primary.weight,"fontStyle":primary.style},
            "sha256":entry.sha256,"license":entry.license,"licensePath":license_name,
            "metadataVerified":true,"equivalent":false}));
        differences.push(json!({"code":"FONT_FALLBACK_DIFFERENCE","source":source,"assetId":id,
            "message":"Only the selected exact static face is mapped. Missing glyphs retain core fallback; the source CSS fallback stack and full glyph coverage are not reproduced or certified.","equivalent":false}));
    }
    if !selected.is_empty() {
        // Pin inherited roles to their original effective font unless explicitly
        // mapped. Otherwise changing body/meta would silently change fallbacks.
        for (position, role) in ROLES.into_iter().enumerate() {
            if indices.contains_key(role) {
                continue;
            }
            let before = old[position];
            if next["typography"][role].is_null() {
                next["typography"][role] = json!({});
            }
            next["typography"][role]["family"] = json!(before.family);
            next["typography"][role]["weight"] = json!(before.weight);
            next["typography"][role]["fontStyle"] =
                json!(if before.italic { "italic" } else { "normal" });
        }
        if next["assets"].is_null() {
            next["assets"] = json!({});
        }
        next["assets"]["fonts"] = json!(assets);
    }
    parse_theme(&serde_json::to_vec(&next).map_err(|e| fail("FONT_CANDIDATE_INVALID", e))?)
        .map_err(candidate_error)?;
    snapshot.verify()?;
    *candidate = next;
    Ok((
        json!({"fontMappings":mappings,"differences":differences,
        "libraryEntries":entries.len(),"coreLoadThemeRequired":true,"layoutValidationRequired":true,
        "validation":"SHA-256 and static family/weight/slant checked; final core::load_theme and layout validation required",
        "licensePolicy":"declared license and original bytes copied; not a legal clearance"}),
        files,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const REGULAR: &[u8] = include_bytes!("../../resume-one-core/tests/fixtures/theme-font.ttf");
    const ITALIC: &[u8] =
        include_bytes!("../../resume-one-core/tests/fixtures/theme-italic-font.ttf");
    const LICENSE: &[u8] = b"Original license\r\nCopyright fixture author\r\n";

    struct Fixture {
        _temp: tempfile::TempDir,
        root: PathBuf,
        library: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            // macOS /var is a symlink; inputs must contain no symlink ancestors.
            let root = fs::canonicalize(temp.path()).unwrap();
            let library = root.join("library.json");
            fs::write(root.join("license.txt"), LICENSE).unwrap();
            Self {
                _temp: temp,
                root,
                library,
            }
        }
        fn entry(&self, name: &str, bytes: &[u8], family: &str, weight: u16, style: &str) -> Value {
            let path = self.root.join(name);
            fs::write(&path, bytes).unwrap();
            json!({"family":family,"weight":weight,"fontStyle":style,"path":path,
                "sha256":format!("{:x}",Sha256::digest(bytes)),"license":"OFL-1.1","licensePath":self.root.join("license.txt")})
        }
        fn write(&self, entries: Value) {
            fs::write(&self.library, serde_json::to_vec(&entries).unwrap()).unwrap();
        }
    }

    fn candidate() -> Value {
        serde_json::from_slice(include_bytes!("../../../assets/themes/zhibai.theme.json")).unwrap()
    }
    fn request(role: &str, family: &str, weight: u16, style: &str) -> Value {
        json!({"role":role,"family":family,"weight":weight,"fontStyle":style})
    }

    #[test]
    fn exact_faces_copy_original_bytes_and_deduplicate_license() {
        let f = Fixture::new();
        let family = "Resume One Font Fixture";
        let a = f.entry("regular.ttf", REGULAR, family, 400, "normal");
        let mut b = f.entry("italic.ttf", ITALIC, family, 400, "italic");
        b["fontFamily"] = json!("Resume One Italic Font Fixture");
        f.write(json!({"formatVersion":1,"fonts":[a,b]}));
        let mut c = candidate();
        let (report, files) = apply(
            &mut c,
            &json!([
                request("label", family, 400, "italic"),
                request("name", family, 400, "normal"),
                request("body", family, 400, "normal")
            ]),
            &f.library,
        )
        .unwrap();
        assert_eq!(c["typography"]["body"]["family"], "asset:font-0");
        assert_eq!(c["typography"]["name"]["family"], "asset:font-0");
        assert_eq!(c["typography"]["label"]["family"], "asset:font-1");
        assert_eq!(files.len(), 3);
        assert!(
            files
                .iter()
                .any(|(name, b)| name == "font-0.ttf" && b == REGULAR)
        );
        assert!(
            files
                .iter()
                .any(|(name, b)| name == "font-license-0.txt" && b == LICENSE)
        );
        assert_eq!(report["fontMappings"].as_array().unwrap().len(), 3);
        for (name, bytes) in &files {
            assert!(!name.starts_with('.'));
            assert_eq!(Path::new(name).components().count(), 1);
            fs::write(f.root.join(name), bytes).unwrap();
        }
        fs::write(
            f.root.join("test.theme.json"),
            serde_json::to_vec(&c).unwrap(),
        )
        .unwrap();
        let loaded = resume_one_core::load_theme(&f.root, Path::new("test.theme.json")).unwrap();
        assert!(resume_one_core::layout::check_theme(&loaded).is_ok());
    }

    #[test]
    fn no_alias_weight_or_slant_substitution() {
        let f = Fixture::new();
        let family = "Resume One Font Fixture";
        f.write(json!([f.entry(
            "regular.ttf",
            REGULAR,
            family,
            400,
            "normal"
        )]));
        let mut c = candidate();
        let before = c.clone();
        let (r, files) = apply(
            &mut c,
            &json!([
                request("body", family, 600, "normal"),
                request("name", family, 400, "italic"),
                request("summary", "Other Family", 400, "normal")
            ]),
            &f.library,
        )
        .unwrap();
        assert!(files.is_empty());
        assert_eq!(c, before);
        assert_eq!(r["differences"].as_array().unwrap().len(), 3);
        assert!(
            r["fontMappings"]
                .as_array()
                .unwrap()
                .iter()
                .all(|m| m["status"] == "retained-fallback")
        );
    }

    #[test]
    fn stack_selects_first_exact_family_in_css_order() {
        let f = Fixture::new();
        let family = "Resume One Font Fixture";
        let mut later = f.entry("later.ttf", REGULAR, "Later Family", 400, "normal");
        later["fontFamily"] = json!(family);
        // Library order must not override CSS order, even for identical bytes.
        f.write(json!([
            later,
            f.entry("first.ttf", REGULAR, family, 400, "normal")
        ]));
        for families in [
            json!([family, "Later Family"]),
            json!([family, "sans-serif", "Later Family"]),
        ] {
            let mut wanted = request("body", family, 400, "normal");
            wanted["families"] = families.clone();
            let (report, _) = apply(&mut candidate(), &json!([wanted]), &f.library).unwrap();
            let mapping = &report["fontMappings"][0];
            assert_eq!(mapping["selectedFamily"], family);
            assert_eq!(mapping["requested"]["families"], families);
            assert_eq!(mapping["status"], "exact-static-face");
            assert!(report["differences"].as_array().unwrap().iter().all(|d| {
                d["code"] != "FONT_STACK_FALLBACK" && d["code"] != "FONT_EXPLICIT_SOURCE_ALIAS"
            }));
        }
    }

    #[test]
    fn stack_falls_back_to_later_exact_family_with_explicit_evidence() {
        let f = Fixture::new();
        let family = "Resume One Font Fixture";
        f.write(json!([f.entry(
            "regular.ttf",
            REGULAR,
            family,
            400,
            "normal"
        )]));
        let mut wanted = request("body", "Missing Primary", 400, "normal");
        let families = json!(["Missing Primary", "Also Missing", family, "sans-serif"]);
        wanted["families"] = families.clone();
        let mut c = candidate();
        let (report, files) = apply(&mut c, &json!([wanted]), &f.library).unwrap();
        assert_eq!(c["typography"]["body"]["family"], "asset:font-0");
        assert!(files.iter().any(|(_, bytes)| bytes == REGULAR));
        let mapping = &report["fontMappings"][0];
        assert_eq!(mapping["requested"]["family"], "Missing Primary");
        assert_eq!(mapping["requested"]["families"], families);
        assert_eq!(mapping["selectedFamily"], family);
        assert_eq!(mapping["equivalent"], false);
        let fallback = report["differences"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["code"] == "FONT_STACK_FALLBACK")
            .unwrap();
        assert_eq!(fallback["source"]["family"], "Missing Primary");
        assert_eq!(fallback["selectedFamily"], family);
        assert_eq!(
            fallback["skippedFamilies"],
            json!(["Missing Primary", "Also Missing"])
        );
        assert_eq!(fallback["equivalent"], false);
    }

    #[test]
    fn generic_sentinels_stop_stack_search_even_with_explicit_library_alias() {
        let f = Fixture::new();
        let family = "Resume One Font Fixture";
        let regular = f.entry("regular.ttf", REGULAR, family, 400, "normal");
        for generic in [
            "sans-serif",
            "serif",
            "system-ui",
            "monospace",
            "cursive",
            "fantasy",
            "ui-serif",
            "ui-sans-serif",
            "ui-monospace",
            "ui-rounded",
            "emoji",
            "math",
            "fangsong",
            "inherit",
            "initial",
            "unset",
            "revert",
            "revert-layer",
            "cjk-sans",
            "cjk-serif",
            "system-sans",
            "system-serif",
            "SANS-SERIF",
            " sans-serif ",
            "-apple-system",
            "BlinkMacSystemFont",
        ] {
            let mut alias = f.entry("generic.ttf", REGULAR, generic, 400, "normal");
            alias["fontFamily"] = json!(family);
            f.write(json!([regular.clone(), alias]));
            for families in [
                json!(["Missing", generic, family]),
                json!([generic, family]),
            ] {
                let mut wanted = request("body", "Missing", 400, "normal");
                wanted["families"] = families.clone();
                let mut c = candidate();
                let before = c.clone();
                let (report, files) = apply(&mut c, &json!([wanted]), &f.library).unwrap();
                assert_eq!(c, before, "{generic}");
                assert!(files.is_empty(), "{generic}");
                assert_eq!(report["fontMappings"][0]["status"], "retained-fallback");
                assert_eq!(report["fontMappings"][0]["requested"]["families"], families);
                assert_eq!(report["differences"][0]["code"], "FONT_FACE_UNAVAILABLE");
            }
        }
    }

    #[test]
    fn stack_requires_exact_weight_and_real_slant_without_library_wide_fallback() {
        let f = Fixture::new();
        let regular = "Resume One Font Fixture";
        let italic = "Resume One Italic Font Fixture";
        f.write(json!([
            f.entry("regular.ttf", REGULAR, regular, 400, "normal"),
            f.entry("italic.ttf", ITALIC, italic, 400, "italic")
        ]));
        for (first, second, style, bytes) in [
            (regular, italic, "italic", ITALIC),
            (italic, regular, "normal", REGULAR),
        ] {
            let mut wanted = request("body", first, 400, style);
            wanted["families"] = json!([first, second]);
            let mut c = candidate();
            let (report, files) = apply(&mut c, &json!([wanted]), &f.library).unwrap();
            assert_eq!(report["fontMappings"][0]["selectedFamily"], second);
            assert_eq!(c["typography"]["body"]["fontStyle"], style);
            assert!(files.iter().any(|(_, actual)| actual == bytes));
            assert!(report["differences"].as_array().unwrap().iter().any(|d| {
                d["code"] == "FONT_STACK_FALLBACK" && d["skippedFamilies"] == json!([first])
            }));
        }
        for (family, weight, style, families, code) in [
            (
                regular,
                600,
                "normal",
                json!([regular, italic]),
                "FONT_FACE_UNAVAILABLE",
            ),
            (
                regular,
                450,
                "normal",
                json!([regular, italic]),
                "FONT_WEIGHT_UNREPRESENTABLE",
            ),
            (
                "Missing",
                400,
                "normal",
                json!(["Missing", "Also Missing"]),
                "FONT_FACE_UNAVAILABLE",
            ),
        ] {
            let mut wanted = request("body", family, weight, style);
            wanted["families"] = families;
            let mut c = candidate();
            let before = c.clone();
            let (report, files) = apply(&mut c, &json!([wanted]), &f.library).unwrap();
            assert_eq!(c, before);
            assert!(files.is_empty());
            assert_eq!(report["differences"][0]["code"], code);
            assert_eq!(report["fontMappings"][0]["equivalent"], false);
        }
    }

    #[test]
    fn explicit_source_alias_is_preserved_for_legacy_and_stack_requests() {
        let f = Fixture::new();
        let alias = "Licensed CSS Family";
        let actual = "Resume One Font Fixture";
        let mut entry = f.entry("renamed.ttf", REGULAR, alias, 400, "normal");
        entry["fontFamily"] = json!(actual);
        f.write(json!([entry]));
        for stack in [false, true] {
            let mut wanted = request("body", if stack { "Missing" } else { alias }, 400, "normal");
            if stack {
                wanted["families"] = json!(["Missing", alias]);
            }
            let (report, files) = apply(&mut candidate(), &json!([wanted]), &f.library).unwrap();
            assert!(files.iter().any(|(_, bytes)| bytes == REGULAR));
            let mapping = &report["fontMappings"][0];
            assert_eq!(mapping["selectedFamily"], alias);
            assert_eq!(mapping["metadataVerified"], true);
            assert_eq!(mapping["equivalent"], false);
            assert_eq!(mapping["requested"].get("families").is_some(), stack);
            assert!(report["differences"].as_array().unwrap().iter().any(|d| {
                d["code"] == "FONT_EXPLICIT_SOURCE_ALIAS"
                    && d["sourceFamily"] == alias
                    && d["fontFamily"] == actual
            }));
            assert_eq!(
                report["differences"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|d| { d["code"] == "FONT_STACK_FALLBACK" }),
                stack
            );
        }
    }

    #[test]
    fn malformed_or_oversized_stacks_are_rejected_transactionally() {
        let f = Fixture::new();
        f.write(json!([]));
        for families in [
            Value::Null,
            json!("Family"),
            json!({"family":"Family"}),
            json!([]),
            json!(vec!["Family"; 17]),
            json!([false]),
            json!([400]),
            json!([null]),
            json!([{}]),
            json!([[]]),
            json!([""]),
            json!(["  "]),
            json!(["Bad\nFamily"]),
            json!(["Bad\u{202e}Family"]),
            json!(["a".repeat(257)]),
            json!(["é".repeat(129)]),
            // Validate the entire list, including unreachable suffixes.
            json!(["sans-serif", false]),
        ] {
            let mut wanted = request("body", "Family", 400, "normal");
            wanted["families"] = families;
            let mut c = candidate();
            let before = c.clone();
            let error = apply(&mut c, &json!([wanted]), &f.library).unwrap_err();
            assert!(error.starts_with("FONT_REQUEST_INVALID"), "{error}");
            assert_eq!(c, before);
        }
    }

    #[test]
    fn stack_accepts_sixteen_bounded_families() {
        let f = Fixture::new();
        let family = "Resume One Font Fixture";
        f.write(json!([f.entry(
            "regular.ttf",
            REGULAR,
            family,
            400,
            "normal"
        )]));
        let mut families = vec!["a".repeat(256)];
        families.extend((0..14).map(|n| format!("Missing {n}")));
        families.push(family.into());
        let mut wanted = request("body", &families[0], 400, "normal");
        wanted["families"] = json!(families);
        let (report, _) = apply(&mut candidate(), &json!([wanted]), &f.library).unwrap();
        assert_eq!(report["fontMappings"][0]["selectedFamily"], family);
    }

    #[test]
    fn stack_fallback_respects_existing_four_asset_limit() {
        let f = Fixture::new();
        let family = "Resume One Font Fixture";
        f.write(json!([f.entry(
            "regular.ttf",
            REGULAR,
            family,
            400,
            "normal"
        )]));
        let mut c = candidate();
        c["assets"] = json!({"fonts":(0..4).map(|n| json!({
            "id":format!("existing-{n}"),"path":format!("existing-{n}.ttf"),
            "sha256":"0".repeat(64),"license":"OFL-1.1"
        })).collect::<Vec<_>>()});
        let before = c.clone();
        let mut wanted = request("body", "Missing", 400, "normal");
        wanted["families"] = json!(["Missing", family]);
        let (report, files) = apply(&mut c, &json!([wanted]), &f.library).unwrap();
        assert_eq!(c, before);
        assert!(files.is_empty());
        assert_eq!(report["differences"][0]["code"], "FONT_ASSET_LIMIT");
        assert_eq!(report["fontMappings"][0]["status"], "retained-fallback");
    }

    #[test]
    fn rejects_hash_wrong_real_weight_and_missing_license_transactionally() {
        let f = Fixture::new();
        let family = "Resume One Font Fixture";
        let original = f.entry("font.ttf", REGULAR, family, 400, "normal");
        for (field, value, code) in [
            ("sha256", json!("0".repeat(64)), "FONT_HASH_MISMATCH"),
            ("weight", json!(600), "FONT_METADATA_MISMATCH"),
            ("family", json!("Wrong Family"), "FONT_METADATA_MISMATCH"),
            ("fontStyle", json!("italic"), "FONT_METADATA_MISMATCH"),
            ("licensePath", Value::Null, "FONT_LIBRARY_INVALID"),
        ] {
            let mut entry = original.clone();
            entry[field] = value;
            f.write(json!([entry]));
            let mut c = candidate();
            let before = c.clone();
            let error = apply(
                &mut c,
                &json!([request("body", family, 400, "normal")]),
                &f.library,
            )
            .unwrap_err();
            assert!(error.starts_with(code), "{error}");
            assert_eq!(c, before);
        }
    }

    #[test]
    fn bounds_and_relative_paths_are_rejected() {
        let f = Fixture::new();
        f.write(json!([]));
        assert!(
            apply(&mut candidate(), &json!([]), Path::new("library.json"))
                .unwrap_err()
                .starts_with("FONT_PATH_INVALID")
        );
        fs::write(&f.library, vec![b' '; MAX_LIBRARY as usize + 1]).unwrap();
        assert!(
            apply(&mut candidate(), &json!([]), &f.library)
                .unwrap_err()
                .starts_with("FONT_FILE_LIMIT")
        );
        f.write(json!(vec![json!({}); 129]));
        assert!(
            apply(&mut candidate(), &json!([]), &f.library)
                .unwrap_err()
                .starts_with("FONT_LIBRARY_LIMIT")
        );
        let mut entry = f.entry(
            "font.ttf",
            REGULAR,
            "Resume One Font Fixture",
            400,
            "normal",
        );
        File::options()
            .write(true)
            .open(f.root.join("font.ttf"))
            .unwrap()
            .set_len(MAX_FONT + 1)
            .unwrap();
        f.write(json!([entry.clone()]));
        assert!(
            apply(&mut candidate(), &json!([]), &f.library)
                .unwrap_err()
                .starts_with("FONT_FILE_LIMIT")
        );
        entry["path"] = json!("font.ttf");
        f.write(json!([entry]));
        assert!(
            apply(&mut candidate(), &json!([]), &f.library)
                .unwrap_err()
                .starts_with("FONT_PATH_INVALID")
        );
    }

    #[test]
    fn detects_same_length_change_after_read() {
        let f = Fixture::new();
        let path = f.root.join("watched.txt");
        fs::write(&path, b"before").unwrap();
        let mut snapshot = Snapshot::default();
        snapshot.read(&path, 100).unwrap();
        fs::write(&path, b"after!").unwrap();
        assert!(snapshot.verify().unwrap_err().starts_with("FONT_CHANGED"));
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlinks_for_library_font_license_and_ancestors() {
        use std::os::unix::fs::symlink;
        let f = Fixture::new();
        let entry = f.entry(
            "font.ttf",
            REGULAR,
            "Resume One Font Fixture",
            400,
            "normal",
        );
        f.write(json!([entry.clone()]));
        let link = f.root.join("library-link.json");
        symlink(&f.library, &link).unwrap();
        assert!(
            apply(&mut candidate(), &json!([]), &link)
                .unwrap_err()
                .starts_with("FONT_SYMLINK")
        );
        let directory = f.root.join("linked-dir");
        symlink(&f.root, &directory).unwrap();
        assert!(
            apply(
                &mut candidate(),
                &json!([]),
                &directory.join("library.json")
            )
            .unwrap_err()
            .starts_with("FONT_SYMLINK")
        );
        for field in ["path", "licensePath"] {
            let link = f.root.join(format!("linked-{field}.ttf"));
            symlink(entry[field].as_str().unwrap(), &link).unwrap();
            let mut linked = entry.clone();
            linked[field] = json!(link);
            f.write(json!([linked]));
            assert!(
                apply(&mut candidate(), &json!([]), &f.library)
                    .unwrap_err()
                    .starts_with("FONT_SYMLINK")
            );
        }
    }
}
