# 受限模板中间结构 v1

[English](./theme-template-ir-v1.md) · **简体中文**

状态：独立迁移工具的编译前端。它把明确选择的 JSX 函数转换为纯数据描述，**不是可安装主题，不是 JavaScript 解释器，也没有接入应用的主题运行时**。任何 `resolved: true` 仅指本规范的有限前端转换通过，不能解释为完整迁移或视觉验收。

## 输入和入口

```sh
resume-one-theme-migrate --source /absolute/upstream-theme --inspect --template-entry src/Resume.jsx
```

入口必须是[源码检查协议](theme-migration-inspection-v1.zh-CN.md)的完整有界快照内文件，报告锁定文件及包 SHA-256；最多 8 个显式入口，不推测入口，不执行 package.json、import、require、模板、网络或 npm 脚本。未支持格式与其他包文件的阻断项仍保留。

模板入口仅为唯一 default export 直接指向的函数声明、函数表达式或同步箭头函数。分析其真实返回值，不把源码字符串或旁支 JSX 当成模板。最多一个 props 参数，支持显式对象解构、别名和 undefined-only 默认值。函数体只接受有限 const 声明后直接 return；helper、工厂、循环语句、任意调用、async/generator、条件 return 与未知语句均阻断。

允许入口函数或 const 同时通过具名声明导出，例如 `export function Section(...) {...}; export default Section;`；这两条声明必须连接到同一个真实绑定，不生成两个入口。具名 helper 仍保留未编译函数阻断，可变声明不因加了 export 而获准。转导出和 `export { ... }` 子句不在此单文件前端内猜测，由独立的模块声明连接步骤追踪，不能把同名私有函数冒充已导出函数。

## 程序结构

`templatePrograms[]` 包含 `entryFile`、`sourceSha256` 与 `program`。程序至少包含：

- `format: "resume-one-template-ir-v1"`、来源路径/字节数、`syntaxValid`、`resolved`。
- `entry`：实际入口函数及其作用域、返回结构；无法确定时为 null。
- `bindings`：词法绑定 id、名称、值、作用域、声明顺序及来源位置。
- `dependencies`：import/export、样式、函数及词法声明的静态证据，不作为可执行内容。
- `blockers`：所有未转换能力、来源范围与原因；展示片段的截断必须明确标注。
- `evaluationContract`：未来有限数据求值器必须遵守的语义，不代表当前已有求值器。

显式 `--fixture` 的[内容求值协议](theme-content-evaluation-v1.zh-CN.md)现实现了其中有界子集；它对固定组件、类型和来源继续做独立验证，并不把前端 unresolved 程序改为完整主题。样式/组件/动态 URL 的有限连接例外只适用于该内容步骤，不改变其他消费者必须拒绝未知依赖的要求。

位置为原始 UTF-8 字节区间与一基行/字节列号，不从展示片段重新解析或编译。作用域 id 与 binding id 在同一程序中可追溯，列表回调不可泄漏或重用其他回调的同名绑定。未知 binding、初始化之前读取、同作用域冲突、可变绑定和 prototype 相关属性不能猜测。

## 有限节点

| 节点 | 含义与约束 |
|---|---|
| `function` | 入口函数，包含作用域与返回体，不包含可执行 JS |
| `input` / `binding` | 显式 JSON props / 对应词法绑定引用 |
| `literal` / `undefined` / `empty` | 有限字面值、缺省值、无子节点；三者不能相互替换 |
| `default` | 仅值为 undefined 时使用 fallback；null、false、0、空串不触发 |
| `get` | 明确属性读取，保留 optional 与可选链延续状态；不允许计算属性/原型访问 |
| `logical` / `compare` / `not` | `&&`、`||`、`??`、`>`、`>=`、`<`、`<=`、`===`、`!==`、`!`；保留原始操作数与短路行为 |
| `conditional` | 显式三元条件与两个分支，不提前执行分支 |
| `group` | 保留括号，尤其是可选链终止边界；`(obj?.a).b` 不等于 `obj?.a.b` |
| `map` | 对明确 JSON 数据路径的数组映射；同步 lexical arrow，item 与可选 index 绑定，保留 optional member/call |
| `join` | JSON 数组字段路径的有限文本连接；无参数默认逗号，或一个静态字符串分隔符，保留可选链边界 |
| `text-template` / `to-string` | 非标签模板字符串的字面片段与显式标量转文本，不采用 JSX 行空白规则或 JSON 序列化 |
| `element` / `fragment` | 有限元素或片段，保持属性/子节点顺序与来源位置 |
| `text` | 合并相邻原文字段/实体后按 JSX 行空白规则处理，同时保留 raw、分段及解码后的 value |
| `attribute` | 明确白名单属性及纯数据值；保留重复属性并阻断，不静默覆盖 |
| `intrinsic` / `component-reference` | 白名单原生标签 / 待连接组件引用；后者必须保持依赖阻断 |
| `blocked` | 未转换的节点占位与来源位置，不得由后端悄悄忽略 |

