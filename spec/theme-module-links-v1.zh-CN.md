# 主题源码模块声明连接 v1

[English](./theme-module-links-v1.md) · **简体中文**

状态：独立迁移工具的静态编译前端步骤，连接真实导入、转导出和声明身份。不执行模块，不证明导出值、组件语义、CSS 或整主题迁移。

## 输入与输出

沿用 `--inspect --template-entry ... --dependency ...`，不新增搜索路径或隐式包下载。全部证据取自同一次有界来源快照：同一个 Tree-sitter 完整文件 CST 与原始字节生成 `syntaxEvidence[].moduleSymbols`，不从展示片段重新解析；模块连接器不再读取文件系统。

`moduleSymbols` 格式为 `resume-one-module-symbols-v1`，保留 imports（local/module/imported，以及本地标识符的 localSpan）、exports（exported/local 或 module/imported）、declarations（独立 id、声明类型、可变性、初始化式证据）、原始位置、顶层顺序、副作用证据与阻断项。位置为原始 UTF-8 字节区间及一基字节列号。整个 import specifier 的 span 与别名 localSpan 不可互换。默认导出匿名函数有合成声明身份，函数表达式内部名字不泄漏为模块绑定；const 别名保持独立声明，不擅自执行初始化式。

输出 `moduleLinks`，格式 `resume-one-module-links-v1`：

- `requests`：所选模板入口的静态导入请求，保留实际 local/exported 映射及来源位置；不会仅搜索同名文本。
- 每项 `declarationResolved`、`trace`、成功的 `target` 或失败的 `blocker`。trace 保留每一步包名/版本、包指纹、文件指纹、查询符号、位置及 live/snapshot 导出模式。
- `statistics` 与非空请求集的 `allRequestedDeclarationsResolved`，均只指声明定位。
- `bindingValueProven`、`semanticsCompiled`、`moduleEvaluationProven`、`componentSemanticsCompiled`、`dependencyGraphComplete`、`migrationComplete` 均不因此变为 true。

即使唯一函数声明已找到，其 JS 绑定仍可能可变；目标中的 mutable 信息必须保留，不能把声明身份当作导出运行值稳定性证明。任何后续组件适配器还须证明固定来源及它自己的有限语义前提，不能仅凭 `declarationResolved` 清除组件阻断。

## 有限解析规则

1. 外部包仅选显式提供且唯一的包名；只沿[依赖源码包协议](theme-dependency-bundle-v1.zh-CN.md)已核对 SHA-256 的精确 exports 字符串/子路径文件，不猜 main/module、条件分支或扩展名。
2. 相对路径只在当前包快照内按词法消去 `.` / `..`，越界、node_modules、转义/编码分隔、URL、loader、反斜杠和空段拒绝。精确文件缺失不会用邻近 `.js` / `.jsx` 冒充。
3. 请求的显式具名/default 导出可沿重导出、局部 import 别名和局部声明追踪；未导出的同名私有函数不匹配。星号/namespace 不枚举，动态工厂、未知表达式和未完成初始化的默认快照引用不执行。
4. 保留 default 快照与具名 live 导出的差异。重复导出/局部绑定、未知声明命名空间、未支持 import attributes、语法错误、歧义或循环均阻断，不由目录或记录顺序选择。
5. 请求的显式导出可以在同一 barrel 另有未用且缺失的导出时定位成功；这不消除那个缺失项，不证明 barrel 可执行、依赖闭包完整或资源均可用。

模块证据限制：8 MiB 文件、100000 CST 节点、深度 128、8192 条累计记录、分析 3 秒。连接器限制：4096 个请求、16384 个累计解析步骤、每请求深度 64、10 秒；超限失败，不输出截断成功报告。最终输出及合并阻断仍受 16 MiB / 4096 项门禁约束。`--require-complete` 在未完成排版/组件/视觉验收时继续以退出码 3 拒绝；不会写入或覆盖近似主题。
