//! Local adaptations still subject to high-fidelity acceptance. Assets and
//! notices are data, never executable themes; registration is not release approval.
use std::{fs, path::Path};

use resume_one_core::export::write_export_unnamed;

pub(super) struct Bundle {
    pub directory: &'static str,
    pub files: &'static [(&'static str, &'static [u8])],
}

include!(concat!(env!("OUT_DIR"), "/community_theme_bundles.rs"));

fn install(root: &Path, bundle: &Bundle) -> Result<(), String> {
    let directory = root.join(bundle.directory);
    match fs::symlink_metadata(&directory) {
        Ok(meta) if !meta.file_type().is_dir() => return Err("同名位置不是普通目录，未安装".into()),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(&directory).map_err(|error| error.to_string())?;
        }
        Err(error) => return Err(error.to_string()),
    }
    // Preflight every existing file before writing anything. A user's edited
    // manifest, font or notice keeps the entire local package untouched.
    for (name, bytes) in bundle.files {
        let path = directory.join(name);
        match fs::symlink_metadata(&path) {
            Ok(meta) => {
                if !meta.file_type().is_file()
                    || meta.len() != bytes.len() as u64
                    || fs::read(&path).map_err(|error| error.to_string())? != *bytes
                {
                    return Err(format!("{name} 已存在且不同，保留本地文件"));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    for (name, bytes) in bundle.files {
        if directory.join(name).exists() {
            continue;
        }
        write_export_unnamed(&directory.join(name), bytes, false).map_err(|error| error.message)?;
    }
    Ok(())
}

pub(super) fn seed(root: &Path) -> Vec<String> {
    BUNDLES
        .iter()
        .filter_map(|bundle| {
            install(root, bundle)
                .err()
                .map(|error| format!("{}：{error}", bundle.directory))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_packages_preserve_six_sample_content_and_print_bounds() {
        let root = tempfile::tempdir().unwrap();
        assert!(seed(root.path()).is_empty());
        assert_eq!(BUNDLES.len(), 67, "pinned community inventory");
        let root = root.path();
        std::thread::scope(|scope| {
            for group in BUNDLES.chunks(BUNDLES.len().div_ceil(4)) {
                scope.spawn(move || {
                    for bundle in group {
                        check_samples(root, bundle);
                    }
                });
            }
        });
    }

    fn check_samples(root: &Path, bundle: &Bundle) {
        use resume_one_core::{
            layout::{self, LayoutElement, PT_PER_MM},
            render::Language,
            resume::parse_resume,
            semantic::project,
        };
        let theme =
            resume_one_core::load_theme(root, &Path::new(bundle.directory).join("theme.json"))
                .unwrap();
        let mut samples = vec![
            serde_json::json!({}),
            serde_json::json!({"basics":{"name":"Alex Chen"}}),
            serde_json::json!({"basics":{"name":"陈晓","summary":"中文短简历"}}),
        ];
        samples.push(
            serde_json::from_slice(include_bytes!("../../../assets/example.resume.json")).unwrap(),
        );
        for sentence in [
            "Reliable engineering and inclusive design. ",
            "可靠工程与清晰设计，保留中文和 English。",
        ] {
            samples.push(serde_json::json!({"basics":{"name":"Alex Chen 陈晓", "email":"alex@example.com", "summary": sentence.repeat(80)}, "work":[{"name":"MainStart", "summary":sentence.repeat(150)},{"name":"MainTail"}], "skills":[{"name":"SideStart","keywords":[sentence.repeat(70)]},{"name":"SideTail"}]}));
        }
        for (index, value) in samples.into_iter().enumerate() {
            let source = serde_json::to_vec(&value).unwrap();
            let document = parse_resume(&source).unwrap();
            let content = project(&document);
            let result = layout::layout_with_theme(&content, Language::En, &theme).unwrap();
            let mut all = String::new();
            for text in result.pages.iter().flatten().filter_map(|op| match op {
                LayoutElement::Text(t) => Some(t),
                _ => None,
            }) {
                assert!(
                    text.x_pt >= -0.02 && text.baseline_y_pt >= -0.02,
                    "{} sample {index}: {:?}",
                    bundle.directory,
                    text.text
                );
                assert!(
                    text.x_pt + text.width_pt() <= result.width_mm * PT_PER_MM + 0.05,
                    "{} sample {index}: {:?}",
                    bundle.directory,
                    text.text
                );
                assert!(
                    text.baseline_y_pt + text.font.vertical_metrics().0 * text.size_pt
                        <= result.height_mm * PT_PER_MM + 0.05,
                    "{} sample {index}: {:?}",
                    bundle.directory,
                    text.text
                );
                if text.target.is_some() {
                    all.push_str(&text.text);
                }
            }
            if index >= 4 {
                assert!(result.pages.len() > 1);
                let rendered = all.to_ascii_lowercase();
                for tail in ["maintail", "sidetail"] {
                    assert!(rendered.contains(tail), "{}: {tail}", bundle.directory);
                }
            }
            assert_eq!(document.value, value);
            assert_eq!(source, serde_json::to_vec(&document.value).unwrap());
        }
    }

    #[test]
    fn reviewed_bundles_install_offline_with_real_fonts_and_all_notices() {
        let root = tempfile::tempdir().unwrap();
        assert!(seed(root.path()).is_empty());
        assert!(seed(root.path()).is_empty());
        for bundle in BUNDLES {
            assert_eq!(bundle.files.last().unwrap().0, "theme.json");
            let theme = resume_one_core::load_theme(
                root.path(),
                &Path::new(bundle.directory).join("theme.json"),
            )
            .unwrap();
            resume_one_core::layout::check_theme(&theme).unwrap();
            assert_eq!(theme.normalized["provenance"]["fidelity"], "adapted");
            assert!(
                theme.normalized["provenance"]["upstreamUrl"]
                    .as_str()
                    .unwrap()
                    .starts_with("https://")
            );
            for (name, bytes) in bundle.files {
                assert_eq!(
                    fs::read(root.path().join(bundle.directory).join(name)).unwrap(),
                    *bytes
                );
            }
        }
    }

    #[test]
    fn edited_package_is_preserved_without_filling_in_new_files() {
        let root = tempfile::tempdir().unwrap();
        let bundle = &BUNDLES[0];
        let directory = root.path().join(bundle.directory);
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("theme.json"), b"user edited manifest").unwrap();
        assert_eq!(seed(root.path()).len(), 1);
        assert_eq!(
            fs::read(directory.join("theme.json")).unwrap(),
            b"user edited manifest"
        );
        assert!(
            bundle
                .files
                .iter()
                .filter(|(name, _)| *name != "theme.json")
                .all(|(name, _)| !directory.join(name).exists())
        );
    }

    #[cfg(unix)]
    #[test]
    fn package_directory_and_assets_never_follow_symlinks() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join(BUNDLES[0].directory)).unwrap();
        assert_eq!(seed(root.path()).len(), 1);
        assert!(fs::read_dir(outside.path()).unwrap().next().is_none());
        let other = tempfile::tempdir().unwrap();
        let directory = other.path().join(BUNDLES[0].directory);
        fs::create_dir(&directory).unwrap();
        let target = outside.path().join("font.ttf");
        fs::write(&target, b"keep").unwrap();
        let asset = bundle_asset_name();
        std::os::unix::fs::symlink(&target, directory.join(asset)).unwrap();
        assert_eq!(seed(other.path()).len(), 1);
        assert_eq!(fs::read(&target).unwrap(), b"keep");
        assert!(!directory.join("theme.json").exists());
    }

    #[cfg(unix)]
    fn bundle_asset_name() -> &'static str {
        BUNDLES[0]
            .files
            .iter()
            .find(|(name, _)| *name != "theme.json")
            .unwrap()
            .0
    }
}