输入只允许 JSON 数据，不允许 getter、Proxy、重写原型或任意对象行为。数组映射仅对稠密 JSON 数组定义；非数组不能被默默转成空列表。短路产生的特殊可选链标记与普通 undefined 分离，括号终止传播。数字保留 JS Number 的有限值，字符串解码只采用明确支持的子集，不以近似转义代替。

文本模板字面片段保留原文与位置，CRLF/CR 按模板字符串规则变为 LF，但不折叠缩进、不解码 HTML 实体。需解码的反斜杠转义明确阻断。插值使用独立标量 ToString 描述：字符串不变，布尔值为 true/false，null 与 undefined 分别为字面文本 `null` / `undefined`，有限数字必须遵守 ECMAScript 十进制规则（-0 为 0），不是 JSON 序列化。数组、对象、JSX 与回调数组等复合值不在此操作域内，不能通过对象 hook 转换。数据类型尚未知时保留明确输入约束；最终编译器必须通过 schema 类型证明或准确的类型验证满足这些约束，不能将此前端 resolved 当作所有输入的行为等价证明。

join 仅接受明确 JSON 数组数据路径、零或一个静态字符串参数；动态分隔符、spread、多个参数及函数调用接收者均阻断。稠密数组的元素限定为标量；null / undefined 元素按 join 本身语义成为空文本，而不是把错误接收者或整个缺失数组转换为空。嵌套数组/对象与非数组接收者必须报告类型不满足，除非原表达式的可选链确实短路；同样需最终类型证明/验证。模板字符串、join、普通 JSX 的文本规则不可混用。

文本实体只支持明确列出的常用命名实体和合法数值码点；未知实体必须阻断。裸 `&` 是原始文本。属性值不采用子节点的多行空白折叠规则。需要扩展实体/转义时必须新增确定行为及回归样例。

## 安全与完整性

原生标签/属性均为代码中的显式白名单。事件、raw HTML、style 对象、资源加载、ref、`as`、spread、动态/成员组件及未核验 URL 不得隐式接受。静态链接仅接受有限协议与无控制字符；动态链接保持独立待验证项。外部组件、styled 基类和 CSS 尚未连接时，即使已保留组件树，程序也不能为 resolved。

单文件最多 1 MiB、50000 个 AST 节点、深度 96、8192 条绑定/依赖/阻断记录、200000 分析步；前端分析预算 2 秒（独立解析预算见源码协议）。超限返回错误，不返回截断的成功程序。整包报告仍受 16 MiB 总预算限制。

任何消费者必须先拒绝 `resolved: false`、未知节点/字段/版本及未满足的依赖，再执行显式的有限数据操作。将此 IR 转成最终声明式布局、完善依赖与样式编译、原版多样本视觉验收是后续独立步骤，当前严格迁移命令仍失败，不生成或覆盖主题。

## 样式继承连接

每个模板入口可另带 `styleGraph`，格式为 `resume-one-style-inheritance-v1`。它将同一模块中真实 styled 定义与模板编译器的 module-scope binding id 连接，名称、定义位置、绑定类型与唯一性必须同时匹配，不能只按文本名称猜测。

节点保留原始顺序、基类、原生视觉元素/待解析 import 终点、由基类到派生类排列的 `layers` 和该层完整 CSS 规则。层中的重复属性、选择器、media、来源位置不扁平化为配色或字号。前向未初始化基类、循环/冲突、深层或可变绑定、未知 helper 基类、外部原语、动态/未解析 CSS 和非视觉标签均显式阻断；不能将 SectionTitle / ContactInfo 等外部组件替换成普通 div。

最多 1024 个定义、继承深度 64。`inheritanceLinked` 仅是该模块有限继承关系与规则解析的连接状态，空图不报告为已连接；节点状态只描述自身关系，整图还必须无阻断。`layoutCompiled` 当前始终 false。CSS 级联、真正字体继承、margin 折叠、linebox、分页及资源闭包不由该图代替；既有样式/组件阻断和最终迁移失败状态继续保留。
