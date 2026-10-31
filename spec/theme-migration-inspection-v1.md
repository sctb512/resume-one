# 主题迁移：源码检查协议 v1

本规范定义独立工具 `resume-one-theme-migrate` 的只读源码检查前端，不是完整主题编译器或视觉还原度认证。声明式主题仍不得执行 JavaScript、网络请求或任意表达式。

## 命令与边界

```sh
resume-one-theme-migrate --source /absolute/path/to/theme --inspect
resume-one-theme-migrate --source /absolute/path/to/theme --inspect --require-complete
```

`--inspect` 仅接受绝对目录路径，和 `--out`、`--dry-run`、`--overwrite`、来源许可及版本覆盖参数互斥。工具读取来源，不执行、安装或下载任何主题代码，不渲染用户简历，不改变来源。包内示例 JSON 和字体仅作为静态来源资源纳入指纹。

正常输出为单行 JSON：`protocolVersion: 1`、`ok: true`、`result`。参数错误退出 2；读取、边界、解析预算或严格完整性错误退出 3，并输出 `ok: false` 与 `error.code` / `error.message`。非严格检查成功只表示报告生成成功。

## 来源快照

完整读取检查范围内的普通文件，包括深层组件、二进制字体、许可正文和包清单；不再以浅层文件名或颜色字符串推断主题已迁移。仅跳过名称为 `.git`、`node_modules`、`target` 的目录，报告列明这些排除项。相同名称的普通文件不被跳过。

限制为最多 16 层路径、1024 个文件、4096 个目录项、每文件 8 MiB、总计 64 MiB。超限整体失败，不将截断结果标成完整。拒绝来源根、祖先或内部的符号链接、特殊文件、不可移植路径、大小写冲突及读前读后检测到的变化。相对路径使用 `/`，组件为可移植 ASCII，拒绝 Windows 保留名、尾部空格/句点及危险分隔符。

文件按相对路径排序，分别记录字节数、SHA-256、是否 UTF-8；报告不嵌入二进制内容。整体 SHA-256 的输入依次为：

1. 字节域 `resume-one-source-bundle\0v1\0`（其中 `\0` 为一个零字节）。
2. 对每个文件：8 字节大端路径长度、UTF-8 相对路径、8 字节大端文件字节长度、原始文件内容。

`sourceSnapshot.complete` 仅表示这一明确范围内的快照读取完整，不表示外部依赖完整、更不表示迁移完成。元数据和文件身份复核可检测普通并发修改，但不构成对恶意并发写入的文件系统原子快照。

## 语法证据

固定解析器为 `tree-sitter 0.25.10`、本仓库固定的 `tree-sitter-javascript 0.25.0`、`tree-sitter-css 0.25.0`。JS grammar 原始发布包校验和与补丁范围见第三方许可；其余 registry 依赖版本与校验和在 Cargo.lock 中锁定。支持 JS / JSX / MJS / CJS 与 CSS；其他可能含样式或模板的格式必须显式列出适配器阻断项。

证据保留静态 import / re-export、require / 动态 import 调用、styled-components 别名、元素或组件基类、声明原顺序与重复属性、嵌套选择器、媒体/打印上下文、JSX 表达式与直接字段路径。注释及普通字符串中的假样式不参与声明计数。识别为样式不等于已实现作用域、CSS 级联或对应排版语义。

位置使用原文件字节区间 `[startByte, endByte)`、一基行号及 UTF-8 字节列号，`columnUnit: "utf8-bytes"`。源码片段最多 2048 字节，UTF-8 边界截取并明确标注 `truncated`；完整原文仍由快照指纹锁定，截断片段不得作为编译输入。styled CSS 包装解析的位置映射回原文件。

上游固定 JS grammar 会将 JSX 文本中的合法裸 `&` 识别为 ERROR（例如 `Awards & Honors`）。本仓库只在该 grammar 的 JSX 文本扫描器中修复这一点：非字符引用的 `&` 直接成为原始 `jsx_text`，合法字符引用仍由原 grammar 处理，位置与值均来自原始字节。不替换解析缓冲区、不忽略 ERROR、不作通用语法修复。依据为 [JSX 文本语法](https://github.com/facebook/jsx/blob/main/spec.emu)。报告的 `parsers.javascriptPatch` 固定为 `resume-one-jsx-ampersand-v1`。

动态插值、需解码的模板转义、非顶层绑定及未分类模板字符串不能静默退化为静态样式。语法错误必须保留 `syntaxValid: false` 及阻断项。单次解析预算 2 秒、每文件最多 100000 个命名节点、单类证据最多 12000 条、CSS 上下文最多 128 层；整包语法检查预算 30 秒，最多 4096 条阻断项，完整报告最多 16 MiB。超限失败，不截断后宣称成功。

## 依赖与状态

相对导入仅做有界词法路径与文件存在性检查，并不是 Node 的条件导出、扩展名或包解析器。越出根目录、缺失相对依赖和未解析表达式均阻断完整迁移；裸模块导入统一列入待锁定的外部依赖。CSS `@import` 不会发起网络请求。许可文件的存在与 package.json 的 license 声明不等于许可核验。

报告至少提供 `inspectionVersion`、`sourceSnapshot`、`parsers`、`package`、`statistics`、`cssProperties`、`externalImports`、`licenseFiles`、`syntaxEvidence`、`blockers`，以及独立状态：

- `migrationComplete`：完整主题语义编译与验收是否完成。
- `visualVerified`：固定条件下的原版对比是否通过。
- `licenseVerified`：来源及再分发许可是否核验。
- `executedSourceCode`、`sourceModified`：本前端始终为 false。

当前前端始终附带 `DECLARATIVE_COMPILER_REQUIRED` 和 `VISUAL_ACCEPTANCE_REQUIRED`，上述前三项均为 false。`--require-complete` 因而必须失败，不写主题文件；错误中的 `inspection` 保留完整报告。旧候选生成模式继续兼容，但结果显式标记 `migrationComplete: false`，严格模式同样拒绝生成或覆盖候选。

## 后续编译及验收约束

完整迁移必须依次解决固定依赖与资源、受限模板/样式语义、中间结构到共享排版能力的转换，再使用相同纸张、字体、输入样例对照原版验收。未映射规则须携带来源位置阻断交付；不得以手改成品主题文件代替工具转换，不得以主题数量、成功解析或配色相似宣称 100% 迁移。视觉误差指标、输入覆盖及人工检查应另行记录，不能由本报告虚构。
