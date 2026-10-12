# Resume One 声明式主题规范 v2

状态：v2 文件结构及校验规则在 Rust 共享核心中实现；PDF 已支持单栏基础视觉参数，其余主题排版、桌面排版器与迁移器仍在开发。配套文件：[JSON Schema](./resume-one-theme-v2.schema.json)与[结构示例](./resume-one-theme-v2.example.json)。目标是在不执行主题代码、不污染 `resume.json` 的前提下，让内置、本地下载和社区迁移主题共用一种文件格式，并尽可能保留原主题的视觉特征。

## 1. 为什么要自定规范

[JSON Resume 官方 Schema](https://github.com/jsonresume/jsonresume.org/tree/master/packages/schema)规范的是**简历内容**。其[主题开发说明](https://www.jsonresume.org/theme-development)和[当前主题脚手架](https://github.com/jsonresume/jsonresume.org/blob/master/docs/CREATING_A_THEME.md)定义的是 npm 包、`render(resume)` 与 HTML/React 渲染约定，并没有要求社区发布统一的、可安全解释的声明式布局 JSON。旧[官方样板主题](https://github.com/jsonresume/jsonresume-theme-boilerplate)使用 JS、Handlebars 和 CSS，也说明现有主题形态并不统一。[Reactive Resume](https://github.com/reactive-resume/reactive-resume)有自己的模板和样式规则，但它属于另一应用的数据与渲染模型，不能直接视为 JSON Resume 主题通用规范。

因此本规范**不是 JSON Resume 官方标准**，也不声称兼容现有 npm 主题包的执行接口。它只规定 Resume One 可以安全加载的主题文件。`jsonresume.org/themes` 仅供独立迁移工具发现原主题；主应用的在线列表只接受迁移完成、通过校验的声明式文件，未迁移项目不出现，不能把包名或截图伪装成可应用主题。

阅读顺序：先看下方的最小文件，再按字段表添加布局细节；[JSON Schema](./resume-one-theme-v2.schema.json)是机器校验基线，本文规定跨字段语义。主题作者不需要懂 Rust、GPUI、CSS 或模板语言。

```json
{
  "formatVersion": 2,
  "id": "calm-paper",
  "name": "静纸",
  "version": "1.0.0",
  "license": "MIT",
  "layout": "classic",
  "colors": { "accent": "#52756B", "seal": "#A14F40", "paper": "#FFFEFA", "canvas": "#F6F3ED" },
  "modules": ["basics", "work", "education", "skills"]
}
```

## 2. 文件与兼容性

- 文件名：根目录 `*.theme.json`，或主题子目录中的 `theme.json`。内置、本地、在线下载后的文件形状完全相同；在线 GitHub 清单 `themes/index.json` 只负责列出路径，不是另一种主题格式。
- v1 `formatVersion: 1` 的 `id/name/description/layout/colors/modules` **继续有效**，其可运行的机器定义见 [v1 JSON Schema](./resume-one-theme-v1.schema.json)；解析器把 v1 在内存中规范化为 v2 的默认值，不静默改写原文件。
- v2 仍保留上述字段和含义，只新增版本、来源、纸张、字体、区域、模块呈现与受限资源字段。`layout` 四个旧值继续作为兼容回退样式；有 `composition` 时，v2 布局器优先使用它。
- `formatVersion` 是解析协议版本；`version` 是单个主题的语义版本。未知协议版本、未知字段或不认识的枚举值应报错，不猜测、不执行。与未来 v3 不兼容时仍可保留文件与来源信息。
- `resume.json` 只包含 JSON Resume 内容字段。已应用主题 ID、主题目录、来源、预览缓存和迁移映射留在应用本地状态；主题切换不得重写简历。
- 主题文件是 UTF-8 JSON，不含注释或尾逗号。`id` 在同一源内唯一；跨来源身份为「源 + 固定版本/commit + id」，不能只靠名字。文件名不决定 ID；清单 ID 必须与文件 ID 相同。
- 在线清单的机器定义见[主题源索引 v1 Schema](./resume-one-theme-source-v1.schema.json)。清单 `name` 必须与 manifest `name` 一致；`manifest` 为相对清单文件的仓库内路径，不能跳出仓库；`sha256` 如有填写，必须对原始 UTF-8 文件字节计算并在解析前验证。重复 ID 拒绝整份清单。

## 3. 字段语义

| 字段 | 作用 | 约束 |
|---|---|---|
| `id`, `name`, `version`, `license`, `provenance` | 身份、署名与迁移来源 | 主题 ID 在一个来源内唯一；来源版本固定；不得以不明许可证再分发 |
| `layout` | v1 兼容回退布局 | `classic/rules/minimal/modern` 四选一 |
| `colors` | 文档视觉色 | 原有 `accent/seal/paper/canvas` 必需；可加 `ink/muted/border/sidebar`；应用外壳颜色由渲染器从这些 token 派生，主题不可直接改工具栏几何 |
| `modules` | 右侧可新增模块 | 只允许 JSON Resume 的 12 个标准模块；不改变内容 Schema；已有但主题未列出的数据仍须保留并用保底排版显示 |
| `page` | 纸张及分页约束 | A4/Letter、毫米边距、模块间距、防拆条目和标题跟随首项；同一语义布局供预览与 PDF 使用 |
| `typography` | 正文、姓名、模块标题、辅助文字 | 受限字体族、磅值、行高、字重、字距；无 CSS 字符串；CJK 与拉丁字体有明确回退 |
| `composition` | 纸张区域与页眉 | 单栏、左右侧栏或分割页眉四种预设；侧栏宽度和间距受限；`placements` 只把标准模块放到主栏/侧栏，同一区域内保留 Resume JSON 的模块顺序 |
| `sections` | 各模块的外观 | 标题、列表/时间线/卡片/标签及日期位置由有限枚举选择；空模块默认不渲染；字段意义和文字由应用统一解释 |
| `decorations` | 有界装饰 | 印章、细线、色带和有限圆角；不能内嵌 HTML/SVG/CSS 或任意绘图命令 |
| `assets.fonts` | 可选本地字体 | 仅主题包内相对路径的 TTF/OTF，附 SHA-256 与许可证；解包、尺寸和许可另行校验；无网络字体 |

对应的[示例文件](./resume-one-theme-v2.example.json)演示了侧栏、模块放置、字体层级和时间线。它只是**结构示例**；在桌面布局器及 PDF 一致性验收完成前，不得把它列为可完整应用的主题。

### 省略字段的确定性默认值

| 省略项 | 解释 |
|---|---|
| `description`, `provenance` | 空描述、无迁移来源；不影响渲染 |
| `page` | A4，四边 16 mm；模块间距 6 mm；条目不跨页；标题跟随首项 |
| `typography` | 正文 `cjk-sans` 10 pt/400/1.4；姓名 `cjk-serif` 26 pt/600/1.2；模块标题 `cjk-serif` 13 pt/600/1.3；辅助文字 `cjk-sans` 8.5 pt/400/1.3；字距 0 |
| `composition` | `single-column`、`stacked` 页眉、`inline` 联系方式；全部模块在主栏，顺序按 Resume JSON 顶层顺序 |
| `sections` | 所有模块 `plain` 标题、`stack` 内容、`inline` 日期；缺失字段不渲染占位符 |
| `decorations`, `assets` | 无装饰、无外部资源；模块分隔由应用默认样式处理 |
| `colors.ink/muted/border/sidebar` | 由基础四色计算，且由应用按对比度规则修正；不从远端加载 |

`layout` 始终必填，供 v1 兼容排版回退；显式 `composition` 优先于它。显式字段只覆盖对应默认值，不影响未提及字段。预览、PDF 和 DOCX 使用同一规范化后的主题语义；DOCX 的分页仍允许与 PDF 不同。

`provenance` 只做元数据，不在主题列表弹出提示：`native` 表示为本规范原创，`adapted` 表示按已注明来源改编，`inspired` 表示仅借鉴视觉方向。迁移版应写 `upstreamUrl/upstreamRevision/upstreamAuthor/upstreamLicense/migratedBy` 中可证实的信息；是否通过验收记录在迁移工具的发布报告中，不用 `fidelity` 冒充审核状态。

## 4. 渲染器的固定语义

1. 布局器负责解析 JSON Resume、字段安全转义、本地化模块标题、日期格式、URL 安全校验和缺失字段处理。主题只能选择预置排版构件及参数，不能提供模板语言、表达式、回调或脚本。
2. `basics` 永远是身份和联系方式的数据来源；主题可选择页眉摆法，不可把姓名、邮箱等复制到主题文件。其他模块用标准 ID 和固定字段映射；未知 Resume JSON 扩展字段原样保存，未有专门构件时不伪造展示。
3. `modules` 控制新增候选，不控制删除现有内容。切主题或从远端更新主题时，任何内容不得被自动移除。
4. 同一 `ResumeDocument + ThemeManifest` 生成交互预览和无控件打印布局。PDF 以无控件布局为准；DOCX 和纯文本使用同一语义内容顺序，但 Word 的字体替换与重排可能使它们**不能承诺逐像素等同**于 PDF。
5. 主题的 `colors` 同时导出应用外壳的语义色，但正文可读性、焦点和高对比模式由应用最终裁决；主题不得降低可访问性约束。

## 5. 安全与验证

配套 [JSON Schema](./resume-one-theme-v2.schema.json)先检查形状、白名单字段、枚举和数值边界；正式版还必须做以下语义检查：

- `sidebar-*` 以外的布局不得使用侧栏位置；侧栏布局至少要有一个标准模块放进侧栏；`asset:<id>` 必须引用确实存在且 hash 匹配的本地字体；不同资源 ID 不得重复。
- 时间线只可用于有时间段的标准模块；`chips` 只用于技能、语言或兴趣等可枚举内容。不能因为不兼容就丢字段。
- 文件和资源的真实路径须经规范化后仍在主题目录内；限制清单大小、字体大小、总下载量、并发、解码时间和缓存容量。只允许经验证的字体；不加载远程 URL、外部图片、JS、HTML、CSS、Handlebars 或任意 SVG。
- `provenance.upstreamUrl` 只是署名/打开来源的元数据，渲染时不得发请求，更不能把用户简历传给主题源。
- 验证失败保持上一个已应用主题；错误留在来源管理/诊断界面，不把无效条目当作可试穿主题。不要静默降级成另一排版后假称“迁移成功”。
- 主题列表不用远程图片或官方截图。内置、本地、在线都以当前 Resume JSON 在本机同一渲染器试穿；悬停只改变屏幕画面，离开恢复，点击才应用。右侧模块始终属于已应用主题。
- 内置主题是随安装包提供的**可定位真实文件**，不藏在可执行文件常量中；它们与用户目录主题走同一解析、校验和渲染路径。右键主题可在 Finder/资源管理器中显示其文件；若安装目录只读，用户仍可查看或复制，但编辑应在自己的主题目录进行。

## 6. 从现有主题迁移的路径

| 原主题形态 | 可自动完成 | 需人工处理 | 结果 |
|---|---|---|---|
| 已是 Resume One v1 声明式文件 | 字段读取、默认值规范化、Schema 检查 | 无需重画 | 原文件直接用，未来可显式升级 v2 |
| JSON Resume npm/JS/React/Handlebars | 固定来源、提取元数据/颜色/字体/资源、用公开样例截图和测量 | 对应区域、模块呈现、分页与中英文排版，许可证核对 | 可解释的 v2 迁移版；非原包运行时 |
| 社区纯 CSS/HTML 模板 | 静态抽取尺寸、色值、字族和主要 DOM 分区 | 选择受限构件、清除模板逻辑、检查动态条件 | 能表达的部分迁移；超出能力明确标记差异 |
| 依赖交互脚本/网络/任意计算 | 仅做目录发现和风险标记 | 改设计或放弃 | 不支持原样执行 |

迁移器是**单独交付的工具** `resume-one-theme-migrate`，不内置在 GUI 或面向 agent 的 `resume-one` CLI 中。它只在独立隔离环境分析第三方代码，且只喂公开中英文测试简历；正式应用、CLI、主题预览和 PDF 导出均不得执行第三方代码。迁移器产出候选 `*.theme.json`、来源/许可证记录与差异报告，再由人用短/长、中/英、全模块、缺字段、多页 A4/Letter 样例验收；未通过验收的候选不发布到在线目录。视觉差异无法通过自动提取全部解决，不能承诺一键无损；许可证不明时不再分发。

## 7. 发布源合规性

正式版默认在线源必须是真实、公开、已迁移和已验收的声明式 GitHub 仓库。构建时锁定 commit，并逐项检查清单、文件 SHA-256、来源许可、视觉/PDF 回归与离线回退；缺失即阻断发布。主应用不执行原代码主题，也不把官方主题目录或示例仓库冒充可用的声明式默认源。迁移器是独立程序，只把人工批准的结果写入在线清单。
