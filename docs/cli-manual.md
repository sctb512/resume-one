# Resume One CLI 手册

状态：本地文档命令及 PDF、DOCX、纯文本、Markdown 导出已实现。PDF 支持主题的单栏基础视觉参数；独立迁移工具支持 v1→v2 及第三方主题低置信度静态提取。完整主题排版仍在开发。

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
| `export` | `resume-one export --file /absolute/resume.json --format pdf --out /absolute/resume.pdf --theme /absolute/dir/name.theme.json` | `pdf`、可编辑 `docx`、`txt`、`md`；`--lang zh`（默认）或 `en`；已有目标需显式 `--overwrite`；`--theme` 当前仅供 PDF 使用 |
| `schema` | `resume-one schema` | 返回固定版本及内置 Schema 内容 |

`--json` 可放在子命令前或后。成功或失败时 stdout 都恰好是一行 JSON，结构为 `{"protocolVersion":1,"ok":true,"result":{...}}` 或 `{"protocolVersion":1,"ok":false,"error":{"code":"...","message":"..."}}`；错误可能附 `file`、`revision`、`error.diagnostics`。非 JSON 模式成功输出格式化结果，错误写入 stderr。`inspect`、`validate` 和补丁结果不输出简历正文；只有 `read` 会返回字段值。Schema 错误只列 JSON Pointer 和失败规则，不回显原值。建议不要长期保存 `read` 的输出日志。

退出码：`0` 成功、`2` 参数错误、`3` JSON/Schema/结构/补丁错误、`4` revision 冲突、`5` 文件 I/O、`6` 导出错误。`init` 与导出默认不覆盖现有文件；导出目标不能是源 JSON 文件。`patch`、`structure` 和 `set-field` 都必须传 `--if-match`，其值来自先前 `inspect` 的 revision；revision 是**原文件字节**的 SHA-256，因此仅改空格也会触发冲突。写入前还会再次核对 revision，操作失败或结果不符合 Schema 时不替换原文件。`--dry-run` 不创建锁文件或修改原文件。

`structure` 有四个操作：`add-module ID [--before ID]`、`remove-module ID`、`add-item ID --index N`、`remove-item ID --index N`。`N` 从 0 开始；新增条目为 `{}`，新增的普通模块为 `[]`，基本信息模块为 `{}`，内容需随后补齐。`--before` 指定现有标准模块，省略时追加；不存在的模块、越界条目、重复模块或不支持的 ID 都会报错。移除模块会删除其中所有条目，建议先 `--dry-run` 并核对目标。自定义扩展字段和其他模块顺序保持不变；命令结果只返回 revision 和变更路径，不回显正文。

`set-field` 仅允许 JSON Resume 标准模块中的普通字符串字段（如 `basics.name`、`work.position`、`education.institution`）；地址可用 `--section basics --field location.city` 等点分字段，不会自动注入富文本或样式。列表模块必须指定从 0 开始的 `--index`，`basics` 必须省略。`--value ''` 会删除该字段；内容相同则保留原始文件字节和 revision，不执行写入。邮箱、URL、日期等仍按内置 Schema 校验，不合规则返回错误并保留原文件。数组字段（如 `highlights`、`keywords`）与自定义扩展字段请使用 `patch`，并对目标先做 `test`。

导出使用同一标准模块语义提取层：姓名、联系方式、概述、工作、志愿、教育、奖项、证书、出版物、技能、语言、兴趣、推荐人及项目；模块顺序按原 JSON 顶层顺序。自定义扩展字段保留在原 JSON 中，但不会未经模板定义就输出。Markdown 中的用户文本会转义；仅 HTTP(S) URL 生成可点击链接。PDF 默认 A4、使用内置简里配色；也可用 `--theme` 指定本地声明式 v1/v2 主题文件，支持根目录 `*.theme.json` 或一级子目录 `theme.json`。主题会先按 Schema 和安全规则验证；当前 PDF 支持单栏布局、四种基础 `layout`、A4/Letter、边距、模块间距、颜色、基础字号和行高；只支持内置无衬线 400 与衬线 600 两种字重。不支持的侧栏/分栏、特殊页眉/模块/装饰、自带字体及字距会报错，不生成不完整的 PDF。PDF 嵌入完整 Noto Sans SC 与 Noto Serif SC 字形并自动换页，文件可能达到约 15 MB。DOCX 为可编辑的 Word 文档，仍使用基础样式；字体引用 Noto Sans SC，如阅读设备未安装，Word/LibreOffice 会使用其替代字体。纯文本和 Markdown 不应用主题。导出不会写回主题或格式元数据到 `resume.json`。

Agent 修改文件的流程为：先用 `inspect --file ... --json` 获取 revision，必要时用 `read --pointer ...` 读取字段；标准模块和条目增删优先用 `structure --if-match REV --dry-run --json` 预检，普通文本字段优先用 `set-field --if-match REV --dry-run --json`，其他具体字段用 `patch --if-match REV --dry-run --json`。预检后在 revision 不变时提交。数组按位置寻址时，建议在补丁中先加 `test` 操作确认条目身份；冲突后应重新读取，不可盲目重放旧操作。

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
