# 尽量还原的主题适配协议 v1

[English](./theme-adaptation-v1.md) · **简体中文**

## 目标与边界

独立迁移工具的 `--adapt` 是显式近似适配模式，与严格编译检查分开。不执行、安装或导入第三方 JS、React、模板、helper、CSS loader；不请求网络资源，不写简历。保留现有声明式 v2 的安全边界，不把 HTML、CSS 或 br 标签塞进简历数据。

工具从完整有界目录快照及 CSS/JSX 语法证据提取可表达的文字角色、配色、版面、模块标题与顺序。组件名/简单选择器到角色的映射属于记录来源的推断，不是完整 DOM 匹配、CSS 级联或原模板程序等价证明。所有 12 个标准 JSON Resume 模块保留；主题不支持的自定义字段仍留在 JSON，不承诺渲染未知扩展。

同文件静态 `createGlobalStyle` 的无条件 `:root/html` 字面变量可供本文件组件使用，记录变量来源；不会把普通组件的局部变量提升为全局，也不据此证明组件挂载与跨文件级联。单一模块的外边距不能覆盖所有模块的全局间距；原零/极小模块间距依赖未迁移盒模型时，采用明确记录差异的 4mm 安全间距，避免标题与前一模块重叠。

逻辑边距长属性按横排、从左到右映射到四个物理页边，并记录方向近似。模块纵向物理/逻辑外边距可投影为统一间距，不求和或模拟 margin collapse；标题的 `border-block-start/end`、`border-inline-start` 可投影为相应内置分隔线，粗细、线型、精确位置及 writing-mode 不等价。未支持的逻辑简写或其它属性不会变成额外的页边距字段。

预览与导出均以纸面样式为适配目标。仅展开确定适用的 `@media print`、`all`、`only print`，支持这些类型与 `screen` 的静态逗号列表；保留原声明、重要性、来源范围及同块源码顺序。含宽度等特征条件、`not`、未知媒体类型、`supports/layer/keyframes`，或任一外层条件未确认时，不展开。`@page` 保留为纸面作用域。报告的 `printProfile` 明确记录投影及跳过规则；跨文件顺序仍为推断，不宣称实现完整 CSS 级联。此投影仅用于 `--adapt`，不改变严格检查结果。

文字继承优先使用明确的文档容器。段落、描述等局部规则只影响相应正文角色，不能变成姓名、标题的继承来源，也不能把局部内距当作纸面边距。支持有界的类名前缀识别，如 `.pro-name`；同时声明字体、字号、宽度及内距的单个未知类选择器可推断为文档容器，须记录依据。单纯用于模块标题对齐的 `aside` 不构成整页侧栏；侧栏需要明确命名或宽度、底色等结构证据。未知列名同时声明 `left/right/sidebar` 网格区域和 20–40% 列宽时可推断为侧栏，并记录依据；仅有方向名称或对齐不能触发。模块与侧栏的通用标题规则不能覆盖身份姓名。这些仍为静态角色投影，不是完整 DOM 计算样式。

实际 JSX 节点和字面 HTML 的纯文本模块标题可提供名称、顺序和标题层级证据；注释、脚本、属性内的伪标签及动态标题不会成为模块。唯一且至少覆盖两个模块的原生标题层级可用于识别模块标题；不同层级混用时不强行合并。源码顺序取包含最多不同标准模块的单一文件，未列出的标准模块仍追加；不证明运行时分支、组件挂载或绑定。解码后的 JS 字面量仅记录原片段起点及解码内偏移，不冒充精确源码字节范围。

无法表达的媒体规则、动态样式、伪元素、图标、布局、字体或分页等，须保留差异与来源。自动可读性替代、数值默认值、字段语义替代必须记录，不得静默改写后称原版一致。

## 静态数值与字体上下文

适配器支持有界的 `calc()`、`min()`、`max()`、`clamp()` 数值投影、括号和嵌套表达式。长度单位为 `pt/px/mm/cm/in/pc/em/rem/%`，其中 `1px = 0.75pt`；只允许可判定的维度运算，例如长度相加、长度乘除无量纲数。二元 `+/-` 两侧须有 CSS 要求的空白。拒绝未知单位/函数、未解析变量、长度乘长度、除零、非有限数及最终非零无单位长度。单个表达式最多 8192 字节、32 层嵌套、512 个节点，函数参数最多 64 个；表达式解析不表示完整 CSS 有效性或级联已获认证。

根字号、body 字号和推断的文档容器字号分别计算：`:root/html` 根字号以 12pt 初始值解释；body 的相对字号以根字号为父上下文；文档容器以 body 为父上下文。`rem` 始终引用根字号，字号中的 `em/%` 引用对应推断父字号，局部段落字号不能改写其它角色的根或父上下文。支持的静态上下文范围为 0.1–96pt，最终输出仍须满足 v2 的字号、行高和字距限制；根字号声明无法求值或超出上下文范围时采用 12pt 并记录 `ROOT_FONT_SIZE_CONTEXT_UNRESOLVED`。`font-size-context` 记录根/文档字号与初始值假设，不证明任意 DOM 祖先链。继承的绝对行高、字距在进入条目子组件前换算为绝对长度，不能随子字号再次缩放；无单位行高保留倍率语义。

