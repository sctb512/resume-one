# Resume One · 简里

跨 macOS 与 Windows 的本地 JSON Resume 工具，目标是用 Rust/GPUI 提供预览与编辑，并提供独立的 `resume-one` CLI 和 `resume-one-theme-migrate` 迁移器。应用不提供账号或云同步；简历和主题均由用户选择的本地目录管理。

## 当前实现状态

CLI 的 `set-field` 已支持标准文本字段修改、清空、Schema 校验、revision 守卫与预演，结果不回显简历正文；详见 [CLI 手册](./docs/cli-manual.md)。

桌面预览与 PDF 已共用字体测量、换行、分页和逐页坐标；预览使用同一套内置 Noto 字体绘制正文，并由排版元数据生成编辑命中区域。分页不会改变右侧模块栏的位置，新增空条目会直接展开表单。当前支持单栏基础版式，尚不代表完整 v2 主题兼容；屏幕与 PDF 的抗锯齿也不保证逐像素一致。

预览插入已支持双向聚焦：先选纸张插入点会突出右栏可添加的模块；先选模块会突出对应插入点，其他内容淡出。Esc 或再次点击已选模块可取消，不修改简历；切换源码也会清除待插入状态。

声明式主题的 `sections` 已支持四种模块标题 `plain/line/accent-line/filled`、两种条目间距 `stack/compact` 和 `inline` 日期，预览、PDF、可编辑 DOCX 均可使用；紧凑模式不缩小文字或省略字段。本地/在线主题走同一能力校验，未实现的时间线、卡片、标签和左右日期仍报错。配置示例与精确语义见主题规范。

在线源共享底层与 CLI `themes` 命令已实现：公开 GitHub ref 固定到 commit、源索引校验、条件请求、哈希与身份检查、256 MiB 有界缓存、显式离线回退，以及原字节、不覆盖的主题下载。该底层不接收简历数据。GUI 可管理单一本地目录及多个 GitHub 来源，通过“本地 / 在线”切换列表、悬停用当前简历试穿、点击应用、右键下载到本地。在线列表按滚动位置预取附近 20 项，最多 3 个后台任务；来源切换和过期响应受身份守卫，诊断集中在管理页。没有虚构默认在线仓库。

仓库正在按功能逐项实现和验收。已完成共享的、**不执行代码**的声明式主题 v1/v2 校验、v1 内存适配与可读性约束下的主题视觉 token 解析，以及 JSON Resume 内容校验、字节级 revision、带 revision 守卫的原子 JSON Patch（含预演）、标准模块/条目的 Schema 安全增删、独立的本地文档 CLI，以及 PDF、可编辑 DOCX、纯文本和 Markdown 导出。仓库现有 [20 套原生声明式主题文件](./assets/themes/)；桌面应用首次运行会将其安全安装到默认本地主题目录，支持更换唯一的本地目录、恢复默认目录、右键定位实际文件，以及左侧悬停试穿和点击选中。原生 JSON 编辑器已支持快捷键切换、实时更新有效预览、Schema 错误提示、格式化与撤销、带版本冲突保护的保存和私有恢复草稿；切换时主题栏连续收放，中央视图交接，而右侧模块栏始终固定。预览页和 JSON 视图右栏均已可增删标准模块与条目，并与 JSON 草稿和撤销链路同步。独立的主题迁移 CLI 已支持 v1→v2 升级，以及对第三方 npm/HTML/CSS/JS 主题目录做**低置信度静态提取**；后者只产生配色和包元数据候选，不是视觉等效迁移，必须人工重建布局并验收许可。

PDF / DOCX 已支持主题的单栏排版、纸张/边距、颜色和基础字体层级；未实现的侧栏、特殊构件与字体会明确拒绝，不静默降级。DOCX 使用可编辑段落、完整嵌入的 Noto 字体和可打印的矢量纸张背景，约 18 MB；CLI `export --theme` 同样支持 DOCX。Word 自行重排，字号/字重受其原生格式约束，不承诺与 PDF 逐像素一致；Microsoft Word 跨平台实机验收仍未完成。桌面应用已支持原生打开 JSON 文件和目录、顶栏文件切换器，以及文件菜单中的四种导出；预览已支持标准文本字段的悬停/点击表单及成组撤销；JSON 代码行旁已提供标准模块与条目的结构控件，支持双向选择插入位置、同一视觉行的命名操作菜单、独立撤销，以及 Schema/输入法/旧动作保护；预览尚未完整实现 v2 排版规范。`prototype-v3/` 只是历史浏览器原型，不是发布产品。请勿把当前源码当成已发布的 macOS/Windows 安装包。

桌面主工作区按 v3 设计还原了朱印品牌、衬线标题、细线模块列表、本机生成的主题缩略图和四格式导出菜单；JSON 视图无标题条或格式化按钮，宽表单采用双列。纸张等比适配窗口且不重新流版，主题栏在 360ms 内真实挤出/拉入，顶栏与右模块栏固定，末段短交接；主题试穿有 160ms 停留判定。可测试的尺寸、动效和当前窄窗边界见[桌面视觉与空间契约](./spec/desktop-appearance.md)。

单栏页眉已支持 `composition.header: stacked/centered`，联系方式支持 `composition.contact: inline/stacked`，可独立组合；居中按实际内容区逐行计算，长文本自动换行，摘要与正文仍左对齐。预览、PDF、可编辑 DOCX 及本地/在线主题能力检查保持一致；分割页眉和图标联系方式仍明确拒绝。配置只属于主题，不写入简历 JSON。

运行已实现的检查：`cargo test --workspace`；从源码使用 CLI：`cargo run -p resume-one-cli -- --help`。macOS 上可通过 `cargo run -p resume-one-app` 打开内置示例，或用 `cargo run -p resume-one-app -- /absolute/resume.json` 打开 JSON Resume 文件；有语法或 Schema 错误的 UTF-8 文件会直接进入源码修复界面。按 `⌘/` 切换预览和源码。首次构建 GPUI 需要安装 Xcode Metal Toolchain。Windows 运行与安装包尚未验收。简历内容校验采用本地固定版本的 [JSON Resume Schema 1.3.1](./spec/json-resume-1.3.1.schema.json)；主题文件格式见 [声明式主题规范](./resume-one-theme-spec-v2.md)。最终发布须同时提供真实的 macOS/Windows 构建、约 20 套验收主题及公开、已迁移、已验收的默认 GitHub 声明式主题源；缺少任一项不得标记正式发布。

JSON Resume Schema 来自 `@jsonresume/schema@1.3.1`，其原始包声明 MIT 许可证，Copyright © 2024 JSON Resume；原文件 SHA-256 为 `a07eedd3d86ac5bb61d72e136d788269e35baf42c35391e15d0be39b3dc5a4bd`。PDF / DOCX 内置 Noto Sans SC 和 Noto Serif SC 静态字体按 OFL 1.1 分发。许可全文与随附声明见 [第三方许可](./spec/THIRD_PARTY_LICENSES.md)。

## 文档

- 共享编辑内核已支持无效输入保留、最近有效预览、无损格式化、带 revision 守卫的全文保存及独立的本地恢复草稿存储，接口契约见 [编辑缓冲区规范](./spec/editor-buffer.md)；原生编辑器已接入这些接口。

- [用户手册](./docs/user-manual.md)
- [CLI 手册](./docs/cli-manual.md)
- [主题格式规范](./resume-one-theme-spec-v2.md)

仓库仅提交实现必需的源码、测试、构建配置，以及 README、正式规范和上述手册；历史调研、设计过程稿与浏览器测试产物不纳入提交。
