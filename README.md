# Resume One · 简里

跨 macOS 与 Windows 的本地 JSON Resume 工具，目标是用 Rust/GPUI 提供预览与编辑，并提供独立的 `resume-one` CLI 和 `resume-one-theme-migrate` 迁移器。应用不提供账号或云同步；简历和主题均由用户选择的本地目录管理。

[项目主页](https://sctb512.github.io/resume-one-site/) · [应用下载](https://github.com/sctb512/resume-one/releases) · [社区主题](https://github.com/sctb512/resume-one-themes)

## 当前实现状态

67 套社区主题的声明式适配由独立主题仓库维护。工具从固定来源生成主题，保留来源、许可和逐包差异记录。工具组合打印页边距与正文内距，按实际字体行框定位，并从有明确源码绑定的条目保留字段顺序、分行和静态组件样式。静态字体使用独立内部字族，避免 DOCX 多字重嵌入冲突。离线加载、样本排版和导出是本地验收范围。公开默认源仍须通过[高保真验收门槛](./resume-one-theme-spec-v2.md#高保真验收门槛)。

左右侧栏已接入预览、PDF 和可编辑 DOCX：页眉/摘要默认全宽，也可独立放入主栏或侧栏；联系方式可跟随身份或单独入栏，主栏与侧栏分别换行和延续到下一页；窄栏长标题换行，页脚不重复，页内增删/悬停命中跟随所属栏位。侧栏底色独立检查文字对比度。PDF 与 DOCX 已用本地渲染图片检查，Word 的实际分页和底色高度仍须独立验收；共享布局能力不等于原版主题视觉一致。

新增独立的本机外观设置：点击右侧模块栏的“外观”，在固定右栏内即时调整正文/标题字体和配色，文档始终可见，不使用遮罩或模态窗口；“完成”回到模块列表。字体、颜色可分别恢复主题默认。预览、缩略图与 PDF/DOCX 使用同一有效主题快照，原始主题和简历 JSON 均不改写。CLI 通过 `export --theme ... --appearance ...` 显式复用外观设置，不隐式读取桌面配置。在线列表只保留仓库选择器，不重复显示“管理来源”。数据格式和验收边界见[独立外观设置](./spec/appearance-settings.md)。

外观栏的字体列表支持全部原生系统字族、搜索、键盘选择及导入 TTF/OTF/TTC/OTC 到简里本机库；不向系统目录或注册表安装。字体整行、常用色与色盘支持悬停试用：页面和软件配色实时联动，移开恢复，点击才确认；系统字体后台读取有界，过期结果不会跳回旧候选。悬停不写设置、简历或主题，也不影响导出。颜色另可通过屏幕取色选择，精确值按需展开；拖动期间只更新预览，松手保存，Esc 撤回本次拖动。预览与导出读取同一字面数据，缺少中文时显式回退；无法嵌入的字体明确拒绝导出。CLI 另有 `fonts list` / `fonts install`。Windows 原生取色和 Microsoft Word 行为仍须独立验收。

macOS 本机已检查字体搜索、选择、原生安装及取消、色盘联动、右栏固定和页内表单；系统取色界面已确认出现，选中/取消/失败回调通过自动化契约测试。系统取色的真实桌面点击与取消手势尚未完成自动化端到端验收，不能把回调模拟当作该项通过，也不能据此宣称正式发布验收完成。

本地声明式主题现可携带经路径、SHA-256、格式及字重校验的静态 TTF/OTF：同一真实字体用于预览、PDF 和 DOCX，不安装到字体库，不写回简历 JSON。macOS 原生字体引擎已独立验证加载与字形测量；界面模拟测试不冒充原生验收。详情见[主题字体资源契约](./resume-one-theme-spec-v2.md#3-字段语义)。在线字体包传输仍未实现。独立迁移工具的 `--adapt --batch` 会静态提取排版、配色和模块顺序，生成可追溯差异报告，不执行上游代码。详见[近似适配协议](./spec/theme-adaptation-v1.md)。

应用包含文档 CLI、独立主题迁移器、20 套原生主题、主题规范和两份手册。发布构建另从固定主题提交装入 67 套社区适配主题。macOS 打包后，命令行工具位于 `.app/Contents/MacOS/`，规范、手册、主题文件和示例位于 `.app/Contents/Resources/`。macOS 预发布包采用临时签名。

社区适配包与本地主题同格式、同交互。主题 JSON 最后落盘，已有本地修改与符号链接受到保护。右键“显示主题文件”可定位包。每个包的 `NOTICE.txt`、`LICENSE.txt` 和 `MIGRATION.json` 记录出处、许可和差异。Rick Osborne 保留原始项目链接与 CC BY-NC-SA 4.0 正文。仓库提供完整离线 ZIP，以及适用于单文件在线协议的主题索引。[社区主题仓库](https://github.com/sctb512/resume-one-themes)

本轮交互精修：360ms 视图切换从 62% 时程开始内容交接，右栏固定；外壳配色以 160ms 过渡；纸张无模块的空白处不自动展开表单，字段区提供常驻溢出滚动条。草稿恢复使用中央提示，选择前保留原文件与草稿并暂时锁定修改。macOS 窗口按钮与工具栏合并为一行。完整示例仍是干净的 JSON Resume，不含样式或应用元数据。

预览悬停使用可逆透明度：首次停留 140ms、条目切换 85ms；表单纯淡入 160ms，鼠标一旦移出编辑框本身立即开始 120ms 淡出，即使仍在纸张内或输入框有焦点。快速回返从当前透明度继续，不平移、不重播；关闭前保留当前可见输入（包括输入法组合文字），不自动保存原文件。同条目内的文字、行间与留白共用一次停留计时；模块区域之外的纸张空白不展开表单；滚动取消候选，收起后避免被原触发区立刻重开。表单采用主题派生的纸色、细线、轻阴影与语义字段分组；独立页级浮层以 90ms 交接新旧表单，最多保留一个不可操作的淡出旧层。纸张及右栏不重排。时间连续性和实际鼠标事件路径纳入测试，不以静态截图代替真实帧率测量。

保存采用非阻塞旁路锁：另一个桌面窗口或 CLI 持锁时立即报告 `RESUME_BUSY`，保留修改和恢复草稿，避免锁竞争导致永久“保存中”。带简历路径启动及恢复上次目录范围现改为窗口创建后后台读取；等待时可切换视图，点击“取消”或按 Esc 放弃结果，右栏不移动，迟到结果不会覆盖当前文档。最多保留两个未结束读取，避免重复取消产生无限后台任务。这不等于中断系统调用或增加磁盘 I/O 超时；实机已复现文件打开调用等待，但系统层原因仍未确定，不能归因于权限弹窗或锁竞争。本项不涵盖启动时主题/设置目录的读取。

桌面已补齐三档响应布局：宽窗双侧栏、中窗主题抽屉、窄窗互斥双抽屉，纸张不因抽屉开合重排。支持系统减少动态效果、表单短入场、主题悬停/应用区分、只读旧预览提示及顶栏键盘入口。Windows 的系统设置联动仍需实机验收；GPUI 屏幕阅读器语义覆盖尚不完整。

CLI 的 `set-field` 已支持标准文本字段修改、清空、Schema 校验、revision 守卫与预演，结果不回显简历正文；详见 [CLI 手册](./docs/cli-manual.md)。

桌面预览与 PDF 已共用字体测量、换行、分页和逐页坐标；预览使用同一套内置 Noto 字体绘制正文，并由排版元数据生成编辑命中区域。分页不会改变右侧模块栏的位置，新增空条目会直接展开表单。当前支持单栏和左右侧栏基础版式，尚不代表完整 v2 主题兼容；屏幕与 PDF 的抗锯齿也不保证逐像素一致。

预览插入已支持双向聚焦：先选纸张插入点会突出右栏可添加的模块；先选模块会突出对应插入点，其他内容淡出。Esc 或再次点击已选模块可取消，不修改简历；切换源码也会清除待插入状态。

声明式主题的 `sections` 已支持四种模块标题 `plain/line/accent-line/filled`、两种条目间距 `stack/compact` 和 `inline/left/right` 日期，预览、PDF、可编辑 DOCX 均可使用；紧凑模式不缩小文字或省略字段。本地/在线主题走同一能力校验，现支持有限的单侧时间线、可分页卡片和关键词标签；自定义装饰等未实现配置仍报错。配置示例与精确语义见主题规范。

在线源与 CLI `themes` 命令共用 GitHub commit 固定、索引及哈希校验、256 MiB 有界缓存和离线回退。桌面预发布版初始配置为私有主题仓库 `sctb512/resume-one-themes` 的 `main/index.json`，已保存的在线源列表优先。启动前将有权读取该仓库的令牌提供给 `RESUME_ONE_THEME_GITHUB_TOKEN`。它只用于该仓库的 GitHub API 请求，其他公开主题源仍使用匿名请求。使用方式见[用户手册](./docs/user-manual.md)。

仓库正在按功能逐项实现和验收。已完成共享的、**不执行代码**的声明式主题 v1/v2 校验、v1 内存适配与可读性约束下的主题视觉 token 解析，以及 JSON Resume 内容校验、字节级 revision、带 revision 守卫的原子 JSON Patch（含预演）、标准模块/条目的 Schema 安全增删、独立的本地文档 CLI，以及 PDF、可编辑 DOCX、纯文本和 Markdown 导出。仓库现有 [20 套原生声明式主题文件](./assets/themes/)；桌面应用首次运行会将其安全安装到默认本地主题目录，后续仅对字节哈希与旧内置版完全一致的文件自动升级，用户改动不覆盖。支持更换唯一的本地目录、恢复默认目录、右键定位实际文件，以及左侧悬停试穿和点击选中。原生 JSON 编辑器已支持快捷键切换、实时更新有效预览、Schema 错误提示、格式化与撤销、带版本冲突保护的保存和私有恢复草稿；切换时主题栏连续收放，中央视图交接，而右侧模块栏始终固定。预览页和 JSON 视图右栏均已可增删标准模块与条目，并与 JSON 草稿和撤销链路同步。独立的主题迁移 CLI 已支持 v1→v2 升级，以及对第三方 npm/HTML/CSS/JS 主题目录做**低置信度静态提取**；后者只把十六进制或不透明整数 RGB 配色及包元数据写入候选，并在报告中列出有界的字体声明与布局线索，不是视觉等效迁移，必须人工重建布局并验收许可。

20 套内置主题已按不同排版骨架精修，而非仅换配色：页眉对齐、联系方式、标题装饰、左右日期、字号层级和纸面密度均有组合差异。「墨序」和「丹朱」进一步采用两种联系方式节奏不同的分栏页眉；未修改的旧内置文件会按精确哈希安全升级，用户改动保留。逐套使用同一份中英混排样例生成 PDF 对照检查；自动测试要求非颜色结构互不重复、文本不越过内容区、尾部内容不丢失。当前渲染器已支持单栏、左右侧栏及有限的时间线、卡片和标签；新增构件不等于社区主题已经完成逐套迁移，也不代表 Word 实机验收完成。

PDF / DOCX 已支持主题的单栏排版、纸张/边距、颜色和基础字体层级；未实现的特殊构件与字体组合会明确拒绝，不静默降级。DOCX 使用可编辑段落、完整嵌入的 Noto 字体和可打印的矢量纸张背景，约 18 MB；CLI `export --theme` 同样支持 DOCX。Word 自行重排，字号/字重受其原生格式约束，不承诺与 PDF 逐像素一致；Microsoft Word 跨平台实机验收仍未完成。桌面应用已支持原生打开 JSON 文件和目录、顶栏文件切换器，以及文件菜单中的四种导出；预览已支持标准文本字段的悬停/点击表单及成组撤销；JSON 代码行旁已提供标准模块与条目的结构控件，支持双向选择插入位置、同一视觉行的命名操作菜单、独立撤销，以及 Schema/输入法/旧动作保护；预览尚未完整实现 v2 排版规范。`prototype-v3/` 只是历史浏览器原型，不是发布产品。请勿把当前源码当成已发布的 macOS/Windows 安装包。

桌面主工作区按 v3 设计还原了朱印品牌、衬线标题、细线模块列表、本机生成的主题缩略图和四格式导出菜单；JSON 视图无标题条或格式化按钮，宽表单采用双列。纸张等比适配窗口且不重新流版，主题栏在 360ms 内真实挤出/拉入，顶栏与右模块栏固定，末段短交接；主题试穿有 160ms 停留判定。可测试的尺寸、动效和当前窄窗边界见[桌面视觉与空间契约](./spec/desktop-appearance.md)。

单栏正文的页眉已支持 `composition.header: stacked/centered/split`，联系方式支持 `composition.contact: inline/stacked`；`split` 或 `preset: split-header` 将姓名/职业与联系方式分列排版，正文仍为单栏。居中按实际内容区逐行计算，长文本自动换行；分栏页眉遇到超长内容会退回全宽流式页眉，避免越界。预览、PDF、可编辑 DOCX 及本地/在线主题能力检查保持一致；图标联系方式仍明确拒绝。配置只属于主题，不写入简历 JSON。

运行已实现的检查：`cargo test --workspace`；从源码使用 CLI：`cargo run -p resume-one-cli -- --help`。macOS 上可通过 `cargo run -p resume-one-app` 打开内置示例，或用 `cargo run -p resume-one-app -- /absolute/resume.json` 打开 JSON Resume 文件；有语法或 Schema 错误的 UTF-8 文件会直接进入源码修复界面。按 `⌘/` 切换预览和源码。macOS 开发包 `bash packaging/macos/bundle.sh debug` 会同时装入桌面程序、CLI 与独立迁移工具；装配后做本机临时签名及严格资源封签校验，但没有 Developer ID 签名或公证，不代表正式安装包。首次构建 GPUI 需要安装 Xcode Metal Toolchain。Windows 运行与安装包尚未验收。简历内容校验采用本地固定版本的 [JSON Resume Schema 1.3.1](./spec/json-resume-1.3.1.schema.json)；主题文件格式见 [声明式主题规范](./resume-one-theme-spec-v2.md)。最终发布须同时提供真实的 macOS/Windows 构建、约 20 套验收主题及公开、已迁移、已验收的默认 GitHub 声明式主题源；缺少任一项不得标记正式发布。

## 本地构建

仓库固定使用 Rust 1.99。macOS 需要 Xcode 与 Metal Toolchain；在仓库根目录运行 `rustup toolchain install 1.99.0`，然后执行 `bash packaging/macos/bundle.sh release`。输出为 `target/release/Resume One.app`，其中 `Contents/MacOS/` 包含桌面程序、`resume-one` 和 `resume-one-theme-migrate`。脚本对装配后的程序做临时签名，`codesign --verify --deep --strict` 必须通过；可以用 `open 'target/release/Resume One.app'` 在本机启动。需要单文件本地评估包时，再运行 `bash packaging/macos/archive.sh release`，得到 `target/release/Resume-One-macOS-unsigned.zip`；归档脚本会在解包后再次验证封签，已有目标不会被覆盖。该包没有 Developer ID 签名或公证，Gatekeeper 会拒绝把它当公开分发包。

Windows 请在**原生 Windows** 上安装 Rust 1.99 MSVC 工具链、Visual Studio C++ Build Tools 和 Windows SDK，在仓库根目录的 PowerShell 运行 `rustup toolchain install 1.99.0-x86_64-pc-windows-msvc`，然后运行 `cargo build --locked --release -p resume-one-app -p resume-one-cli -p resume-one-theme-migrate`。预期三个二进制分别为 `target\release\resume-one-app.exe`、`target\release\resume-one.exe` 和 `target\release\resume-one-theme-migrate.exe`。该命令尚未在 Windows 实机验收，不得把预期路径当成已交付的 Windows 安装包。

Windows 可用 `powershell -File packaging/windows/bundle.ps1 -Profile release -Archive` 装配便携 ZIP。脚本自动定位 Windows SDK 的 `x64/fxc.exe`，也可设置 `GPUI_FXC_PATH`。包内附规范、手册、原生主题和示例。Windows 桌面构建使用原生 Windows 环境编译 DirectX 着色器。

### 随包加入社区主题

本地源码构建默认提供 20 套原生主题。需要和发布包一样内置社区主题时，将主题仓库检出到 [锁定提交](./packaging/community-themes.ref)，并在运行 Cargo 或打包脚本前设置 `RESUME_ONE_COMMUNITY_THEMES_DIR` 为其中 `themes/` 的绝对路径。

```sh
git -C ../resume-one-themes checkout --detach "$(cat packaging/community-themes.ref)"
export RESUME_ONE_COMMUNITY_THEMES_DIR="$(cd ../resume-one-themes/themes && pwd)"
bash packaging/macos/bundle.sh release
```

Windows 可在 PowerShell 用 `$env:RESUME_ONE_COMMUNITY_THEMES_DIR = (Resolve-Path '../resume-one-themes/themes').Path` 指定同一目录，然后执行 Windows 打包命令。使用已发布的主题 ZIP 时，也可直接在应用「管理」中选择解压后的 `themes/`。

## 三仓与自动发布

| 仓库 | 内容 | 发布触发 |
| --- | --- | --- |
| `resume-one` | 应用、两个 CLI、原生主题、规范与手册 | `v*` 标签在原生 runner 构建 macOS ARM64 与 Windows x64 ZIP。校验包结构、二进制架构和主题文件后创建 GitHub 预发布。 |
| `resume-one-themes` | 社区主题、许可、迁移记录与两个索引 | `v*` 标签在验证 Schema 和 SHA-256 后发布完整主题 ZIP。 |
| `resume-one-site` | 简里静态项目主页 | `main` 构建通过后部署 GitHub Pages。仓库的 Pages 发布来源需选择 GitHub Actions。 |

应用工作流从同一所有者的主题仓库检出锁定提交。主题仓库须先包含该提交。私有仓库可通过应用仓库的 Actions Secret `THEMES_READ_KEY` 使用主题仓库的只读 Deploy Key，或使用仅具有该主题仓库内容读取权限的 `THEMES_READ_TOKEN`。两种凭证只在打包时使用。应用和主题工作流支持手动构建制品，创建 Release 由标签推送触发。预发布仍采用当前本地验收口径。

macOS 如需保留已有应用，可向 `bundle.sh` 传第二个参数指定尚不存在的绝对 `.app` 路径；向 `archive.sh` 传第三个参数指定该应用归档。归档前后均检查封签。项目采用 [Apache-2.0](./spec/LICENSE-APACHE-2.0.txt)，第三方与字体声明保留在包内。

JSON Resume Schema 来自 `@jsonresume/schema@1.3.1`，其原始包声明 MIT 许可证，Copyright © 2024 JSON Resume；原文件 SHA-256 为 `a07eedd3d86ac5bb61d72e136d788269e35baf42c35391e15d0be39b3dc5a4bd`。PDF / DOCX 内置 Noto Sans SC 和 Noto Serif SC 静态字体按 OFL 1.1 分发。许可全文与随附声明见 [第三方许可](./spec/THIRD_PARTY_LICENSES.md)。

## 文档

- 共享编辑内核已支持无效输入保留、最近有效预览、无损格式化、带 revision 守卫的全文保存及独立的本地恢复草稿存储，接口契约见 [编辑缓冲区规范](./spec/editor-buffer.md)；原生编辑器已接入这些接口。

- [用户手册](./docs/user-manual.md)
- [CLI 手册](./docs/cli-manual.md)
- [主题格式规范](./resume-one-theme-spec-v2.md)

仓库仅提交实现必需的源码、测试、构建配置，以及 README、正式规范和上述手册；历史调研、设计过程稿与浏览器测试产物不纳入提交。
