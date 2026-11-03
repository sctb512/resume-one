# Resume One CLI 手册

锁竞争：`patch`、`structure`、`set-field` 在其他写入者持锁时立即以退出码 `5` 和错误码 `RESUME_BUSY` 返回，不无限等待、不覆盖原文件。稍后重新 `inspect` 获取 revision 后再决定是否重试；不要删除旁路锁文件。

状态：本地文档命令及 PDF、DOCX、纯文本、Markdown 导出已实现。独立迁移工具支持 v1→v2、第三方低置信度候选，以及完整有界源码快照、JS/JSX/CSS 语法证据和严格未完成拦截；结构编译及完整视觉迁移仍在开发，不能把语法解析通过当成 100% 还原。

`resume-one` 是独立本地程序，不依赖桌面应用运行，不会自动读取 GUI 当前打开的简历。所有文件操作都需要显式的**绝对路径** `--file` 或 `--dir`。从仓库运行时可使用 `cargo run -p resume-one-cli -- <命令>`；构建后的程序名为 `resume-one`。Windows PowerShell 中路径应加引号，例如 `--file 'C:\Users\Me\Documents\resume.json'`。

| 命令 | 示例 | 结果 |
|---|---|---|
| `init` | `resume-one init --file /absolute/resume.json` | 创建最小有效文件；已存在则拒绝覆盖 |
| `list` | `resume-one list --dir /absolute/folder` | 只列本目录 `.json` 普通文件，不读正文、不递归 |
| `inspect` | `resume-one inspect --file /absolute/resume.json` | 字节级 SHA-256 revision、模块与数量；不返回正文 |
| `validate` | `resume-one validate --file /absolute/resume.json` | 用内置 JSON Resume Schema 1.3.1 校验 |
| `read` | `resume-one read --file /absolute/resume.json --pointer /work/0/summary` | 明确读取正文；省略 `--pointer` 即读取完整 JSON |
| `patch` | `resume-one patch --file /absolute/resume.json --patch /absolute/change.json --if-match sha256:... --dry-run` | RFC 6902 补丁预演；去掉 `--dry-run` 才写入；`--patch -` 从 stdin 读取 |
| `structure` | `resume-one structure --file /absolute/resume.json --if-match sha256:... --dry-run add-module skills --before education` | 安全增删标准模块与条目；去掉 `--dry-run` 才写入 |
| `set-field` | `resume-one set-field --file /absolute/resume.json --if-match sha256:... --section work --index 0 --field position --value '设计师' --dry-run` | 设置标准文本字段；`basics` 不带 `--index`；空值移除字段 |
| `export` | `resume-one export --file /absolute/resume.json --format pdf --out /absolute/resume.pdf --theme /absolute/dir/name.theme.json` | `pdf`、可编辑 `docx`、`txt`、`md`；`--lang zh`（默认）或 `en`；已有目标需显式 `--overwrite`；`--theme` 支持 PDF / DOCX |
| `schema` | `resume-one schema` | 返回固定版本及内置 Schema 内容 |
| `themes` | `resume-one themes --repository owner/repository --ref main --cache /absolute/theme-cache list --json` | 显式联网读取声明式主题清单；`--offline` 只读已验证缓存；不读取简历 |
| `verify-theme-source` | `resume-one verify-theme-source --dir /absolute/source-checkout --json` | 离线检查待发布默认源的清单、主题文件和人工验收记录；不读取简历 |

`--json` 可放在子命令前或后。成功或失败时 stdout 都恰好是一行 JSON，结构为 `{"protocolVersion":1,"ok":true,"result":{...}}` 或 `{"protocolVersion":1,"ok":false,"error":{"code":"...","message":"..."}}`；错误可能附 `file`、`revision`、`error.diagnostics`。非 JSON 模式成功输出格式化结果，错误写入 stderr。`inspect`、`validate` 和补丁结果不输出简历正文；只有 `read` 会返回字段值。Schema 错误只列 JSON Pointer 和失败规则，不回显原值。建议不要长期保存 `read` 的输出日志。

