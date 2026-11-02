# 有限共享组件内容转换 v1

状态：独立迁移器的纯数据编译步骤，不是 React 运行时、可安装主题或排版后端。声明定位之后，按固定来源及明确输入域转换组件的内容边界；任何未支持语义继续阻断。不得仅凭组件名字选择适配器。

## Section 的来源身份

首个适配器仅处理真实导入并经[模块声明连接](theme-module-links-v1.md)追到的 `@jsonresume/core@0.3.6` 的 `src/primitives/Section.jsx` / `Section` 函数。完整文件 SHA-256 必须为：

```
51a0b42151195761af25b97113e9e838b7275054ec74432263e07e185c2c4c05
```

固定来源对应 jsonresume/jsonresume.org 提交 `542cd5315fc9995fca3c401d0c818767530bb6bd`。包名/版本、文件路径/哈希、声明名称和真实导入绑定须同时匹配；本地同名函数、名称碰巧相同的标签、改过实现的包都不能套用。具名/default 别名只在已连接的真实声明相同时适用。

标签绑定还必须与模板 IR 的 import specifier 词法位置生成的 binding id 一致，不按 local 文本名称猜测；函数参数或 map 回调中的同名局部变量不能误匹配。此版本不展开 styled(Section) 包装、attrs、polymorphic as、Provider 或宿主 forwarding 策略。

## 有限内容合同

- 输入为既有模板元素及 children IR，不读取或修改用户简历 JSON，不执行任何表达式。
- 无条件保留语义 section 容器；不因 children 为 null、false、空串、空数组或缺失而删容器，数字 0 也不丢。
- 保留 children 的原始顺序与 IR；未知表达式、未解析子组件、条件/map 以及各自 blocker 不变。不自建标题，不从 title/name 合成内容。
- 接受明确的字符串 id 与 className；缺省 className 为 `resume-section`，显式 className 按原式 `` `resume-section ${className || ''}`.trim() `` 处理。保留内部空白，不去重 class；trim 遵循 ECMAScript 空白集合，不采用 Rust 更宽的 Unicode trim。
- key 只保留为 React 逻辑身份，不写成 DOM 属性；不能因 key 不入 DOM 而吞掉未知表达式的阻断。
- 非有限类型、动态 id/className、显式 children 属性、未知属性、spread、style/theme/ref/as/forwardedAs 等均拒绝；不假装已实现一般 rest 转发。字符串仍为数据，后端须按目标格式转义，不能作为 raw HTML。

组件包装边界的转换成功，不代表子树内容均已转换。原模板程序、绑定、依赖及 blocker 不被改写或清除；输出与原始位置并存，不能把未转换子内容替成空内容来获得成功。

## 检查报告

`--inspect --template-entry ... --dependency ...` 的 `componentContent` 使用格式 `resume-one-section-content-adapter-v1`。`componentPrograms` 描述固定来源与有限操作；`instances` 给出各调用的原始 IR 指针、位置、真实绑定证据、原始属性/children、转换后的 `boundary` 或明确 blocker。`wrapperContentLowered` 只指该容器的有限内容映射，`childrenResolved` 继续为 false。统计是静态 IR 位置数，不是运行时 map 实例数或已迁移主题数。

`retainedBlockers` 保留原检查、模块连接和模板前端的阻断；原程序不被覆盖。不存在目标调用时不凭空报告 Section 样式缺口。所有输出仍为纯数据，不含可执行脚本。

输入/输出各最多 16 MiB，输入最多 250000 个 JSON 节点、深度 128；渲染 IR 遍历最多 50000 步、深度 96、2048 个调用位置、3 秒，模板程序最多 64 个、导入请求最多 4096 个、显式依赖最多 8 包。序列化前检查深度，超限失败，不返回截断成功结果。最终合并报告仍须符合完整检查协议的 16 MiB 门禁。

## 样式与验收门槛

必须保留原 core 的 StyledSection 样式依赖：`margin-bottom` 先按 JS truthiness 选择 `theme.spacing.section` 或 `var(--resume-space-section, 2rem)`；print 下 `page-break-inside: avoid`。数值 0 与字符串 `0px` 的分支不同；CSS var 值与有效性也不能提前猜测。

内容转换不计算 CSS、rem/em、自定义变量、继承/级联、margin collapse、linebox 或分页；不把外部 styled primitive 当作无样式 generic section。默认样式、宿主上下文、外层包装和原版视觉样例尚未通过时，`layoutCompiled`、完整组件语义和 `migrationComplete` 继续为 false，严格迁移仍拒绝输出主题。
