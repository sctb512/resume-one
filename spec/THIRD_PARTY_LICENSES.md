# 第三方规范许可

## gpui-component

桌面组件固定于 crates.io 的 `gpui-component 0.5.1`，原始发布包 SHA-256：`d021d46b4088d3d93a57ccdf443da85695a77272108caca2f6fe5369f584966a`。源码、翻译和原始说明保留于 [vendor/gpui-component](../vendor/gpui-component/)，采用 Apache-2.0；完整原始许可见 [LICENSE-APACHE](../vendor/gpui-component/LICENSE-APACHE)。不从开发机 Cargo 缓存打临时补丁，其他机器构建也使用同一版本。

Resume One 修改仅位于 `src/input/state.rs`、`src/input/element.rs`、`src/history.rs`：公开当前帧只读文本坐标与组合输入状态、转发行旁滚轮、隔离程序化编辑事务、使新编辑失效旧重做记录，并在平台以 unmark 提交输入法文本时发送内容变更。行旁控件复用该版本的真实字体、软换行、滚动和裁剪坐标，未替换文本输入引擎。后续升级必须重新验收这些行为。

`json-resume-1.3.1.schema.json` 原样取自 `@jsonresume/schema@1.3.1`（JSON Resume），原始包的 `LICENSE.md` 如下：

The MIT License (MIT)

Copyright &copy; 2024 JSON Resume

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## Noto Sans SC 字体

PDF / DOCX 导出随程序嵌入 [Noto Sans SC Regular](../assets/fonts/NotoSansSC-Regular.ttf)。字体来自 [Google Fonts 官方仓库](https://github.com/google/fonts/tree/main/ofl/notosanssc) 的 `NotoSansSC[wght].ttf`，在 `wght=400` 位置生成静态实例，并更新名称表；没有裁剪字形。字体文件 SHA-256：`eeb06b8a64fd04a2744d95579db1571b51027cda61ed78c62e4b730791525461`。原始字体及派生实例按 SIL Open Font License 1.1 分发；版权声明与完整许可见 [OFL-NotoSansSC.txt](../assets/fonts/OFL-NotoSansSC.txt)。字体许可证不适用于用户使用该字体生成的简历文档。

## Noto Serif SC 字体

PDF / DOCX 导出还嵌入 [Noto Serif SC SemiBold](../assets/fonts/NotoSerifSC-SemiBold.ttf)。字体来自 [Google Fonts 官方仓库](https://github.com/google/fonts/tree/main/ofl/notoserifsc) 的 `NotoSerifSC[wght].ttf`，在 `wght=600` 位置生成静态实例，并更新名称表；没有裁剪字形。字体文件 SHA-256：`e827457c14fb9cd2eaf2101b60393c76b6b074e42986bbe4b65f45557860d17c`。原始字体及派生实例按 SIL Open Font License 1.1 分发；版权声明与完整许可见 [OFL-NotoSerifSC.txt](../assets/fonts/OFL-NotoSerifSC.txt)。字体许可证不适用于用户使用该字体生成的简历文档。

## 字体资源验收夹具

[`theme-font.ttf`](../crates/resume-one-core/tests/fixtures/theme-font.ttf) 是上述 Noto Sans SC Regular 的小型测试子集，包含空格、`AdaZm中简里` 所需字形；字族、全名、唯一标识和 PostScript 名均改为 `Resume One Font Fixture` / `ResumeOneFixture`，不使用保留名称 Source。保留源字体的版权和许可元数据，依同一 [OFL 1.1 全文](../assets/fonts/OFL-NotoSansSC.txt) 分发，仅用于自动化验收，不作为用户默认字体。SHA-256：`0a8f72c42d7bff2bb6d67b78e603d46e2a285de93bbeb164d10fb2299f25945f`。

[`theme-italic-font.ttf`](../crates/resume-one-core/tests/fixtures/theme-italic-font.ttf) 是真实 Merriweather Italic 的静态测试子集，并非由正体合成倾斜。原始文件来自 [Google Fonts 固定版本](https://github.com/google/fonts/blob/9862abc066c9b83bf84c3e8ed1fc2d8d48f9bbf0/ofl/merriweather/Merriweather-Italic%5Bopsz%2Cwdth%2Cwght%5D.ttf)，原始 SHA-256 为 `f68a8f4989258679e4fbaf50aa42400132b5373c2d9d2514ba82ef6e85947a0b`。固定轴为 wght=400、wdth=100、opsz=18，子集保留 U+0020–U+007E、U+00E9、U+0301，用于拉丁文斜体、组合重音和缺字回退验收。修改后的主名称统一为 `Resume One Italic Font Fixture` / `ResumeOneItalicFontFixtureItalic`，不使用保留名称 Merriweather；原始版权、许可元数据及 [OFL 1.1 全文](../assets/fonts/OFL-Merriweather.txt) 保留。夹具 SHA-256 为 `b294f4c5f3de886047ba99bc78854ee2e59dbb2eb47729f0ae8f5526c77fcae2`。仅用于测试，不作为用户默认字体。
