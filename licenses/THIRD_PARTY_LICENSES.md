# Third-Party Notices

**English** · [简体中文](./THIRD_PARTY_LICENSES.zh-CN.md)

## Numeric Text in the Migrator

The bounded content evaluator uses unmodified [ryu-js 1.0.3](https://github.com/boa-dev/ryu-js) by David Tolnay and boa-dev for ECMAScript decimal number spelling. The upstream package offers Apache-2.0 OR BSL-1.0. Resume One uses BSL-1.0. Cargo.lock records the registry version and checksum. [Boost Software License 1.0](LICENSE-BOOST-1.0.txt)

## Syntax Parsers

The stand-alone migrator pins MIT-licensed [tree-sitter 0.25.10](https://github.com/tree-sitter/tree-sitter/tree/v0.25.10), [tree-sitter-javascript 0.25.0](https://github.com/tree-sitter/tree-sitter-javascript/tree/v0.25.0), and [tree-sitter-css 0.25.0](https://github.com/tree-sitter/tree-sitter-css/tree/v0.25.0). The original JavaScript crate has SHA-256 `68204f2abc0627a90bdf06e605f5c470aa26fdcb2081ea553a04bdad756693f5`. The vendored grammar changes only `src/scanner.c` to recognize a bare ampersand as JSX text, preserving character references and source positions without rewriting the input or suppressing syntax errors. The other parser sources are unchanged. [Vendored JavaScript grammar](https://github.com/sctb512/resume-one/tree/main/vendor/tree-sitter-javascript/)

Original copyright notices, in the order listed above:

Copyright (c) 2018-2024 Max Brunsfeld

Copyright (c) 2014 Max Brunsfeld

Copyright (c) 2018 Max Brunsfeld

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

## gpui-component

The desktop app bundles `gpui-component 0.5.1` under Apache-2.0. Its original crates.io package has SHA-256 `d021d46b4088d3d93a57ccdf443da85695a77272108caca2f6fe5369f584966a`. Builds use the same vendored source, localization, and upstream documentation. [Vendored component library](https://github.com/sctb512/resume-one/tree/main/vendor/gpui-component/) · [Apache license](../vendor/gpui-component/LICENSE-APACHE)

Resume One modifies `src/input/state.rs`, `src/input/element.rs`, and `src/history.rs` to expose read-only per-frame text positions and composition status, forward gutter scrolling, isolate programmatic editing transactions, clear redo on a new edit, and dispatch input changes when the system commits IME text through unmark. Gutter controls use this version's font, wrapping, scroll, and clipping geometry. Recheck these behaviors when updating the vendored version.

## JSON Resume Schema

The bundled `json-resume-1.3.1.schema.json` is copied unchanged from `@jsonresume/schema@1.3.1`. The original LICENSE.md follows.

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

## JSON Resume English Date Reference

The Rust date range code in `crates/resume-one-core/src/date_range.rs` follows the pinned `formatDateRange` short English month behavior for a deterministic subset. It distinguishes a missing end date from the ongoing sentinel and validates calendar dates without JavaScript rollover. No Intl, clock, time-zone lookup, or upstream execution is involved. The source SHA-256 is `4fb77d24f9a3ea8e2a2855a92d4d9a9c8599e9a6f81aa23bf75da862265287fd`. [Pinned upstream reference](https://github.com/jsonresume/jsonresume.org/blob/542cd5315fc9995fca3c401d0c818767530bb6bd/packages/utils/src/dates.js)

MIT License

Copyright (c) 2014-2026 JSON Resume contributors

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

## Noto Sans SC

PDF and DOCX can embed Noto Sans SC Regular, obtained from the official Google Fonts `NotoSansSC[wght].ttf` at wght=400 with updated naming and no glyph removal. The static file SHA-256 is `eeb06b8a64fd04a2744d95579db1571b51027cda61ed78c62e4b730791525461`. The original and static derivative are distributed under SIL OFL 1.1. These font terms do not apply to documents a user creates with the font. [Bundled font source](https://github.com/sctb512/resume-one/blob/main/assets/fonts/NotoSansSC-Regular.ttf) · [Upstream font](https://github.com/google/fonts/tree/main/ofl/notosanssc) · [OFL and copyright](../assets/fonts/OFL-NotoSansSC.txt)

## Noto Serif SC

PDF and DOCX can also embed Noto Serif SC SemiBold, obtained from `NotoSerifSC[wght].ttf` at wght=600 with updated naming and no glyph removal. The static file SHA-256 is `e827457c14fb9cd2eaf2101b60393c76b6b074e42986bbe4b65f45557860d17c`. The original and static derivative are distributed under SIL OFL 1.1. These font terms do not apply to documents a user creates with the font. [Bundled font source](https://github.com/sctb512/resume-one/blob/main/assets/fonts/NotoSerifSC-SemiBold.ttf) · [Upstream font](https://github.com/google/fonts/tree/main/ofl/notoserifsc) · [OFL and copyright](../assets/fonts/OFL-NotoSerifSC.txt)

## Font Test Fixtures

`theme-font.ttf` is a Noto Sans SC Regular subset for automated checks, with space and the characters in `AdaZm中简里`. The family, full name, unique ID, and PostScript name use Resume One Font Fixture / ResumeOneFixture, without the reserved name Source. The original copyright and license metadata are retained. SHA-256 is `0a8f72c42d7bff2bb6d67b78e603d46e2a285de93bbeb164d10fb2299f25945f`. [Fixture source](https://github.com/sctb512/resume-one/blob/main/crates/resume-one-core/tests/fixtures/theme-font.ttf) · [SIL OFL 1.1](../assets/fonts/OFL-NotoSansSC.txt)

`theme-italic-font.ttf` is a static subset of real Merriweather Italic at wght=400, wdth=100, and opsz=18. The upstream SHA-256 is `f68a8f4989258679e4fbaf50aa42400132b5373c2d9d2514ba82ef6e85947a0b`. The subset includes U+0020–U+007E, U+00E9, and U+0301 for italic, combining accent, and missing glyph tests. It uses the names Resume One Italic Font Fixture / ResumeOneItalicFontFixtureItalic without the reserved name Merriweather and preserves original copyright and license metadata. Fixture SHA-256 is `b294f4c5f3de886047ba99bc78854ee2e59dbb2eb47729f0ae8f5526c77fcae2`. Both fixtures are for testing only. [Fixture source](https://github.com/sctb512/resume-one/blob/main/crates/resume-one-core/tests/fixtures/theme-italic-font.ttf) · [Pinned source font](https://github.com/google/fonts/blob/9862abc066c9b83bf84c3e8ed1fc2d8d48f9bbf0/ofl/merriweather/Merriweather-Italic%5Bopsz%2Cwdth%2Cwght%5D.ttf) · [SIL OFL 1.1](../assets/fonts/OFL-Merriweather.txt)