退出码：`0` 成功、`2` 参数错误、`3` JSON/Schema/结构/补丁错误、`4` revision 冲突、`5` 文件 I/O、`6` 导出错误、`7` 在线主题源/缓存/下载错误。`init` 与导出默认不覆盖现有文件；导出目标不能是源 JSON 文件。`patch`、`structure` 和 `set-field` 都必须传 `--if-match`，其值来自先前 `inspect` 的 revision；revision 是**原文件字节**的 SHA-256，因此仅改空格也会触发冲突。写入前还会再次核对 revision，操作失败或结果不符合 Schema 时不替换原文件。`--dry-run` 不创建锁文件或修改原文件。

`structure` 有四个操作：`add-module ID [--before ID]`、`remove-module ID`、`add-item ID --index N`、`remove-item ID --index N`。`N` 从 0 开始；新增条目为 `{}`，新增的普通模块为 `[]`，基本信息模块为 `{}`，内容需随后补齐。`--before` 指定现有标准模块，省略时追加；不存在的模块、越界条目、重复模块或不支持的 ID 都会报错。移除模块会删除其中所有条目，建议先 `--dry-run` 并核对目标。自定义扩展字段和其他模块顺序保持不变；命令结果只返回 revision 和变更路径，不回显正文。

`set-field` 仅允许 JSON Resume 标准模块中的普通字符串字段（如 `basics.name`、`work.position`、`education.institution`）；地址可用 `--section basics --field location.city` 等点分字段，不会自动注入富文本或样式。列表模块必须指定从 0 开始的 `--index`，`basics` 必须省略。`--value ''` 会删除该字段；内容相同则保留原始文件字节和 revision，不执行写入。邮箱、URL、日期等仍按内置 Schema 校验，不合规则返回错误并保留原文件。数组字段（如 `highlights`、`keywords`）与自定义扩展字段请使用 `patch`，并对目标先做 `test`。

导出使用同一标准模块语义提取层：姓名、联系方式、概述、工作、志愿、教育、奖项、证书、出版物、技能、语言、兴趣、推荐人及项目；模块顺序按原 JSON 顶层顺序。自定义扩展字段保留在原 JSON 中，但不会未经模板定义就输出。Markdown 中的用户文本会转义；仅 HTTP(S) URL 生成可点击链接。PDF / DOCX 默认 A4、使用内置简里配色；也可用 `--theme` 指定本地声明式 v1/v2 主题文件，支持根目录 `*.theme.json` 或一级子目录 `theme.json`。主题会先按 Schema 和安全规则验证；目前支持单栏或左右侧栏正文、分栏页眉、四种基础 `layout`、A4/Letter、边距、模块间距、颜色、基础字号和行高，以及内置无衬线 400 与衬线 600。任意等宽分栏、图标联系方式、特殊模块/装饰及字距会报错，失败时不替换已有输出。

