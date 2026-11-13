# 简里 · Resume One

[English](./README.md) · **简体中文**

简里是面向 macOS 和 Windows 的本地 [JSON Resume](https://jsonresume.org/) 编辑器。你可以直接在纸面上修改简历，也可以编辑原始 JSON。简历以普通 JSON 文件保存在你选择的目录中，无需账号。跨设备同步可通过 iCloud Drive、OneDrive 等自行管理的目录完成。

[项目主页](https://sctb512.github.io/resume-one-site/) · [下载预览版](https://github.com/sctb512/resume-one/releases) · [社区主题](https://github.com/sctb512/resume-one-themes)

## 开始使用

1. 从项目主页下载对应平台的 ZIP，解压后打开简里。下载包覆盖 macOS Apple 芯片、Intel 和通用版，以及 Windows x64 和 ARM64。
2. 试改示例简历，或打开自己的 JSON 文件或文件夹。按 `⌘/`（macOS）或 `Ctrl+/`（Windows）在纸面预览与 JSON 之间切换。右侧模块栏在两种视图中都可用。
3. 在左侧悬停试穿主题，点击应用。右侧「外观」可以独立调整字体和配色，无需修改简历或主题文件。
4. 导出 PDF、可编辑 DOCX、Markdown 或纯文本。支持从应用内检查更新。

完整操作见[用户手册](./docs/user-manual.md)。发布页的安装包目前为预览版，macOS 包采用临时签名，未做 Developer ID 签名或公证。

## 主题与命令行

- **本地主题**包含 20 套声明式主题。可以指定一个主题目录，也可以恢复默认目录。
- **在线主题**默认提供「迁移版社区主题」和「原版 JSON Resume」两个来源，也可添加自定义 GitHub 来源。原版主题使用当前简历在本机实时渲染。已下载的主题可以离线使用。两类主题的清单、许可和来源见[社区主题仓库](https://github.com/sctb512/resume-one-themes)。
- **简历 CLI** `resume-one` 支持 Schema 校验、结构化修改和导出。它可以独立运行，也可接入脚本和 Agent。
- **主题迁移 CLI** `resume-one-theme-migrate` 提供静态检查、声明式主题升级和社区主题适配。适配结果需要人工核对版式和许可。

```sh
resume-one validate --file "$PWD/resume.json" --json
resume-one export --file "$PWD/resume.json" --format md --out "$PWD/resume.md"
```

macOS 命令行工具位于 `Resume One.app/Contents/MacOS/`，Windows 的工具位于解压目录。三个程序共用包内两套完整的离线字体。请保留完整的应用包或解压目录。更多命令见 [CLI 手册](./docs/cli-manual.md)。

## 本地构建

仓库使用 Rust 1.99。macOS 需要 Xcode 和 Metal Toolchain，Windows 需要 Rust MSVC 工具链、Visual Studio C++ Build Tools 和 Windows SDK。打包还需要带 Zstandard 支持的 Python 3.14。

**macOS**

```sh
bash packaging/macos/bundle.sh release
bash packaging/macos/archive.sh release
```

**Windows（PowerShell）**

```powershell
.\packaging\windows\bundle.ps1 -Profile release -Archive
```

macOS 输出到 `target/release/`。Windows 输出到 `target/<Rust 目标>/release/`，也可通过 `-Architecture x64` 或 `-Architecture arm64` 指定已安装的目标。打包脚本会查找 Windows SDK 的 `fxc.exe`，也可通过 `GPUI_FXC_PATH` 指定。

从源码运行可用 `cargo run -p resume-one-app`。运行工作区测试可用 `cargo test --locked --workspace`。

## 文档与项目仓库

Rust 工作区位于 [`crates/`](./crates/)。用户和 CLI 手册位于 [`docs/`](./docs/)。[`spec/`](./spec/) 存放技术规范，[Schema](./spec/schemas/) 和[规范示例](./spec/examples/)各有独立目录。发布脚本位于 [`packaging/`](./packaging/)。

- [声明式主题规范](./spec/resume-one-theme-spec-v2.md) · [原版主题包规范](./spec/json-resume-original-theme.md) · [JSON Resume Schema 1.3.1](./spec/schemas/json-resume-1.3.1.schema.json)
- 本仓库维护桌面应用和两个 CLI。推送 `v*` 标签后，GitHub Actions 构建和校验各平台 ZIP，再发布到 [Releases](https://github.com/sctb512/resume-one/releases)。
- [社区主题仓库](https://github.com/sctb512/resume-one-themes)维护迁移版和原版主题。[官网仓库](https://github.com/sctb512/resume-one-site)维护双语主页和下载入口。

发布后可以在构建流水线中传入 `verify_update_tag`，从四种原生架构验证公开 ZIP 的下载和安装。

项目采用 [Apache-2.0](./LICENSE)。字体和其他第三方资源的许可见[第三方许可](./licenses/THIRD_PARTY_LICENSES.md)。
