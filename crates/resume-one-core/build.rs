use sha2::{Digest, Sha256};
use std::{env, fs, path::Path};

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    if env::var_os("CARGO_FEATURE_PACKAGED_FONTS").is_none() {
        return;
    }
    for (key, name) in [
        ("SANS", "NotoSansSC-Regular.ttf"),
        ("SERIF", "NotoSerifSC-SemiBold.ttf"),
    ] {
        let path = Path::new("../../assets/fonts").join(name);
        println!("cargo::rerun-if-changed={}", path.display());
        let bytes = fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        println!("cargo::rustc-env=RESUME_ONE_{key}_SIZE={}", bytes.len());
        println!(
            "cargo::rustc-env=RESUME_ONE_{key}_SHA256={:x}",
            Sha256::digest(&bytes)
        );
    }
}
