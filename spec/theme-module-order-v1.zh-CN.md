# 主题模块显示顺序 v1

[English](./theme-module-order-v1.md) · **简体中文**

声明式 v2 的 `composition.moduleOrder` 可指定正文模块的显示顺序，不修改简历 JSON、顶层字段顺序或主题文件。

```json
{"composition":{"preset":"single-column","moduleOrder":["education","publications","work","projects","skills"]}}
```

此数组最多 11 项，允许空数组，只接受标准正文模块标识：work、volunteer、education、awards、certificates、publications、skills、languages、interests、references、projects。重复、空字符串、未知标识、basics、null 与错误类型均拒绝。basics 属于刊头，不参与正文排序。

省略或空数组保持简历原有顺序。非空数组按照声明顺序显示已存在的模块；未列出的模块按其原输入顺序稳定追加，不能用数组遗漏来删除内容。数组不创建缺失模块，也不改变模块内条目顺序。

侧栏布局仍遵守 `composition.placements` 的区域归属，各区域采用同一顺序规则。跨区域内容的坐标先后不等于一个线性的阅读顺序；不为满足顺序而移动侧栏模块到主栏。

桌面预览与 PDF 的共享布局路径、可编辑 DOCX 采用同一排序函数。TXT 和 Markdown 不应用主题，继续保持简历输入顺序。回归测试覆盖缺省、空数组、部分顺序、未列项追加、输入/主题不改写、侧栏以及 PDF/DOCX 文本顺序。