本地主题的 `assets.fonts` 可提供最多四个静态 TTF/OTF（每文件不超过 8 MiB），用 `asset:<id>` 指定给正文/姓名/模块标题/辅助文字；文件相对主题所在目录，路径、原字节 SHA-256、字体格式及实际字重须通过验证。渲染使用真实字面并检查嵌入权限，不安装到系统或简里字体库，不修改主题/简历。缺少资源、可变字体、WOFF/WOFF2、集合或字重不符明确失败，不静默改成内置字体。同名字族/字重但内容不同的字面不能在同一进程混用；具体约束见[主题规范](../resume-one-theme-spec-v2.md#3-字段语义)。在线来源仍不下载字体资源，不能用在线单文件下载代替完整主题包。

PDF 嵌入完整 Noto 字形，使用共享测量布局，文件可能达到约 15 MB。DOCX 使用可编辑的原生 Word 段落、显式换行、主题分隔线和重复的页底矢量背景，保留模块顺序与条目避断页设置；字号按 Word 半点单位取整，600 字重映射为 Word 粗体。DOCX 按 OOXML 规范嵌入完整 Noto Sans SC 与 Noto Serif SC 字体，未做仅覆盖当前文字的子集化，文件约 18 MB。支持嵌入字体的阅读器不需要预装字体；禁用嵌入字体或不支持该特性的阅读器仍可能替代字体。Word / LibreOffice 自行换行和分页，不承诺与 PDF 逐像素相同；尚未完成 Microsoft Word 跨平台实机验收。纯文本和 Markdown 不应用主题，向它们传 `--theme` 会报参数错误。所有导出均不写回主题或格式元数据到 `resume.json`。

PDF / DOCX 同时支持主题内 `sections.<标准模块>.heading` 的 `plain/line/accent-line/filled`，`items` 的 `stack/compact`，以及 `date: inline/left/right`；配置只作用于对应模块。左侧日期占标题左侧，标题续行与首行对齐；右侧日期在标题首行右侧。两者都给日期和标题留至少 12 pt 间隙，过长日期独立成行，其他辅助信息留在下方。DOCX 使用可编辑的左/右制表位与段落，长标题按共享字形测量拆行。紧凑模式仅减少条目与描述段落的间距，不改变文字、字号、字段顺序或模块数量。时间线、卡片、标签仍返回 `EXPORT_RENDER`，不覆盖已有输出。本地主题和 `themes download` 的能力检查一致。

`composition.header` 支持 `stacked`（默认左对齐）、`centered`（按内容区逐行居中）和 `split`（身份在左、联系方式在右）；`preset: split-header` 也可启用分栏页眉，但不能同时声明不一致的 `header`。`composition.contact` 支持 `inline`（默认行内连接）和 `stacked`（每项独立起行）。长姓名、职业和联系方式自然换行，不截断；分栏页眉在缺少一侧内容或单页无法容纳时，PDF/预览退回全宽流式页眉。摘要保持全宽左对齐，正文可按侧栏规则分区。DOCX 分栏页眉使用无边框双单元格表格，文字仍可编辑，由阅读器重排；不支持的图标联系方式返回 `EXPORT_RENDER`，不会覆盖已有输出或修改源 JSON。

```sh
resume-one export --file /absolute/resume.json --format docx --out /absolute/resume.docx --theme /absolute/themes/chosen.theme.json --lang zh --json
```

Agent 修改文件的流程为：先用 `inspect --file ... --json` 获取 revision，必要时用 `read --pointer ...` 读取字段；标准模块和条目增删优先用 `structure --if-match REV --dry-run --json` 预检，普通文本字段优先用 `set-field --if-match REV --dry-run --json`，其他具体字段用 `patch --if-match REV --dry-run --json`。预检后在 revision 不变时提交。数组按位置寻址时，建议在补丁中先加 `test` 操作确认条目身份；冲突后应重新读取，不可盲目重放旧操作。

## 在线主题命令

### 独立外观文件

PDF/DOCX 导出可额外指定 `--appearance /absolute/appearance.json`，须与 `--theme` 一起使用。它只覆盖本次渲染，不改写简历、主题或桌面设置，也不隐式读取本机 GUI 偏好：

```sh
resume-one export --file /absolute/resume.json --format pdf --out /absolute/resume.pdf --theme /absolute/themes/chosen.theme.json --appearance /absolute/appearance.json --json
```

外观文件直接包含 `{"formatVersion":1,"bodyFont":"font:Arial","headingFont":null,"colors":{"accent":"#52756B"}}`，不含应用设置的外层 `appearance` 键。字体支持 `cjk-sans` / `cjk-serif` 或 `font:<原生字族名称>`，null 或省略表示跟随主题；颜色允许 `accent/paper/canvas/ink/muted/border/seal` 的 `#RRGGBB`。文件上限 64 KiB；格式、版本、字体名称格式或颜色无效返回 `APPEARANCE_INVALID`（退出码 3），读取错误为 `APPEARANCE_IO`（5），不创建或替换输出。字体未安装、字形缺失、数据格式或嵌入标记不满足导出要求时明确报告渲染错误，不静默替代字族。TXT/Markdown 不支持主题外观参数。完整契约见[独立外观设置 v1](../spec/appearance-settings.md)。

### 本机字体

`resume-one fonts list --json` 列出原生系统字族及简里字体库字族，不读取简历。`resume-one fonts install --file /absolute/new-font.ttf --json` 显式安装到简里本机库，不修改系统字体目录或注册表；支持 TTF / OTF / TTC / OTC，先校验、后原子创建，重复内容不再复制。可加 `--dir /absolute/font-library` 指定纯文件安装目录；该目录不会自动加入桌面或导出搜索路径。所有路径须为绝对路径；错误返回 `FONT_LIST`、`FONT_LIBRARY` 或 `FONT_INSTALL`（退出码 5）。

### 来源与下载

`themes` 只接受公开 GitHub 来源，不接受网站 URL、令牌或私有仓库。显式参数为 `--repository owner/repository`、`--ref`（默认 `main`）、`--index`（默认 `themes/index.json`）、绝对路径 `--cache`；它不会读取或更改 GUI 的目录设置。这里的 `owner/repository` 是参数占位符，不是软件默认源。

```sh
resume-one themes --repository owner/repository --ref main --cache /absolute/theme-cache list --json
resume-one themes --repository owner/repository --ref main --cache /absolute/theme-cache --offline download calm-paper --dir /absolute/local-themes --json
```

不带 `--offline` 时，分支/标签先解析成完整 commit，清单与主题文件均从该 commit 获取；可直接传 40 位 commit 固定版本。清单失败但旧缓存有效时返回旧清单及 `stale: true`、`warning`；没有缓存则报错。`list` 返回清单元数据，`manifestsValidated: false` 明确表示还未逐项验证主题，不能据此宣称可应用。`download ID --dir ...` 才校验原始文件的 SHA-256（清单提供时）、ID、名称、声明式格式和当前排版能力，再安装到显式指定且已存在的本地目录。

下载文件名包含来源指纹、主题 ID 和内容摘要，保留原始 UTF-8 字节，不写入样式、HTML、简历正文或缓存元数据；已存在且字节相同则幂等返回，内容不同或为符号链接则拒绝覆盖。缓存与本地安装目录是两回事：缓存可按最近访问时间自动淘汰，总量 256 MiB；本地下载文件不会被缓存清理删除。`--offline` 不产生网络请求，只允许有效缓存。源/主题文件上限各 1 MiB，单份清单最多 500 项，网络请求超时 15 秒，不跟随重定向，也不请求图片、脚本或字体资源。来源、网络、缓存和主题下载错误退出码为 `7`；相对路径参数仍为 `2`。

`verify-theme-source` 只读取所选仓库的 `themes/index.json`、`themes/review.json`、清单列出的主题文件及 `qa/` 下的验收证据；不会联网、下载、发布、修改文件或运行第三方主题代码。它要求每项主题有原文件 SHA-256、v2 `adapted` 来源、可用的当前渲染能力，以及与主题 ID、摘要、上游 URL/版本/许可一致的人工验收记录。记录遵循 [`theme-source-review-v1.schema.json`](../spec/theme-source-review-v1.schema.json)，包含审核人、日期、中英文短/长、缺字段、多页样例的上游与迁移版 PNG 截图，以及一份多页 PDF；全部证据需附原文件 SHA-256。每主题共 13 个证据文件，截图上限各 8 MiB、PDF 30 MiB、合计 512 MiB；重复路径、缺失文件、摘要不符或符号链接都会失败。证据只允许公开测试简历，不得含私人内容。失败退出码 `7`；成功结果增加 `evidenceFilesVerified`，`structurallyVerified: true` 只证明记录完整且一致，`humanReviewIndependentlyVerified: false` 明确表明没有机器证明许可或视觉审核真实完成。程序只检查 PNG/PDF 文件签名，不完整解码或判断相似度。发布者仍须人工打开上游和迁移版证据，核验上游许可及视觉/PDF 效果，公开 GitHub 仓库并固定真实 commit 后再配置默认源；此命令不代替发布流程。

## 独立迁移工具

### 尽量还原的适配模式

允许明确视觉替代时，使用 `--adapt`；原有 `--require-complete` 仍是严格等价门禁，不与它混用。适配器读取真实语法证据，将可表达的颜色、字体角色、字号、版面与模块标题/顺序投影到声明式 v2；未支持的样式、推断和替代写入报告，不执行主题代码，不修改 resume JSON。

```sh
resume-one-theme-migrate --source /absolute/upstream --adapt --out /absolute/example.theme.json --verify-fixture /absolute/public-en.json --verify-fixture /absolute/public-zh.json
resume-one-theme-migrate --source /absolute/catalog.json --adapt --batch --out /absolute/new-theme-packages --font-library /absolute/font-library.json --verify-fixture /absolute/public-en.json --verify-fixture /absolute/public-zh.json --verify-fixture /absolute/public-long.json --verify-fixture /absolute/public-missing.json --verify-exports
```

批量清单为 `{"items":[{"slug":"example","sourceDirectory":"/absolute/upstream","upstreamUrl":"https://example.com/source","upstreamRevision":"fixed-version","license":"MIT","licenseFile":"/absolute/LICENSE"}]}`。目录必须是绝对路径，slug 唯一。目标必须是新目录，不覆盖已有本地主题。每套输出 `community-<slug>/theme.json`、`MIGRATION.json`、`NOTICE.txt` 及引用的字体/许可，汇总为 `migration-report.json`。单项失败继续检查其余项，但整批返回退出码 3 和 `ADAPT_BATCH_PARTIAL`，不把失败算成完成。

`--verify-fixture` 只读取明确选择的公开/脱敏样本；报告保存指纹和计数，不保存样本正文。它检查 schema、模块渲染目标与纸面边界；`--verify-exports` 再检查 PDF/DOCX 字节生成。自动检查不代替原版逐主题截图、阅读器布局检查或 Microsoft Word 实机验收，`visualVerified` 仍为 false。

字体资料库是 `{"formatVersion":1,"fonts":[...]}`，每项含 `family/weight/fontStyle/path/sha256/license/licensePath`；字体和许可路径必须绝对，字体最多 8 MiB，只支持静态 TTF/OTF。对已改名但有明确来源的字体可用 `fontFamily` 指定真实内部字族，别名关系会记录为差异。工具只按精确字重与斜体匹配，最多嵌入 4 个字面；缺失时保留明确的内置字体替代，不安装字体或联网下载。带字体的适配使用批量目录模式。

完整约束见[适配协议](../spec/theme-adaptation-v1.md)。主题可用 `composition.moduleOrder` 指定显示顺序；未列出的模块稳定追加，不删数据，详见[模块顺序规范](../spec/theme-module-order-v1.md)。

批量可加 `--jobs 4` 并行处理；允许范围 1 至 4，默认 1。单项失败不影响其余主题继续检查，汇总顺序仍与清单一致。

迁移主题的独立程序名为 `resume-one-theme-migrate`，**不属于**这个 CLI；它只处理主题文件，不处理用户的 `resume.json`。从源码可用 `cargo run -p resume-one-theme-migrate -- <参数>`。v1 声明式主题升级用法：

```sh
resume-one-theme-migrate --source /absolute/old.theme.json --out /absolute/new.theme.json --license MIT --upstream-url https://example.com/theme --upstream-revision abc123 --dry-run
```

第三方主题包的旧版低置信度候选提取用法（不是完整迁移）：

```sh
resume-one-theme-migrate --source /absolute/jsonresume-theme-example --out /absolute/example.theme.json --upstream-url https://github.com/owner/jsonresume-theme-example --dry-run
```

`--source` 可指向主题目录或单个 CSS/HTML/JS/TS 等静态源码文件。工具只读取 `package.json` 和浅层样式源码，不运行 JS、模板或安装依赖，也不读取任何简历；从目录提取时最多扫描两层子目录、64 个样式文件、每文件 512 KiB、总共 4 MiB，跳过符号链接和超限文件。候选配色可从十六进制或不透明、整数通道的 `rgb(...)` / `rgba(..., 1)` 静态文本提取；半透明颜色需要背景合成，故不猜测结果。候选还包含包名、版本与许可声明；布局固定为安全的单栏保底样式。报告另列 `fontFamiliesFound`（原始字体声明）与 `layoutSignals`（如 grid、分栏、打印页面），每项都只是有界静态文本线索，可能来自注释或脚本字符串，**不会自动写入候选主题**；字体、模块构件、条件逻辑和原视觉效果没有自动迁移。`extraction.confidence` 为 `low`，`unmapped` 列出差异；缺少可信配色时使用保底色。该候选可供人工继续编辑，不能直接收入默认在线主题仓库。

去掉 `--dry-run` 才写入。目标为新的 `*.theme.json` 或 `theme.json`，默认不覆盖，`--overwrite` 也绝不覆盖来源。`--license`、`--upstream-url` 和 `--upstream-revision` 只记录声明，迁移器不会联网核实；没有许可声明时使用 `NOASSERTION`。程序始终返回单行 JSON，含 v2 `candidate`、来源/候选 SHA-256、`written` 和人工审核清单；`licenseVerified`、`visualVerified` 均为 `false`。目录来源的 SHA-256 是本次实际读取文件的有序内容指纹，不代表未扫描或被跳过文件。v1 升级仅转换数据结构；第三方提取仅为人工移植起点，都不构成视觉或再分发许可验收。

### 完整源码与语法检查

```sh
resume-one-theme-migrate --inspect --source /absolute/upstream-theme
resume-one-theme-migrate --inspect --source /absolute/upstream-theme --require-complete
```

`--inspect` 是只读操作，仅接受绝对主题目录，不接受 `--out`、许可覆盖、`--dry-run` 或 `--overwrite`。它读取包内普通文件及二进制资源并逐项计算 SHA-256；不再局限于浅层样式文件。限制为 16 层、1024 文件、4096 目录条目、每文件 8 MiB、总计 64 MiB；超限、特殊文件、路径冲突或符号链接（包含根目录/父路径）直接失败，不静默跳过。仅 `.git/node_modules/target` 三种目录树被明确排除并列于报告。原始包内的示例 JSON 只作为静态文件参与摘要，不作为用户简历渲染或执行。

JS/JSX 和 CSS 使用固定版本的 Tree-sitter 语法解析，报告保留静态导入、组件基类、重复声明、嵌套选择器/打印规则、JSX 字段/条件表达式的源位置。注释、普通字符串中的样式不冒充真实样式。动态样式、尚未解码的模板转义、未支持格式和缺失依赖均列为阻断项；不会执行表达式、安装依赖或请求字体网络地址。完整源码快照不等于依赖闭包完整，也不等于结构编译/许可/视觉验收通过。

固定字面量和顶层唯一 `const` 的有限数值运算可以静态展开，例如 `${GRID_SIZE * 10}px`；不调用 JS，不解释 props 函数或使用猜测值。成功项在 `constantSubstitutions`，未完成项在 `dynamicExpressions`，并保留原始位置和原因。展开 CSS 由真实 CSS 解析器再次处理，生成的声明范围映射回原文件；作用域冲突、写入、未初始化引用、未知运算或超限均拒绝。不支持项不会被替换成空文本。

报告为单行 JSON，包含 `inspectionVersion: 1`、`sourceSnapshot`、`statistics`、`syntaxEvidence` 和 `blockers`。`sourceSnapshot.complete` 只说明已完整读取声明边界内的文件；`migrationComplete/visualVerified/licenseVerified` 当前仍为 false。没有可靠的相似度或迁移百分比。详细契约见[迁移检查规范](../spec/theme-migration-inspection-v1.md)。

加 `--require-complete` 后，只要尚有未完成迁移或验收就返回退出码 `3`、错误 `MIGRATION_INCOMPLETE`，完整检查报告位于 `error.inspection`；不能被当成可发布主题。该标志也可用于旧候选命令：当前即使指定 `--overwrite` 也会在写入前拒绝，不改变已有文件。未加严格标志的旧候选输出保留兼容行为，但明确返回 `migrationComplete: false`。目前尚无完整语义编译与自动视觉通过路径，严格命令不会成功；这不是已实现“100% 自动迁移”的声明。

### 显式模板编译入口

```sh
resume-one-theme-migrate --source /absolute/upstream-theme --inspect --template-entry src/Resume.jsx
```

`--template-entry` 必须和 `--inspect` 一起使用，只能选择来源快照内的相对 JS/JSX 路径，最多 8 个、不重复，不读取包外文件。报告的 `templatePrograms` 保留真实 default-export 函数的元素、字段、条件、列表映射、词法作用域与来源范围。未知调用、属性、语句、样式或共享组件不能被静默跳过。输入无副作用执行，输出是纯数据中间结构，不是可直接安装的主题。

`program.resolved` 只表示受限模板前端无阻断；不代表 CSS、依赖、排版或视觉完成。缺省值不等于空字符串，可选链与括号边界保持独立语义。完整契约见[模板 IR 规范](../spec/theme-template-ir-v1.md)。最终主题迁移仍须通过来源协议的严格门禁。

模板前端也可描述无转义的非标签文本模板，以及数组字段的 `.join()` / `.join(', ')` 等有限操作；不会调用源代码。标量转文本、null/undefined、可选链、分隔符和原始空白各自保留语义。未知类型仍附带明确约束，复合值、未知调用或动态分隔符不能据此被认定为等价转换。

`templatePrograms[].styleGraph` 连接同模块的真实 styled 继承与模板 binding id，保留基类→派生类的样式层、重复声明和打印规则。外部共享组件仍须锁定其实现，不会被替换为普通容器。`inheritanceLinked` 不等于排版或视觉完成，`layoutCompiled` 当前仍为 false，严格迁移门禁不会因此放行。

### 显式共享依赖源码

```sh
resume-one-theme-migrate --source /absolute/theme --inspect --template-entry src/Resume.jsx --dependency /absolute/core --dependency /absolute/utils
```

每个 `--dependency` 指定一个本地包的绝对目录，最多 8 个；不会搜索、安装、下载或执行依赖。`dependencyBundle` 记录每包明确版本、完整文件指纹、exports 的精确文件与缺失项、原始导入和未支持能力。多个同名包或与主题同名的依赖会失败，不能按提供顺序猜测。全部依赖源码合计最多 64 MiB，仍拒绝符号链接与包外路径。

目前只支持显式字符串/子路径 exports，不执行条件导出、wildcard、loader 或 Node fallback。`file-present` 仅表示文件存在。所选模板另有 `moduleLinks`：可沿精确文件、具名/default 转导出与局部 import 别名追到真实声明，保留各层文件与包指纹；缺失、歧义、循环、星号/namespace 等明确阻断。

`declarationResolved` 只表示声明定位，不表示导出值稳定、组件已转换或 CSS 已恢复。`dependencyGraphComplete` 和 `rootExternalImportsResolved` 继续为 false，不能用提供了目录或找到函数替代真正的组件/传递依赖编译。`--require-complete` 仍会拒绝未完成的主题，不写近似产物。详见[依赖源码包协议](../spec/theme-dependency-bundle-v1.md)与[模块声明连接协议](../spec/theme-module-links-v1.md)。

同一命令还输出 `componentContent`。当前仅对固定版本、完整文件哈希及真实导入绑定匹配的 core Section，转换直接调用的有限容器内容，保留 children 顺序、空值、静态 id/className 和原始阻断。`wrapperContentLowered` 不表示子树、样式或排版完成；styled 包装、未知属性、动态 key/id/className 等继续阻断。完整边界见[共享组件内容转换规范](../spec/theme-shared-components-v1.md)。它仍不是可安装主题，也不会修改简历或上游源码。

### 用公开样例验证内容

```sh
resume-one-theme-migrate --source /absolute/theme --inspect --template-entry src/Resume.jsx --dependency /absolute/core --dependency /absolute/utils --fixture /absolute/public-resume.json
```

`--fixture` 是显式读取简历内容的开关，必须同时选择模板入口；不自动使用包里的示例。输出会包含样例文本，请使用公开/脱敏文件并保护报告。文件不超过 1 MiB，须通过内置 JSON Resume schema 校验；不会修改文件或把样式写回 JSON。

`contentEvaluation` 输出保留条件、列表、缺失/null 区别的未排版内容树，并支持固定来源 Section、SectionTitle、DateRange、ContactInfo 的有限域。错误不输出部分树，不用空内容顶替。`contentEvaluated:true` 只针对本次样例，不能当成主题已经迁移、CSS/分页已恢复或视觉验收通过；`--require-complete` 仍拒绝未完成主题。类型、来源与资源限制见[内容求值协议](../spec/theme-content-evaluation-v1.md)。
