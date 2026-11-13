# Original JSON Resume Theme Packages v1

**English** · [简体中文](./json-resume-original-theme.zh-CN.md)

The [JSON Resume theme directory](https://jsonresume.org/themes) is a browsing catalog. The legacy registry interface was designed around sample preview links. Resume One distributes pinned, packaged upstream themes under `originals/` in the [theme repository](https://github.com/sctb512/resume-one-themes). The packages supply the dependencies and hashes needed for local rendering. See the [upstream catalog interface](https://github.com/jsonresume/jsonresume.org/blob/master/apps/registry/pages/api/themes.js).

## Files and interface

`originals/index.json` follows the [source index schema](./schemas/resume-one-theme-source-v1.schema.json). An original package ends in `.original.theme.json` and requires the package's SHA-256 in the index. The client resolves the ref to a commit and loads the index and package from that commit. The IDs and names must match.

The same index format accepts a `files` map for declarative packages. Keys are sibling TTF, OTF, TXT, or MD filenames, and values are SHA-256 digests. An entry with `files` must also hash its manifest. Paths and digests must match `assets.fonts` for declared fonts. Fonts are limited to 8 MiB each, text to 1 MiB each, and the package to 32 MiB. Verify all resources fetched at the same commit before rendering or installation. The installed directory includes its `theme.json`.

Local configuration contains `repository`, `ref`, `indexPath`, and an optional user-facing `name`. The first three establish cache identity.

The original package follows its [JSON Schema](./schemas/json-resume-original-theme-v1.schema.json) and has a 1 MiB UTF-8 limit. Its `package`, `version`, `author`, `upstream`, and `license` identify the upstream release. The theme repository carries its license and dependency notices. The packager pins npm inputs and bundles the template, styles and static resources. It can bind old file reads to fixed bytes. `globalThis.__resumeOneTheme` exposes synchronous `render(resume) -> string` and optional `pdfRenderOptions`. An optional `stylesheets` map replaces HTTPS stylesheet references with bundled CSS.

## Execution and display

The renderer receives a copy of the schema-valid résumé, including extension fields. The copy supplies empty `basics.location` and `basics.profiles` for old templates. Each render uses a separate QuickJS instance with time, memory, stack and HTML limits. The theme has no filesystem, network, Node, or OS API. The system WebView lays out sanitized HTML and retains bundled styles and data URI resources. The preview uses WebKit on macOS and WebView2 on Windows. Original PDF export uses a temporary isolated Chromium process.

Upstream styles, fonts and browser versions affect output. External assets need to ship in the package. Exact visual parity with the website's current deployment is not guaranteed. The sections panel follows standard JSON Resume. JSON editing works even for fields the upstream template hides. Supported rendered sections and items can expose editing forms through the app's original-theme integration. Editable DOCX uses the default Resume One layout.
