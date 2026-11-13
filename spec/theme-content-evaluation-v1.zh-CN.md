# 主题内容求值协议 v1

[English](./theme-content-evaluation-v1.md) · **简体中文**

状态：独立迁移器的有界纯数据步骤。把编译器已经生成的 IR 与显式选择的 JSON Resume 样例组合，产生未排版内容树。**不是主题安装格式，不执行 JS，不计算 CSS，不代表视觉或整主题验收。**

```sh
resume-one-theme-migrate --source /absolute/theme --inspect --template-entry src/Resume.jsx --dependency /absolute/core --dependency /absolute/utils --fixture /absolute/public-resume.json
```

## 输入与隐私

`--fixture` 必须同时提供 `--inspect` 和至少一个 `--template-entry`。不自动读取来源包里的 resume.json、examples、GUI 文档或用户目录。省略此选项不求值任何简历内容；提供后，标准输出会包含样例文本。调用者应选择公开/脱敏样例，保护输出，不能把含私人数据的报告提交进仓库。

样例须为显式绝对路径、不超过 1 MiB 的普通文件，拒绝符号链接及符号链接父目录、. / .. 路径。读取前后比较长度与修改时间，属于普通并发变化检查，不是对恶意竞态的原子文件系统证明。使用内置 JSON Resume 1.3.1 schema 校验；不请求远程 schema，不修改样例、来源或主题。报告锁定样例原字节 SHA-256，不把排版数据写进 resume JSON。

## 求值语义

只接收本工具从同一来源快照生成的 `resume-one-template-ir-v1`，不是开放的 JSON 程序执行接口。语法错误及未知语句/调用仍拒绝。允许单独处理样式、组件及动态 URL 的前端待连接项：仅这三类 blocker 可进入内容步骤；每个实际调用还须通过真实词法 binding、样式终点、固定组件来源和 URL 验证。原始 blocker 与 resolved 状态不改写。

- 模块/函数 const 按声明顺序初始化；未被使用的初始化式也不得跳过。JSX 元素的构建与最终渲染分开，未使用的 JSX 不触发其子组件渲染。
- missing/undefined、null、false、0、空串分别保留；默认值仅在 undefined 时生效。短路、条件和可选链保留原边界，括号终止可选链传播。
- map 只接受稠密 JSON 数组，按顺序绑定 item/index 及回调局部 const；非数组报错，不替换为空数组。Fragment 是元素而不是数组，不支持把 Fragment 当 map/join 接收者；读取其 length 得到 undefined。
- join 仅连接有限标量；null/undefined 项为空文本。文本模板保留字面片段，标量转换使用 ECMAScript Number 拼写（ryu-js 1.0.3），-0 为 0；不把对象/数组序列化成文本。
- 严格比较支持标量，不用 JSON 深比较冒充对象身份；关系比较支持同类数字/字符串及非字符串标量的 ToNumber：undefined 为 NaN、null 为 0、布尔为 0/1，NaN 的四种关系比较都为 false。字符串互比按 UTF-16 码元排序；数字与字符串的隐式转换仍拒绝。未支持的 coercion、原型/继承方法、复合 key 或字段接收者明确报错。
- key 在元素构建时验证，但不转发为 DOM 属性。布尔/null 子内容不渲染，数字 0 保留；对象作为子内容报错。br/hr 不能有内容。原生标签与属性使用有限白名单，文本始终为数据，不是 raw HTML。

## 固定共享组件

调用须经模块声明连接定位到 `@jsonresume/core@0.3.6` 的真实函数、精确文件路径与 SHA-256，不能仅按名字替换。来源包/文件指纹与依赖快照交叉核对；DateRange 和 ContactInfo 还核对 utils 0.4.0 的精确 helper 文件、exports 目标及 security 桥。任何修改均退出已支持域。

| 组件 | 当前有限域 |
|---|---|
| Section | 保留容器与子内容顺序；有限 id/className；未支持 rest/polymorphism 不猜测 |
| SectionTitle | 默认 h2 或整数 1–6；有限 id/className，icon 等未覆盖行为报错；样式仍未编译 |
| DateRange | canonical Gregorian YYYY / YYYY-MM / YYYY-MM-DD、默认 short/en-US；missing end 只显示起点，显式 falsy end 显示 Present；非法日、时区时间戳、其他 locale/format/numbering/presentLabel 不近似 |
| ContactInfo | email→phone→location→website→profiles 的源顺序，network\|\|username、分隔符及空项规则；不把平铺 props 自动填入 basics；危险已知协议按源移除 href，未支持 URL 安全回退明确报错 |

DateRange / ContactInfo 忽略的 children 不触发子组件渲染。ContactInfo 的 URL 文字只去掉小写 http(s) 前缀与恰好一个尾斜杠；非商业/其他主题许可不因使用此步骤而改变。有限数据域错误不等于声称原 JS 在该输入也会抛错，只表示迁移器不能证明该输入等价。

## 独立文件中的 styled 声明

导入的 const styled 定义可以沿同一冻结快照内的精确声明、别名和基类连接，不把它们当成普通函数执行。必须匹配真实 const、initializer 与 styled 原始 span、文件/包指纹；碰撞、可变绑定、未初始化基类、循环、attrs/helper 和未支持基类均拒绝。跨包解析仍使用显式依赖 exports，不猜文件扩展名或 Node fallback。

内容树的 `externalStyleLayers` 保留 base→derived 各层包名、来源/文件指纹、文件名、声明及位置，与本模块的 `styleBindings` 分开。终点只能是白名单原生元素，或继续经过固定 hash 守卫的 core 组件。完整原始 CSS 与其 blocker 留在来源报告中，不计算级联、不清除样式错误。每次外部声明链最多 64 层、1024 步、3 秒；内容求值器的整体预算同时生效。

## 输出与门禁

`contentEvaluation.format` 为 `resume-one-content-evaluation-v1`，包含样例身份与各模板入口的来源身份。成功项的 `result.contentEvaluated:true` 和 `tree` 仅证明这次有限内容求值完成；树保留元素、属性、原始 tag / 样式 binding 及固定样式角色。失败项只返回 error，不保留截断树或用空内容顶替失败节点，并汇入总 blocker。

`stylesCompiled`、`layoutCompiled`、`moduleInitializationVerified`、`visualVerified`、`migrationComplete` 继续为 false。原版内容树相同，也不能代替字体、级联、linebox、margin collapse、分页、多样本和 GUI/PDF/DOCX 的独立验收。严格迁移依旧拒绝未完成主题。

输入遍历先限制深度 96；报告最多 400000 节点、样例最多 50000 节点及 1 MiB 保守内存估算。每入口最多 200000 求值步、3 秒、数组 10000 项、累计 32 MiB 保守分配费用（包含值、字符串、容器槽位与复制，不仅计文本），求值深度最多 96。内容输出的保守费用与实际 JSON 序列化各最多 8 MiB，最终合并报告仍受 16 MiB 门禁。超限拒绝，不把截断结果当完成。
