# 迁移依赖源码包协议 v1

状态：独立迁移工具的只读依赖输入检查。目标是锁定显式选择的共享源码及缺口，不是下载器、Node 解析器、完整模块连接器或原版渲染验收。所选模板的有限声明追踪另见[模块声明连接协议](theme-module-links-v1.md)，不改变此处的闭包与验收状态。

```sh
resume-one-theme-migrate --source /absolute/theme --inspect --template-entry src/Resume.jsx --dependency /absolute/core --dependency /absolute/utils
```

## 输入与边界

`--dependency` 必须与 `--inspect` 使用，每次一个绝对 UTF-8 包目录，可重复但最多 8 个。工具不搜索父目录/node_modules、不执行包脚本、import/require 或字体加载，也不下载缺失包。操作系统对用户所选同步目录的透明行为不属于本工具发起的网络请求。

每包复用[完整源码快照协议](theme-migration-inspection-v1.md)，符号链接、不可移植路径和并发变化等同样拒绝。所有依赖原始文件合计最多 64 MiB，单个根包快照仍受原有边界限制。包依次读取，指纹只证明对应快照内容，不是多个目录的文件系统原子快照。

根 package.json 最多 256 KiB，必须为对象，禁止重复键（含转义后的同名键），最多 4096 个键。name 采用有界、小写 npm 名称子集，version 为明确的 SemVer 版本而非范围、tag、workspace 或 link。同名依赖即使版本不同也拒绝；主题来源包与依赖同名同样拒绝，不能靠目录顺序挑选身份。

## 导出与导入

仅检查 exports 的单个字符串，或显式 `.` / `./subpath` 到字符串的映射。目标必须为普通 `./` 包内文件路径，不能包含越界、编码分隔、反斜杠、URL/盘符、query/hash loader、wildcard 或 node_modules 路径。条件对象（包括 default/import）、数组、mixed map 和通配规则均明确未支持；无 exports 时不猜测 main/module 的 Node fallback。

| 导出状态 | 含义 |
|---|---|
| `file-present` | exports 指定的精确文件存在于同一静态快照，带文件 SHA-256；不代表可执行、已编译或依赖完整 |
| `missing` | 显式目标文件缺失，不用其他扩展名或目录入口冒充 |
| `unsupported` | 目标/分支/路径不在有限规则内，未选择或执行 |

JS/JSX/CSS 由现有语法前端读取。显式相对 import 仅对快照内精确路径或候选进行有界词法检查；多个扩展名候选保持歧义、单个补扩展候选也不冒充 Node 解析。外部导入保持未连接，即使另一输入包名称相同；require/dynamic import 不假定未被遮蔽或已执行。未知源码格式、CSS import、动态样式与错误均保留阻断。资源分析未完成，不因观察不到 import 就认定没有字体、图片等依赖。

## 输出与完成状态

根检查报告的 `dependencyBundle` 包含：

- `dependencyBundleVersion: 1`、输入范围、限制、包数与原始总字节数。
- `fingerprints`：按包名排序的 name、version、root、完整来源 SHA-256 与 package.json SHA-256。
- `packages`：每包的完整来源快照、声明元数据、exportsEntries、导入来源与候选、语法证据、不支持格式及阻断项。
- 外部导入汇总与带 packageName 的阻断项；不将声明依赖版本当成已锁定的传递依赖。
- `dependencyGraphComplete`、`rootExternalImportsResolved`、`originalRenderingVerified`、`migrationComplete`、`visualVerified`、`licenseVerified` 当前均为 false。
- `executedSourceCode`、`sourceModified`、`networkAccessed` 始终为 false（指本工具执行的动作）。

依赖报告预算 30 秒、16384 条单包导入记录、4096 条汇总阻断、16 MiB 输出；来源+模板+样式+依赖的最终合并报告也必须不超过 16 MiB 和 4096 条阻断。超限或身份/快照错误返回失败，不输出截断的成功结果。

CLI 的包输入错误为退出码 3 / `DEPENDENCY_BUNDLE_ERROR`，`error.diagnostics[].code` 保留具体原因；来源同名冲突为 `DEPENDENCY_DUPLICATES_SOURCE`。`--require-complete` 仍因依赖/编译/验收未完成而失败，完整已生成报告保留于 `error.inspection`。源文件存在、导出可定位及快照读取完整不是高保真迁移完成的证据。