百分数仅在具有明确字号参考的投影中使用，不能把该参考当作页面或容器宽度。页边距、文档内距及条目行前后距不按字号换算百分数；依赖容器宽度、视口或任意动态表达式的值仍保留为未支持差异。已有的同文件字面变量解析与数值求值是分开的有界步骤，不执行 JavaScript。

## 纸面盒子组合与首行定位

各边的 `@page margin`、`html padding`、`body padding` 和单一推断文档容器的 `padding` 分组选择声明后相加，而不是把所有声明争用一个页边距 token。多个文档容器候选只选一组，并记录 `DOCUMENT_PADDING_CONTAINER_INFERRED`；不假设它们同时挂载或互为祖先，不叠加局部段落、子节点和备选布局的内距。各组的声明选择保留现有静态优先级规则，不构成完整 CSS 级联或盒模型模拟。结果逐边记录 `page-box-composition`，包含输入盒子、来源、毫米值及最终值。

例如，上边的 `@page margin: 19.05mm` 与文档容器 `padding-top: 7.94mm` 组合为 `page.marginsMm.top: 26.99`。参与组合的每项须为 0–35mm，最终值须为 3–35mm；负值、百分数、无法求值或组合结果越界时采用 16mm，并记录 `NUMERIC_VALUE_UNREPRESENTABLE`，不放宽 Schema 边界，也不猜测容器宽度。任意嵌套盒子、margin collapse 和原网页分页不在此组合契约内。

