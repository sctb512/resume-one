# Resume One CLI 手册

状态：本地文档命令、纯文本与 Markdown 导出已实现。PDF/DOCX 导出尚未实现；迁移器另行提供。

`resume-one` 是独立本地程序，不依赖桌面应用运行，不会自动读取 GUI 当前打开的简历。所有文件操作都需要显式的**绝对路径** `--file` 或 `--dir`。从仓库运行时可使用 `cargo run -p resume-one-cli -- <命令>`；构建后的程序名为 `resume-one`。Windows PowerShell 中路径应加引号，例如 `--file 'C:\Users\Me\Documents\resume.json'`。

| 命令 | 示例 | 结果 |
|---|---|---|
| `init` | `resume-one init --file /absolute/resume.json` | 创建最小有效文件；已存在则拒绝覆盖 |
| `list` | `resume-one list --dir /absolute/folder` | 只列本目录 `.json` 普通文件，不读正文、不递归 |
| `inspect` | `resume-one inspect --file /absolute/resume.json` | 字节级 SHA-256 revision、模块与数量；不返回正文 |
| `validate` | `resume-one validate --file /absolute/resume.json` | 用内置 JSON Resume Schema 1.3.1 校验 |
| `read` | `resume-one read --file /absolute/resume.json --pointer /work/0/summary` | 明确读取正文；省略 `--pointer` 即读取完整 JSON |
| `patch` | `resume-one patch --file /absolute/resume.json --patch /absolute/change.json --if-match sha256:... --dry-run` | RFC 6902 补丁预演；去掉 `--dry-run` 才写入；`--patch -` 从 stdin 读取 |
| `export` | `resume-one export --file /absolute/resume.json --format md --out /absolute/resume.md` | UTF-8 `txt` 或 `md`；`--lang zh`（默认）或 `en`；已有目标需显式 `--overwrite` |
| `schema` | `resume-one schema` | 返回固定版本及内置 Schema 内容 |

`--json` 可放在子命令前或后。成功或失败时 stdout 都恰好是一行 JSON，结构为 `{"protocolVersion":1,"ok":true,"result":{...}}` 或 `{"protocolVersion":1,"ok":false,"error":{"code":"...","message":"..."}}`；错误可能附 `file`、`revision`、`error.diagnostics`。非 JSON 模式成功输出格式化结果，错误写入 stderr。`inspect`、`validate` 和补丁结果不输出简历正文；只有 `read` 会返回字段值。Schema 错误只列 JSON Pointer 和失败规则，不回显原值。建议不要长期保存 `read` 的输出日志。

退出码：`0` 成功、`2` 参数错误、`3` JSON/Schema/补丁错误、`4` revision 冲突、`5` 文件 I/O、`6` 导出错误。`init` 与导出默认不覆盖现有文件；导出目标不能是源 JSON 文件。`patch` 必须传 `--if-match`，其值来自先前 `inspect` 的 revision；revision 是**原文件字节**的 SHA-256，因此仅改空格也会触发冲突。写入前还会再次核对 revision，补丁失败或结果不符合 Schema 时不替换原文件。`--dry-run` 连锁文件也不创建。

文本导出使用与未来 PDF/DOCX 相同的标准模块语义提取层：姓名、联系方式、概述、工作、志愿、教育、奖项、证书、出版物、技能、语言、兴趣、推荐人及项目。自定义扩展字段保留在原 JSON 中，但不会未经模板定义就输出。Markdown 中的用户文本会转义；仅 HTTP(S) URL 生成可点击链接。导出不会写回主题或格式元数据到 `resume.json`。

Agent 修改文件的流程为：先用 `inspect --file ... --json` 获取 revision，必要时用 `read --pointer ...` 读取字段，再用 `patch --if-match REV --dry-run --json` 预检，最后在 revision 不变时提交。数组按位置寻址时，建议在补丁中先加 `test` 操作确认条目身份；冲突后应重新读取，不可盲目重放旧补丁。

迁移主题的独立程序名为 `resume-one-theme-migrate`，**不属于**这个 CLI；它只处理主题文件，不处理用户的 `resume.json`。
