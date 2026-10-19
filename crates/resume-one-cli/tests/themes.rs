use resume_one_themes::{
    GithubClient, GithubSource, HttpResponse, ThemeCache, ThemeError, Transport, sha256,
};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    path::Path,
    process::{Command, Output},
    sync::Mutex,
};

const COMMIT: &str = "0123456789012345678901234567890123456789";

struct Mock(Mutex<VecDeque<Vec<u8>>>);
impl Transport for Mock {
    fn get(&self, _: &str, _: Option<&str>, _: usize) -> Result<HttpResponse, ThemeError> {
        Ok(HttpResponse {
            status: 200,
            etag: None,
            bytes: self
                .0
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected request"),
        })
    }
}

fn call(cache: &Path, args: &[&str]) -> (Output, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_resume-one"))
        .args([
            "--json",
            "themes",
            "--repository",
            "example/themes",
            "--ref",
            COMMIT,
            "--cache",
            cache.to_str().unwrap(),
            "--offline",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(output.stderr.is_empty());
    assert_eq!(String::from_utf8_lossy(&output.stdout).lines().count(), 1);
    let value = serde_json::from_slice(&output.stdout).unwrap();
    (output, value)
}

#[test]
fn offline_catalog_and_download_do_not_read_or_modify_a_resume() {
    let root = tempfile::tempdir().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let private_file = directory.path().join("resume.json");
    std::fs::write(&private_file, b"private resume content").unwrap();
    let bytes = include_bytes!("../../../assets/themes/zhibai.theme.json");
    let theme = resume_one_core::parse_theme(bytes).unwrap();
    let index = serde_json::to_vec(
        &json!({"version":1,"themes":[{"id":theme.id,"name":theme.name,
        "manifest":"zhibai.theme.json","sha256":sha256(bytes)}]}),
    )
    .unwrap();
    let mock = Mock(Mutex::new(VecDeque::from([index, bytes.to_vec()])));
    let source = GithubSource::new("example/themes", COMMIT, "themes/index.json").unwrap();
    let client = GithubClient::with_transport(ThemeCache::new(root.path()).unwrap(), mock);
    let catalog = client.catalog(&source, false).unwrap().catalog;
    client.theme(&catalog, "zhibai", false).unwrap();
    let (output, listing) = call(root.path(), &["list"]);
    assert!(output.status.success());
    assert_eq!(listing["result"]["themes"][0]["id"], "zhibai");
    assert_eq!(listing["result"]["stale"], true);
    assert_eq!(listing["result"]["manifestsValidated"], false);
    let (output, downloaded) = call(
        root.path(),
        &[
            "download",
            "zhibai",
            "--dir",
            directory.path().to_str().unwrap(),
        ],
    );
    assert!(output.status.success());
    assert_eq!(downloaded["result"]["verified"], true);
    let path = downloaded["result"]["file"].as_str().unwrap();
    assert_eq!(std::fs::read(path).unwrap(), bytes);
    std::fs::write(path, b"user changes").unwrap();
    let (output, error) = call(
        root.path(),
        &[
            "download",
            "zhibai",
            "--dir",
            directory.path().to_str().unwrap(),
        ],
    );
    assert_eq!(output.status.code(), Some(7));
    assert_eq!(error["error"]["code"], "FILE_EXISTS");
    assert_eq!(std::fs::read(path).unwrap(), b"user changes");
    assert_eq!(
        std::fs::read(private_file).unwrap(),
        b"private resume content"
    );
    assert!(!downloaded.to_string().contains("private resume content"));
}

#[test]
fn offline_misses_and_invalid_paths_are_machine_readable() {
    let root = tempfile::tempdir().unwrap();
    let (output, error) = call(root.path(), &["list"]);
    assert_eq!(output.status.code(), Some(7));
    assert_eq!(error["error"]["code"], "SOURCE_NOT_CACHED");
    let (output, error) = call(Path::new("relative-cache"), &["list"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(error["error"]["code"], "PATH_NOT_ABSOLUTE");
}

#[test]
fn release_audit_command_is_offline_and_does_not_accept_native_themes_as_migrations() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("themes")).unwrap();
    let bytes = include_bytes!("../../../assets/themes/zhibai.theme.json");
    std::fs::write(root.path().join("themes/zhibai.theme.json"), bytes).unwrap();
    std::fs::write(
        root.path().join("themes/index.json"),
        serde_json::to_vec(&json!({"version":1,"themes":[{
            "id":"zhibai","name":"知白","manifest":"zhibai.theme.json","sha256":sha256(bytes)
        }]}))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        root.path().join("themes/review.json"),
        serde_json::to_vec(&json!({"version":1,"themes":[{
            "id":"zhibai","sha256":sha256(bytes),"upstreamUrl":"https://github.com/example/theme",
            "upstreamRevision":"abc123","upstreamLicense":"MIT","licenseReviewedBy":"Tester",
            "visualReviewedBy":"Tester","reviewDate":"2026-10-19","visualSamples":["zh-short",
            "zh-long","en-short","en-long","missing-fields","multi-page"],
            "pdfReviewed":true,"limitations":"Fixture only"
        }]}))
        .unwrap(),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_resume-one"))
        .args([
            "--json",
            "verify-theme-source",
            "--dir",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(7));
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["error"]["code"], "RELEASE_NOT_MIGRATED");
}
