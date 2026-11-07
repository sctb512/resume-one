# 简里原版 JSON Resume 主题包 v1

[JSON Resume 主题页](https://jsonresume.org/themes) 是给人浏览的目录。旧 Registry 的 `/api/themes` 目前重定向到 Registry 介绍页，上游对应实现也只返回示例预览链接。它没有提供适合本机渲染的固定版本、依赖和包摘要。简里将原版主题的固定版本放在独立主题仓库的 `originals/`，与已有声明式包分开发布。[上游目录接口源码](https://github.com/jsonresume/jsonresume.org/blob/master/apps/registry/pages/api/themes.js)

## 文件与接口

`originals/index.json` 沿用[来源索引 v1](../resume-one-theme-source-v1.schema.json)。原版包路径以 `.original.theme.json` 结尾，每项必须填写整个文件的 SHA-256。客户端将分支解析为 Git commit，再按同一 commit 获取索引和包。索引中的 ID 和名称须与包内值一致。

包遵循 [JSON Schema](./json-resume-original-theme-v1.schema.json)，UTF-8 文件上限为 1 MiB。`package`、`version`、`author`、`upstream` 和 `license` 对应上游发布。主题仓库另附原始许可及依赖声明。将上游 npm 包和依赖固定后，打包器把模板、样式和静态资源装入单文件。旧包读取的模板和样式可在打包时绑定到静态字节。JS 入口以 `globalThis.__resumeOneTheme` 暴露同步 `render(resume) -> string`，可附 `pdfRenderOptions`。可选的 `stylesheets` 用原始 HTTPS URL 对应已打包的 CSS，渲染器用这些内容替换 HTML 引用，无需网络。

## 执行与显示

- JSON 文件通过简历 Schema 后，完整对象的副本传给原版渲染器，未知字段保留。为旧模板在副本补齐空的 `basics.location` 和 `basics.profiles`。
- 每次渲染用独立 QuickJS 实例，限制执行时间、内存、调用栈和 HTML 大小。主题代码无文件、网络、Node 或系统接口。
- 临时 Chromium 实例绘制生成的 HTML，保留可用的内联样式与 data URI 资源，清除可执行 HTML，并拦截网络和文件请求。预览的图像由本机绘制，PDF 保留 Chromium 输出的文本。
- 预览、打印 CSS 与上游包相关。外部字体或图片需要随包提供，系统字体和 Chromium 版本也会影响结果。原版包不承诺与官网实时部署逐像素一致。
- 原版主题的模块栏面向标准 JSON Resume 结构。文档内容在 JSON 编辑器修改，纸面没有声明式排版的编辑锚点。可编辑 DOCX 走简里默认排版。
