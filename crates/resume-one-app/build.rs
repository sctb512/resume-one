//! Embed only data from the version-controlled, locally accepted theme set.
//! Theme source programs and runtime directory discovery are never involved.
use std::{env, fs, path::Path};

fn portable_name(value: &str) -> bool {
    !value.starts_with('.')
        && !value.is_empty()
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
}

fn main() {
    let root = Path::new(&env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../../assets/community-themes");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut packages = fs::read_dir(&root)
        .expect("community theme data directory")
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    packages.sort();
    let mut generated = String::from("pub(super) const BUNDLES: &[Bundle] = &[\n");
    let mut count = 0;
    for package in packages {
        let meta = fs::symlink_metadata(&package).unwrap();
        assert!(
            !meta.file_type().is_symlink(),
            "theme data must not be a symlink"
        );
        if !meta.is_dir() {
            continue;
        }
        let directory = package
            .file_name()
            .unwrap()
            .to_str()
            .expect("UTF-8 theme directory");
        assert!(
            portable_name(directory) && directory.starts_with("community-"),
            "nonportable community theme directory"
        );
        let mut files = fs::read_dir(&package)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert!(
            files.len() <= 32,
            "community package has too many data files"
        );
        assert!(
            files.iter().any(|p| p.file_name().unwrap() == "theme.json"),
            "community package requires theme.json"
        );
        // Manifest last also governs first-run installation: a partial copy
        // cannot advertise a theme before its fonts and notices are present.
        files.sort_by_key(|p| (p.file_name().unwrap() == "theme.json", p.clone()));
        generated.push_str(&format!("Bundle {{ directory: {directory:?}, files: &[\n"));
        let mut total = 0u64;
        for file in files {
            let meta = fs::symlink_metadata(&file).unwrap();
            assert!(
                meta.file_type().is_file(),
                "theme assets must be ordinary files, not symlinks or directories"
            );
            total += meta.len();
            assert!(
                meta.len() <= 16 * 1024 * 1024 && total <= 64 * 1024 * 1024,
                "community theme data size limit"
            );
            let name = file
                .file_name()
                .unwrap()
                .to_str()
                .expect("UTF-8 data filename");
            let extension = file
                .extension()
                .and_then(|v| v.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            assert!(
                portable_name(name)
                    && matches!(extension.as_str(), "json" | "ttf" | "otf" | "txt" | "md"),
                "executable or unsupported theme asset"
            );
            let absolute = file.canonicalize().unwrap();
            generated.push_str(&format!(
                "({name:?}, include_bytes!({:?}).as_slice()),\n",
                absolute.to_str().unwrap()
            ));
        }
        generated.push_str("] },\n");
        count += 1;
    }
    assert!(
        count > 0 && count <= 256,
        "community theme count must be bounded and nonzero"
    );
    generated.push_str("];\n");
    fs::write(
        Path::new(&env::var_os("OUT_DIR").unwrap()).join("community_theme_bundles.rs"),
        generated,
    )
    .unwrap();
}