存在已提取样式声明时，适配输出显式设置 `page.firstLinePosition: "line-box"`。共享渲染器从行框顶部推进，按当前行真实字面的 `(lineHeight + ascent - descent) × sizePt / 2` 向下定位基线。`ascent/descent` 为 em 单位的上升部及正值下降部。混合字号的连续行分别定位，推进量取行框高度。主栏、侧栏、分栏页头及续页使用独立起点。字段的默认值和渲染语义见[主题规范](./resume-one-theme-spec-v2.zh-CN.md#纸张起点与首行定位)。

行框模式的顶部安全保护按字面可能超出行框的上溢量留白，不再额外叠加完整 ascent；当前适配阶段使用内置回退字面度量，必要调整仍记录 `BASELINE_TOP_CLEARANCE_ADAPTED`。加载真实资源后的最终样本检查核对实际字面上下缘，不能把该保护当成原版排版等价证明。DOCX 保留组合后的物理上边距，使用 Word 原生可编辑行框，不另加 ascent 空白段落或再次增大边距；其字形定位、换行和分页仍由阅读器决定，未认证与 PDF/原网页逐像素一致。

## 真实字段绑定与条目行分组

`styles.entryTemplates` 从工具拥有的模板 IR 提取候选，追踪真实模块集合、`map` 项的作用域与绑定身份、受支持的直接别名/解构以及同一条目的字段读取。字段同名、组件同名或字符串中出现模块名均不足以建立绑定；遮蔽变量、索引参数、跨条目字段和未解析调用不得猜测为当前条目。原生标签及已链接到原生标签的静态样式组件提供源码分组线索；`br` 可作为分行边界，不写入简历 JSON。

仅将可用、选择无歧义且每行单一样式来源的候选投影为 `sections.<模块>.entryHeader`，保留受支持的源码字段顺序、纯文本前后缀、行前后距和本地静态文字样式。遵守 v2 每条目页头 1–8 行、每行 1–6 个字段、同一字段不重复等限制；混合行内样式、多个同模块候选或超出有界子集时保留标准模块排版与明确差异，不截断字段冒充完成。正文、要点和 URL 继续走标准内容层，原数据与条目索引不变。支持的缺字段条件仅限已识别的同字段存在性等有限模式；不能表达的标点/条件变化须记录，不能把保留源码顺序称为保留全部运行时语义。

候选的 `fieldBindingsVerified/sourceOrderVerified` 只证明有界字段绑定和源码顺序；`computedLineGroupingVerified/styleInheritanceVerified` 仍为 `false`。分组未计算完整 CSS 的 `display`、行内换行、运行时条件或组件语义，因此不是原页面实际行分组、完整样式继承或视觉等价认证。`styles.entryTemplates` 整体的 `layoutCompiled/visualVerified/migrationComplete/equivalent` 不因使用候选而变为 `true`；样式报告的 `layoutValidated: true` 仅表示共享渲染器接受生成的声明式配置。

同一文件中，由绑定图连接到同一元素的 `styled(Base)` 样式层按基类到派生类合并静态声明，再求该元素的相对字号。DOM 祖先链与样式层分别处理。文本或字面片段经过不同组件、祖先样式或属性时，整行按混合样式保留差异。

`@jsonresume/core` 的 `DateRange` 仅在真实导入绑定可核对、`startDate/endDate` 均直接读取同一条目原字段且没有额外选项或子节点时，显式适配为 `dateRange` 与 `dateFormat: "short-en"`。它使用声明式渲染器有限的日期表示，不运行上游格式化器；记录 `DATE_FORMATTER_ADAPTED`、导入/参数证据及依赖快照匹配状态，`formatterImplementationVerified/dateSemanticsVerified/equivalent` 保持 `false`。即使固定依赖快照匹配，也不证明任意日期字符串、地区、包初始化或日期样式等价。此路径属于 `--adapt` 的有损适配，不能使严格迁移或 `--require-complete` 通过。

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

来源 package.json 的作者声明可安全表示时写入 `provenance.upstreamAuthor`，原始作者和仓库声明仍留在报告中；批量包 NOTICE 同时记录作者及可表示的 HTTP(S) 原项目地址。仅解析静态元数据，不联网核验或暗示上游背书，不把包含邮箱的 npm 人员声明原样作为显示名。

## 字体资料库

显式提供 `{ "formatVersion": 1, "fonts": [...] }`，最多 128 个静态字面。每项包含 `family`（原 CSS 请求字族）、`weight`、`fontStyle`、绝对 `path`、原字节 `sha256`、许可声明 `license` 与许可正文绝对路径 `licensePath`。

已依法改名的静态字面可显式给出 `fontFamily` 表示其真实内部字族。该别名关系必须在结果中记录；工具校验元数据、真实字重/斜体与字节指纹，不据此证明别名两端字形来源等价。没有该字段时，真实字族必须和 `family` 完全相同。不得自动做模糊字族匹配、近似字重替换或安装系统字体。

DOCX 每个字族的 regular/bold/italic/boldItalic 各有一个嵌入槽位。例如同族 400 与 500、600 与 700 同时使用会发生槽位冲突，导出检查会拒绝。资料库制作者可在许可允许且保留完整来源、原始摘要及修改说明的前提下，预先为各真实静态字面设置独立内部字族，显式填写 `fontFamily`；应验证字形、字重和度量未变。仅改 JSON 中的别名而不改真实字体元数据无效。迁移器本身不改字体二进制，仍逐字节复制已验证的资料库输入。

资料库不超过 1 MiB，每个字体不超过 8 MiB，许可正文不超过 1 MiB；拒绝符号链接、非绝对路径、可变字体、集合、WOFF/WOFF2、缺失许可正文、指纹不符与读取期间变化。先验证整份显式资料库，再选取最多 4 个实际字面，优先正文、姓名、模块标题、条目标题，其后副标题、摘要、日期、辅助信息。缺失字面或超过 4 个的角色保留明确的内置替代，并记录差异。复制原字体和许可字节，不偷偷修改字体或许可证。

生成的字体请求保留 CSS 字族栈 `families`（最多 16 项）。依原顺序选择资料库中首个字族、真实字重及斜体均准确匹配的字面；遇到通用字族或系统字体占位符即停止，不越过它选择后面的字体。采用第二个及后续字族时记录 `FONT_STACK_FALLBACK`、跳过的字族及实际选择，不等同于首选字族；不做近似字重替换。旧请求没有 `families` 时仍仅按原 `family/weight/fontStyle` 精确匹配。

每个字体资料库条目可提供 `notice` 纯文本（最多 16384 UTF-8 字节，仅允许换行、回车和制表控制符），用于保留原始/静态输入/最终命名字面摘要、来源版本和命名转换说明。选中的实际字面按文件摘要去重，说明原文进入 `fonts.notices` 和包内 NOTICE；不进入主题样式或简历。工具将它标为未独立核实的输入声明，不把携带说明等同于验证字体来源或许可；库制作者仍须保存及核验原始材料。非法或超长说明在修改候选前拒绝。

## 自动验证与状态

每项总是验证 schema、资源指纹与共享渲染器能力。`--verify-fixture` 显式读取不超过 1 MiB 的公开/脱敏 JSON Resume 样本，使用内置 schema，不自动使用来源中的简历样例。报告只保存样本路径、指纹、页数、文字行数与模块目标计数，不保存简历正文。

布局检查拒绝非有限坐标、越出纸面的文字与消失的标准模块，核对样本原字节未变。`--verify-exports` 额外生成 PDF/DOCX 字节并检查格式头，验证序列化路径可用；不替代独立阅读器解析、逐页视觉检查或 Microsoft Word 实机测试。

结果保留 `visualVerified:false`、`licenseVerified:false`、`exactMigrationComplete:false`。适配成功不复用严格迁移的“完整等价”状态；`--adapt` 与 `--require-complete` 冲突。严格模式仍拒绝缺少完整编译与视觉验证的主题。

最终纳入内置目录前还需人工/可视复核差异、主题辨识度、可读性和真实导出。仅有主题数量、schema 通过或大面积白底的像素相似度，不能代替验收。
