# Resume One · 简里

**English** · [简体中文](./README.zh-CN.md)

Resume One is a local [JSON Resume](https://jsonresume.org/) editor for macOS and Windows. Edit on the page or work directly in JSON. Your résumé remains a JSON file in the folder you choose. No account is needed. You can use a folder managed by iCloud Drive, OneDrive, or another service to sync across devices.

[Project website](https://sctb512.github.io/resume-one-site/en.html?lang=en) · [Download a preview](https://github.com/sctb512/resume-one/releases) · [Community themes](https://github.com/sctb512/resume-one-themes)

## Getting started

1. Download the ZIP for your platform from the project website, extract it, and open Resume One. Archives are available for macOS Apple silicon, Intel and Universal, and for Windows x64 and ARM64.
2. Try editing the sample, or open your own JSON file or folder. Press `⌘/` on macOS or `Ctrl+/` on Windows to switch between the page and JSON source. The sections panel is available in both views.
3. Hover over a theme to preview it and click to apply it. Use **Appearance** on the right to adjust fonts and colors independently of the résumé and theme files.
4. Export to PDF, editable DOCX, Markdown, or plain text. You can check for updates from within the app.

See the [user guide (Chinese)](./docs/user-manual.md) for details. Downloads are preview releases. macOS packages use ad hoc signing and are not Developer ID signed or notarized.

## Themes and command-line tools

- **Local themes** include 20 declarative layouts. You can choose a single local theme folder or reset it to the default.
- **Online themes** are available from two default sources, Adapted Community Themes and Original JSON Resume. You can also add custom GitHub sources. Original themes render your current résumé live on your device. Downloaded themes work offline. See the [community theme repository](https://github.com/sctb512/resume-one-themes) for catalogs, licenses, and upstream links.
- **Résumé CLI** (`resume-one`) validates files against the schema, makes structured edits, and exports them. It runs independently of the desktop app and works in scripts and agent workflows.
- **Theme migration CLI** (`resume-one-theme-migrate`) provides static inspection, declarative theme upgrades, and community theme adaptation. Adapted themes need a review of their layout and licensing.

```sh
resume-one validate --file "$PWD/resume.json" --json
resume-one export --file "$PWD/resume.json" --format md --out "$PWD/resume.md"
```

The CLI binaries live in `Resume One.app/Contents/MacOS/` on macOS and in the extracted folder on Windows. All three programs share the two complete offline fonts bundled with the app. Keep the entire bundle or extracted folder together. See the [CLI manual (Chinese)](./docs/cli-manual.md) for more commands.

## Building locally

The repository uses Rust 1.99. macOS requires Xcode and the Metal Toolchain. Windows requires a Rust MSVC toolchain, Visual Studio C++ Build Tools, and the Windows SDK. Packaging also requires Python 3.14 with Zstandard support.

**macOS**

```sh
bash packaging/macos/bundle.sh release
bash packaging/macos/archive.sh release
```

**Windows (PowerShell)**

```powershell
.\packaging\windows\bundle.ps1 -Profile release -Archive
```

macOS packages go to `target/release/`. Windows packages go to `target/<Rust target>/release/`. Use `-Architecture x64` or `-Architecture arm64` to select an installed target. The packaging script locates `fxc.exe` in the Windows SDK, or you can set `GPUI_FXC_PATH` explicitly.

Use `cargo run -p resume-one-app` to run from source and `cargo test --locked --workspace` to run the workspace tests.

## Documentation and repositories

The Rust workspace lives in [`crates/`](./crates/). User and CLI manuals are in [`docs/`](./docs/). [`spec/`](./spec/) contains specifications, with [schemas](./spec/schemas/) and [examples](./spec/examples/) in their own folders. Release scripts are in [`packaging/`](./packaging/).

- [Declarative theme specification (Chinese)](./spec/resume-one-theme-spec-v2.md) · [Original theme package specification (Chinese)](./spec/json-resume-original-theme.md) · [JSON Resume Schema 1.3.1](./spec/schemas/json-resume-1.3.1.schema.json)
- This repository holds the desktop app and both CLIs. Pushing a `v*` tag triggers a GitHub Actions build that verifies each platform archive before publishing to [Releases](https://github.com/sctb512/resume-one/releases).
- The [theme repository](https://github.com/sctb512/resume-one-themes) hosts adapted and original themes. The [website repository](https://github.com/sctb512/resume-one-site) maintains the bilingual homepage and downloads.

After publishing, run the release workflow with `verify_update_tag` to test downloading and installing the public archives on all four native architectures.

Resume One is licensed under [Apache-2.0](./LICENSE). See [third-party licenses](./licenses/THIRD_PARTY_LICENSES.md) for fonts and other resources.
