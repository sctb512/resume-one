# Resume One CLI 手册

状态：本地文档命令及 PDF、DOCX、纯文本、Markdown 导出已实现。PDF 和 DOCX 支持主题的单栏基础视觉参数；独立迁移工具支持 v1→v2 及第三方主题低置信度静态提取。完整主题排版仍在开发。

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

导出使用同一标准模块语义提取层：姓名、联系方式、概述、工作、志愿、教育、奖项、证书、出版物、技能、语言、兴趣、推荐人及项目；模块顺序按原 JSON 顶层顺序。自定义扩展字段保留在原 JSON 中，但不会未经模板定义就输出。Markdown 中的用户文本会转义；仅 HTTP(S) URL 生成可点击链接。PDF / DOCX 默认 A4、使用内置简里配色；也可用 `--theme` 指定本地声明式 v1/v2 主题文件，支持根目录 `*.theme.json` 或一级子目录 `theme.json`。主题会先按 Schema 和安全规则验证；目前支持单栏、四种基础 `layout`、A4/Letter、边距、模块间距、颜色、基础字号和行高，以及内置无衬线 400 与衬线 600。侧栏/分栏、特殊页眉/模块/装饰、自带字体及字距会报错，失败时不替换已有输出。

PDF 嵌入完整 Noto 字形，使用共享测量布局，文件可能达到约 15 MB。DOCX 使用可编辑的原生 Word 段落、显式换行、主题分隔线和重复的页底矢量背景，保留模块顺序与条目避断页设置；字号按 Word 半点单位取整，600 字重映射为 Word 粗体。DOCX 按 OOXML 规范嵌入完整 Noto Sans SC 与 Noto Serif SC 字体，未做仅覆盖当前文字的子集化，文件约 18 MB。支持嵌入字体的阅读器不需要预装字体；禁用嵌入字体或不支持该特性的阅读器仍可能替代字体。Word / LibreOffice 自行换行和分页，不承诺与 PDF 逐像素相同；尚未完成 Microsoft Word 跨平台实机验收。纯文本和 Markdown 不应用主题，向它们传 `--theme` 会报参数错误。所有导出均不写回主题或格式元数据到 `resume.json`。

PDF / DOCX 同时支持主题内 `sections.<标准模块>.heading` 的 `plain/line/accent-line/filled`，`items` 的 `stack/compact`，以及 `date: inline`；配置只作用于对应模块。紧凑模式仅减少条目与描述段落的间距，不改变文字、字号、字段顺序或模块数量。时间线、卡片、标签及左右日期仍返回 `EXPORT_RENDER`，不覆盖已有输出。本地主题和 `themes download` 的能力检查一致。

单栏 `composition.header` 支持 `stacked`（默认左对齐）和 `centered`（按内容区逐行居中），`composition.contact` 支持 `inline`（默认行内连接）和 `stacked`（每项独立起行）。两项可任意组合，长姓名、职业和联系方式自然换行，不截断；摘要与正文保持左对齐。DOCX 使用原生段落对齐及文本换行，仍可编辑；不支持的分割页眉、图标联系方式返回 `EXPORT_RENDER`，不会覆盖已有输出或修改源 JSON。

```sh
resume-one export --file /absolute/resume.json --format docx --out /absolute/resume.docx --theme /absolute/themes/chosen.theme.json --lang zh --json
```

Agent 修改文件的流程为：先用 `inspect --file ... --json` 获取 revision，必要时用 `read --pointer ...` 读取字段；标准模块和条目增删优先用 `structure --if-match REV --dry-run --json` 预检，普通文本字段优先用 `set-field --if-match REV --dry-run --json`，其他具体字段用 `patch --if-match REV --dry-run --json`。预检后在 revision 不变时提交。数组按位置寻址时，建议在补丁中先加 `test` 操作确认条目身份；冲突后应重新读取，不可盲目重放旧操作。

## 在线主题命令

`themes` 只接受公开 GitHub 来源，不接受网站 URL、令牌或私有仓库。显式参数为 `--repository owner/repository`、`--ref`（默认 `main`）、`--index`（默认 `themes/index.json`）、绝对路径 `--cache`；它不会读取或更改 GUI 的目录设置。这里的 `owner/repository` 是参数占位符，不是软件默认源。

```sh
resume-one themes --repository owner/repository --ref main --cache /absolute/theme-cache list --json
resume-one themes --repository owner/repository --ref main --cache /absolute/theme-cache --offline download calm-paper --dir /absolute/local-themes --json
```

