# Resume One · 简里

跨 macOS 与 Windows 的本地 JSON Resume 工具，目标是用 Rust/GPUI 提供预览与编辑，并提供独立的 `resume-one` CLI 和 `resume-one-theme-migrate` 迁移器。应用不提供账号或云同步；简历和主题均由用户选择的本地目录管理。

## 当前实现状态

仓库正在按功能逐项实现和验收。已完成共享的、**不执行代码**的声明式主题 v1/v2 校验、v1 内存适配与可读性约束下的主题视觉 token 解析，以及 JSON Resume 内容校验、字节级 revision、带 revision 守卫的原子 JSON Patch（含预演）、独立的本地文档 CLI，以及 PDF、可编辑 DOCX、纯文本和 Markdown 导出。仓库现有 [20 套原生声明式主题文件](./assets/themes/)；桌面应用骨架已将它们内嵌，支持左侧悬停试穿和点击选中，并用 JSON Resume 内容生成启动预览。独立的主题迁移 CLI 已支持 v1→v2 升级，以及对第三方 npm/HTML/CSS/JS 主题目录做**低置信度静态提取**；后者只产生配色和包元数据候选，不是视觉等效迁移，必须人工重建布局并验收许可。PDF 已支持主题的单栏排版、纸张/边距、颜色和基础字体层级；未实现的侧栏、特殊构件与字体会明确拒绝，不静默降级。DOCX 仍为基础样式。桌面应用已支持原生打开 JSON 文件和目录、顶栏文件切换器及只读预览；GUI 编辑、持久设置、在线源和 GUI 导出尚未实现，预览也尚未完整实现 v2 排版规范。`prototype-v3/` 只是历史浏览器原型，不是发布产品。请勿把当前源码当成已发布的 macOS/Windows 安装包。

运行已实现的检查：`cargo test --workspace`；从源码使用 CLI：`cargo run -p resume-one-cli -- --help`。macOS 上可通过 `cargo run -p resume-one-app` 打开内置示例预览，或用 `cargo run -p resume-one-app -- /absolute/resume.json` 只读打开指定 JSON Resume 文件。首次构建 GPUI 需要安装 Xcode Metal Toolchain。Windows 运行与安装包尚未验收。简历内容校验采用本地固定版本的 [JSON Resume Schema 1.3.1](./spec/json-resume-1.3.1.schema.json)；主题文件格式见 [声明式主题规范](./resume-one-theme-spec-v2.md)。最终发布须同时提供真实的 macOS/Windows 构建、约 20 套验收主题及公开、已迁移、已验收的默认 GitHub 声明式主题源；缺少任一项不得标记正式发布。

JSON Resume Schema 来自 `@jsonresume/schema@1.3.1`，其原始包声明 MIT 许可证，Copyright © 2024 JSON Resume；原文件 SHA-256 为 `a07eedd3d86ac5bb61d72e136d788269e35baf42c35391e15d0be39b3dc5a4bd`。PDF 内置 Noto Sans SC 和 Noto Serif SC 静态字体按 OFL 1.1 分发。许可全文与随附声明见 [第三方许可](./spec/THIRD_PARTY_LICENSES.md)。

## 文档

- [用户手册](./docs/user-manual.md)
- [CLI 手册](./docs/cli-manual.md)
- [主题格式规范](./resume-one-theme-spec-v2.md)

仓库仅提交实现必需的源码、测试、构建配置，以及 README、正式规范和上述手册；历史调研、设计过程稿与浏览器测试产物不纳入提交。
