# Resume One CLI 手册

状态：`resume-one` CLI 尚未实现；此处记录稳定目标接口，不应将下列命令视为当前可执行。

`resume-one` 是独立本地程序，不依赖桌面应用运行，不会自动读取 GUI 当前打开的简历。所有文件操作都需要显式 `--file` 或 `--dir`。计划命令：`init`、`list`、`inspect`、`validate`、`read`、`patch`、`export`、`schema`。导出格式为 `pdf`、`docx`、`txt`、`md`。

Agent 修改文件的流程为：先用 `inspect --file ... --json` 获取 revision，必要时用 `read --pointer ...` 读取字段，再用 `patch --if-match REV --dry-run --json` 预检，最后在 revision 不变时提交。`inspect` 不输出简历正文；只有 `read` 明确返回内容。`--json` 模式的 stdout 只输出单行机器协议，不混入日志或进度。已有导出目标默认不覆盖。

迁移主题的独立程序名为 `resume-one-theme-migrate`，**不属于**这个 CLI；它只处理主题文件，不处理用户的 `resume.json`。
