# Resume One · 简里

跨 macOS 与 Windows 的本地 JSON Resume 工具，目标是用 Rust/GPUI 提供预览与编辑，并提供独立的 `resume-one` CLI 和 `resume-one-theme-migrate` 迁移器。应用不提供账号或云同步；简历和主题均由用户选择的本地目录管理。

## 当前实现状态

仓库正在按功能逐项实现和验收。首个功能是共享的、**不执行代码**的声明式主题 v1/v2 校验与 v1 内存适配。桌面应用、正式导出、CLI 和迁移器尚未完成；`prototype-v3/` 只是历史浏览器原型，不是发布产品。请勿把当前源码当成已发布的 macOS/Windows 安装包。

运行已实现的检查：`cargo test --workspace`。主题文件格式见 [声明式主题规范](./resume-one-theme-spec-v2.md)。最终发布须同时提供真实的 macOS/Windows 构建、约 20 套验收主题及公开、已迁移、已验收的默认 GitHub 声明式主题源；缺少任一项不得标记正式发布。

## 文档

- [用户手册](./docs/user-manual.md)
- [CLI 手册](./docs/cli-manual.md)
- [主题格式规范](./resume-one-theme-spec-v2.md)

仓库仅提交实现必需的源码、测试、构建配置，以及 README、正式规范和上述手册；历史调研、设计过程稿与浏览器测试产物不纳入提交。
