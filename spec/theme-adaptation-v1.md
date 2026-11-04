# 尽量还原的主题适配协议 v1

## 目标与边界

独立迁移工具的 `--adapt` 是显式近似适配模式，与严格编译检查分开。不执行、安装或导入第三方 JS、React、模板、helper、CSS loader；不请求网络资源，不写简历。保留现有声明式 v2 的安全边界，不把 HTML、CSS 或 br 标签塞进简历数据。

工具从完整有界目录快照及 CSS/JSX 语法证据提取可表达的文字角色、配色、版面、模块标题与顺序。组件名/简单选择器到角色的映射属于记录来源的推断，不是完整 DOM 匹配、CSS 级联或原模板程序等价证明。所有 12 个标准 JSON Resume 模块保留；主题不支持的自定义字段仍留在 JSON，不承诺渲染未知扩展。

同文件静态 `createGlobalStyle` 的无条件 `:root/html` 字面变量可供本文件组件使用，记录变量来源；不会把普通组件的局部变量提升为全局，也不据此证明组件挂载与跨文件级联。单一模块的外边距不能覆盖所有模块的全局间距；原零/极小模块间距依赖未迁移盒模型时，采用明确记录差异的 4mm 安全间距，避免标题与前一模块重叠。

逻辑边距长属性按横排、从左到右映射到四个物理页边，并记录方向近似。未支持的逻辑简写或其它属性不会变成额外的页边距字段。共享渲染器按首行基线定位：原 CSS 上内距不足以容纳文字上缘时，按内置字面度量增加必要留白并记录差异；最终样本检查包含实际字面的上下缘。

无法表达的媒体规则、动态样式、伪元素、图标、布局、字体或分页等，须保留差异与来源。自动可读性替代、数值默认值、字段语义替代必须记录，不得静默改写后称原版一致。

## 单个适配

```sh
resume-one-theme-migrate --source /absolute/source-package --adapt --out /absolute/example.theme.json --verify-fixture /absolute/public-en.json --verify-fixture /absolute/public-zh.json
```

`--dry-run` 不写文件；覆盖现有文件仍须 `--overwrite`。来源必须为有界普通目录，沿用源码快照的文件数、大小、路径、排除目录及读取期间变化检查。目标与父路径拒绝符号链接。单文件模式不写字体资源；带资源的主题须使用批量目录模式。

## 批量目录包

```sh
resume-one-theme-migrate --source /absolute/catalog.json --adapt --batch --out /absolute/new-output-directory --font-library /absolute/fonts.json --verify-fixture /absolute/public-en.json --verify-fixture /absolute/public-zh.json --verify-fixture /absolute/public-long.json --verify-fixture /absolute/public-missing.json --verify-exports
```

清单为最多 4 MiB 的 JSON，`items` 含 1 至 256 项。每项必须有唯一 `slug` 与绝对 `sourceDirectory`；可含 `license`、`licenseFile`、`upstreamUrl`、`upstreamRevision`。slug 为小写 ASCII 字母、数字与连字符，首字母/数字，最长 53 字符。输出主题 id 和目录均加 `community-` 前缀，避免与应用原生主题冲突。

目标目录须尚不存在，不能和任一来源目录互相包含。不替换既有本地主题，不跳过失败项冒充全量完成。每项生成 `theme.json`、`NOTICE.txt`、`MIGRATION.json` 及引用到的静态字体和许可正文。主题先以暂存文件加载与验证，通过后才发布 `theme.json`。失败项没有最终主题清单，仍在汇总报告记录失败原因；已生成的其它包保留，整个命令返回 `ADAPT_BATCH_PARTIAL` 与退出码 3。

`--jobs 1` 至 `--jobs 4` 控制有界并行，默认 1；各项独立验证和发布，汇总始终保持输入清单的顺序。只对完整快照中真实 JS 字符串和无动态插值的 style 区域做嵌入 CSS 提取，保留源文件、字节范围与摘要；不执行模板插值或把注释当源码。遇到解析器不完全支持的 CSS，近似模式只采用可独立重新解析的声明并保留明确差异，不改变严格检查结果。

JS 字符串转义按受限字面解码处理；不拼接跨插值片段。动态 HTML 中完整且自身无插值的 `<style>` 区域仍可读取，区域外的表达式不会执行。声明依赖 Twind/Tailwind 的包可对固有 JSX 标签的静态 `className` 采用显式列举的常见文字工具类约定；输出原属性字节范围、已投影类名与未解析类名。自定义颜色、插件、任意值、动态组合及响应式变体不会猜测，`configurationVerified/equivalent` 保持 `false`。

汇总写入 `migration-report.json`。`allGeneratedAndValidated` 只表示每项均生成并完成所要求的自动检查，不等于原版视觉等价、许可核验或 Microsoft Word 验收。

## 字体资料库

显式提供 `{ "formatVersion": 1, "fonts": [...] }`，最多 128 个静态字面。每项包含 `family`（原 CSS 请求字族）、`weight`、`fontStyle`、绝对 `path`、原字节 `sha256`、许可声明 `license` 与许可正文绝对路径 `licensePath`。

已依法改名的静态字面可显式给出 `fontFamily` 表示其真实内部字族。该别名关系必须在结果中记录；工具校验元数据、真实字重/斜体与字节指纹，不据此证明别名两端字形来源等价。没有该字段时，真实字族必须和 `family` 完全相同。不得自动做模糊字族匹配、近似字重替换或安装系统字体。

资料库不超过 1 MiB，每个字体不超过 8 MiB，许可正文不超过 1 MiB；拒绝符号链接、非绝对路径、可变字体、集合、WOFF/WOFF2、缺失许可正文、指纹不符与读取期间变化。先验证整份显式资料库，再选取最多 4 个实际字面，优先正文、姓名、模块标题、条目标题，其后副标题、摘要、日期、辅助信息。缺失字面或超过 4 个的角色保留明确的内置替代，并记录差异。复制原字体和许可字节，不偷偷修改字体或许可证。

## 自动验证与状态

每项总是验证 schema、资源指纹与共享渲染器能力。`--verify-fixture` 显式读取不超过 1 MiB 的公开/脱敏 JSON Resume 样本，使用内置 schema，不自动使用来源中的简历样例。报告只保存样本路径、指纹、页数、文字行数与模块目标计数，不保存简历正文。

布局检查拒绝非有限坐标、越出纸面的文字与消失的标准模块，核对样本原字节未变。`--verify-exports` 额外生成 PDF/DOCX 字节并检查格式头，验证序列化路径可用；不替代独立阅读器解析、逐页视觉检查或 Microsoft Word 实机测试。

结果保留 `visualVerified:false`、`licenseVerified:false`、`exactMigrationComplete:false`。适配成功不复用严格迁移的“完整等价”状态；`--adapt` 与 `--require-complete` 冲突。严格模式仍拒绝缺少完整编译与视觉验证的主题。

最终纳入内置目录前还需人工/可视复核差异、主题辨识度、可读性和真实导出。仅有主题数量、schema 通过或大面积白底的像素相似度，不能代替验收。
