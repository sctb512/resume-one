# 简里原版 JSON Resume 主题包 v1

[JSON Resume 主题页](https://jsonresume.org/themes) 是给人浏览的目录。旧 Registry 的 `/api/themes` 目前重定向到 Registry 介绍页，上游对应实现也只返回示例预览链接。它没有提供适合本机渲染的固定版本、依赖和包摘要。简里将原版主题的固定版本放在独立主题仓库的 `originals/`，与已有声明式包分开发布。[上游目录接口源码](https://github.com/jsonresume/jsonresume.org/blob/master/apps/registry/pages/api/themes.js)

## 文件与接口

`originals/index.json` 沿用[来源索引 v1](./schemas/resume-one-theme-source-v1.schema.json)。原版包路径以 `.original.theme.json` 结尾，每项必须填写整个文件的 SHA-256。客户端将分支解析为 Git commit，再按同一 commit 获取索引和包。索引中的 ID 和名称须与包内值一致。

同一索引也支持声明式主题的可选 `files` 对象。键为与清单同目录的 TTF、OTF、TXT 或 MD 文件名，值为文件 SHA-256。含 `files` 的条目也须提供清单自身的 `sha256`。声明的字体路径及摘要须与 `assets.fonts` 一致。字体上限为 8 MiB，文本上限为 1 MiB，完整包上限为 32 MiB。应用按同一 commit 下载全部资源，校验后才渲染或安装。下载到本地时保存一个带 `theme.json` 的完整文件夹。

本机来源配置由 `repository`、`ref`、`indexPath` 和可选的 `name` 组成。展示名称由用户编辑，缓存身份由前三项决定。

包遵循 [JSON Schema](./schemas/json-resume-original-theme-v1.schema.json)，UTF-8 文件上限为 1 MiB。`package`、`version`、`author`、`upstream` 和 `license` 对应上游发布。主题仓库另附原始许可及依赖声明。将上游 npm 包和依赖固定后，打包器把模板、样式和静态资源装入单文件。旧包读取的模板和样式可在打包时绑定到静态字节。JS 入口以 `globalThis.__resumeOneTheme` 暴露同步 `render(resume) -> string`，可附 `pdfRenderOptions`。可选的 `stylesheets` 用原始 HTTPS URL 对应已打包的 CSS，渲染器用这些内容替换 HTML 引用，无需网络。

## 执行与显示

- JSON 文件通过简历 Schema 后，完整对象的副本传给原版渲染器，未知字段保留。为旧模板在副本补齐空的 `basics.location` 和 `basics.profiles`。
- 每次渲染用独立 QuickJS 实例，限制执行时间、内存、调用栈和 HTML 大小。主题代码无文件、网络、Node 或系统接口。
- 预览将清理后的 HTML 交给应用内系统 WebView 实时布局，macOS 使用 WebKit，Windows 使用 WebView2。沙箱禁止页面脚本和外部资源请求，保留随包提供的样式与 data URI 资源。原版 PDF 由临时隔离的 Chromium 进程生成。
- 预览、打印 CSS 与上游包相关。外部字体或图片需要随包提供，系统字体和浏览器引擎版本也会影响结果。原版包不承诺与官网实时部署逐像素一致。
- 原版主题的模块栏面向标准 JSON Resume 结构。文档内容在 JSON 编辑器修改，纸面没有声明式排版的编辑锚点。可编辑 DOCX 走简里默认排版。
