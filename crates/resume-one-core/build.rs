use sha2::{Digest, Sha256};
use std::{env, fs, path::Path};

fn main() {
    println!("cargo::rustc-check-cfg=cfg(resume_one_packaged_fonts)");
    println!("cargo::rerun-if-env-changed=RESUME_ONE_PACKAGED_FONTS");
    if env::var("RESUME_ONE_PACKAGED_FONTS").as_deref() != Ok("1") {
        return;
    }
    println!("cargo::rustc-cfg=resume_one_packaged_fonts");
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