不带 `--offline` 时，分支/标签先解析成完整 commit，清单与主题文件均从该 commit 获取；可直接传 40 位 commit 固定版本。清单失败但旧缓存有效时返回旧清单及 `stale: true`、`warning`；没有缓存则报错。`list` 返回清单元数据，`manifestsValidated: false` 明确表示还未逐项验证主题，不能据此宣称可应用。`download ID --dir ...` 才校验原始文件的 SHA-256（清单提供时）、ID、名称、声明式格式和当前排版能力，再安装到显式指定且已存在的本地目录。

下载文件名包含来源指纹、主题 ID 和内容摘要，保留原始 UTF-8 字节，不写入样式、HTML、简历正文或缓存元数据；已存在且字节相同则幂等返回，内容不同或为符号链接则拒绝覆盖。缓存与本地安装目录是两回事：缓存可按最近访问时间自动淘汰，总量 256 MiB；本地下载文件不会被缓存清理删除。`--offline` 不产生网络请求，只允许有效缓存。源/主题文件上限各 1 MiB，单份清单最多 500 项，网络请求超时 15 秒，不跟随重定向，也不请求图片、脚本或字体资源。来源、网络、缓存和主题下载错误退出码为 `7`；相对路径参数仍为 `2`。

`verify-theme-source` 只读取所选仓库的 `themes/index.json`、`themes/review.json` 与清单列出的主题文件；不会联网、下载、发布、修改文件或运行第三方主题代码。它要求每项主题有原文件 SHA-256、v2 `adapted` 来源、可用的当前渲染能力，以及与主题 ID、摘要、上游 URL/版本/许可一致的人工验收记录。记录遵循 [`theme-source-review-v1.schema.json`](../spec/theme-source-review-v1.schema.json)，包含审核人、日期、中英文短/长、缺字段、多页及 PDF 检查声明。失败退出码 `7`；成功结果的 `structurallyVerified: true` 只证明记录完整且一致，`humanReviewIndependentlyVerified: false` 明确表明没有机器证明许可或视觉审核真实完成。发布者仍须人工核验上游许可和视觉/PDF 证据，公开 GitHub 仓库并固定真实 commit 后再配置默认源；此命令不代替发布流程。

## 独立迁移工具

迁移主题的独立程序名为 `resume-one-theme-migrate`，**不属于**这个 CLI；它只处理主题文件，不处理用户的 `resume.json`。从源码可用 `cargo run -p resume-one-theme-migrate -- <参数>`。v1 声明式主题升级用法：

```sh
resume-one-theme-migrate --source /absolute/old.theme.json --out /absolute/new.theme.json --license MIT --upstream-url https://example.com/theme --upstream-revision abc123 --dry-run
```

第三方主题包的静态提取用法：

```sh
resume-one-theme-migrate --source /absolute/jsonresume-theme-example --out /absolute/example.theme.json --upstream-url https://github.com/owner/jsonresume-theme-example --dry-run
```

`--source` 可指向主题目录或单个 CSS/HTML/JS/TS 等静态源码文件。工具只读取 `package.json` 和浅层样式源码，不运行 JS、模板或安装依赖，也不读取任何简历；从目录提取时最多扫描两层子目录、64 个样式文件、每文件 512 KiB、总共 4 MiB，跳过符号链接和超限文件。候选只包含可识别的十六进制配色、包名、版本与许可声明；布局固定为安全的单栏保底样式，字体、模块构件、条件逻辑和原视觉效果**没有自动迁移**。报告中的 `extraction.confidence` 为 `low`，`unmapped` 列出差异；缺少可信配色时使用保底色。该候选可供人工继续编辑，不能直接收入默认在线主题仓库。

去掉 `--dry-run` 才写入。目标为新的 `*.theme.json` 或 `theme.json`，默认不覆盖，`--overwrite` 也绝不覆盖来源。`--license`、`--upstream-url` 和 `--upstream-revision` 只记录声明，迁移器不会联网核实；没有许可声明时使用 `NOASSERTION`。程序始终返回单行 JSON，含 v2 `candidate`、来源/候选 SHA-256、`written` 和人工审核清单；`licenseVerified`、`visualVerified` 均为 `false`。目录来源的 SHA-256 是本次实际读取文件的有序内容指纹，不代表未扫描或被跳过文件。v1 升级仅转换数据结构；第三方提取仅为人工移植起点，都不构成视觉或再分发许可验收。
