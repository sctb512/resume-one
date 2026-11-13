# Theme Module Display Order v1

**English** · [简体中文](./theme-module-order-v1.zh-CN.md)

Declarative v2 themes can set `composition.moduleOrder` to choose the display order. The résumé, theme file, and original top-level field order do not change.

```json
{"composition":{"preset":"single-column","moduleOrder":["education","publications","work","projects","skills"]}}
```

The array accepts up to 11 standard body module IDs: `work`, `volunteer`, `education`, `awards`, `certificates`, `publications`, `skills`, `languages`, `interests`, `references`, and `projects`. Reject duplicate IDs, empty strings, unknown IDs, `basics`, null, and other types. Basics belongs in the header.

If omitted or empty, keep input order. Otherwise, display existing listed modules in the specified order, then append unlisted modules in their input order. The array cannot create or remove modules or reorder entries inside them.

Sidebar themes still use `composition.placements`. Apply the ordering within each region without moving modules to another column to simulate a reading order. Preview, PDF, and editable DOCX share this rule. Text and Markdown use the résumé's input order. Tests cover omission, empty and partial arrays, stable append, preservation of source files, sidebars, and PDF/DOCX text order.
