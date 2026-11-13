# Independent Appearance Settings v1

**English** · [简体中文](./appearance-settings.zh-CN.md)

## Scope and precedence

Themes declare default fonts and colors through `typography` and `colors`. The app derives the effective appearance in memory from the applied or previewed theme, followed by local overrides and a readability contrast adjustment. The source theme and JSON Resume stay byte-for-byte intact. Appearance settings live on this device, apply across documents and sources, and travel independently of either file. Body and heading font overrides can follow the theme independently. Color reset preserves font choices. Changes use their own reset command and never mark the resume dirty or consume a content undo step.

## Data format

Save this object under `appearance` in the existing application `settings.json`. A CLI `--appearance` file contains the object itself.

```json
{
  "formatVersion": 1,
  "bodyFont": "cjk-sans",
  "headingFont": null,
  "colors": {
    "accent": "#52756B",
    "paper": "#FFFEFA"
  }
}
```

Only the listed fields are accepted, with version 1. An omitted or null font follows the theme. Available selectors are `cjk-sans`, `cjk-serif`, and `font:<native family name>`, such as `font:Arial`. Names are limited to 256 bytes and reject control and bidirectional formatting characters. The body override covers body/meta and explicitly styled itemTitle/label/summary/sideDate roles. Unspecified secondary roles inherit effective body or meta. The heading override covers name/sectionTitle. Select the closest supported real weight in the native family. The bundled fonts provide weight 400 or 600 respectively. A personal family override uses real upright faces for its entire role group, preserving supported weights without synthesizing italics. Resetting the group restores all theme styles. Size, line height, tracking, case, margins, and layout remain those of the theme. A saved unknown family produces an explicit load or export error.

The font list combines native OS enumeration with the app library. TTF, OTF, TTC, and OTC imports go to `Resume One/fonts` under local app data. Validate each file and atomically create a hash-named destination. This operation leaves system font directories and registry entries alone. Limits are 256 MiB per source, 128 faces per collection, 64 MiB per face, and 128 files or 1 GiB for the library. Reject symlinked libraries and existing destinations with different content. Extract standalone SFNT faces from collections and recalculate offsets and checksums. Bound the runtime cache by count and bytes and preserve face identity for already-shaped layouts.

`colors` is optional and accepts `accent/paper/canvas/ink/muted/border/seal` only. Accept `#RRGGBB`, normalize to uppercase, and remove the override on null. A sidebar-specific color has no independent override. Reject unknown keys, future versions, invalid colors, and unresolved fonts. Apply contrast correction to display colors only.

## Interface and state

The Theme → Appearance menu command and the Appearance action in the right rail dispatch the same action. The command works from the focused JSON input and does not bypass a modal theme manager or create a second inspector. Appearance / Done has a minimum 40 × 30 logical-pixel target and visible keyboard focus.

Use the single Manage action and `⌘,` / `Ctrl+,` for folders and online sources. Show the online source selector when entries exist. Open appearance in the right rail in place of the sections list. Done returns to sections. At wide and medium widths the document and right rail stay in place. At narrow widths dock the inspector at right and allocate space for it beside the document.

The right rail shows a searchable, virtualized list of system and imported families, the two bundled choices, and Follow theme. Preview on entry anywhere in a row and restore the confirmed choice on exit. Click or press Enter to confirm. The selection indicator always marks the confirmed choice. Resolve all relevant faces in the background and retain the last valid display while loading. At most one read and one replaceable latest preview request may be pending. Invalidate stale previews on exit, search, scrolling, navigating back, closing, window exit, confirmation, or installation. Support Up, Down, Enter, and Esc without intercepting IME candidate actions. Returning restores focus to the relevant font picker. The native import dialog accepts at most 16 files, reports each result, locates new families, and returns focus to search after completion or cancellation.

Full-width selectable rows preserve their selection indicator even when a long name is truncated. Body and heading keep separate latest selections while bounded resolution runs serially. Esc closes the innermost font list or color picker first, including when focus has moved into the document. The system owns Esc during screen sampling, after which focus returns to the active picker.

A swatch expands an HSV saturation/value surface, hue strip, presets, and eyedropper in place. Only one expands at a time. Hover previews presets and samples the pointer position on the surface or strip. Leaving restores the confirmed color. Click or release after dragging to confirm. Hover leaves the committed marker, exact field, and settings file alone. The document chrome, workspace, controls, and JSON syntax palette use the effective colors. The shell uses a reversible 160 ms transition and contrast correction, without resizing the inspector. Font and color previews each affect only their corresponding role or token. The optional exact field accepts a complete `#RRGGBB` or an empty reset and reports an unfinished value on blur. Drag previews in memory, commits once on release or leaving the window, and consumes pointer interaction so document forms stay idle. Esc first cancels the drag and then closes the picker. Keyboard changes can scroll the focused control into view. Follow theme resets only colors.

Launch sampling only at the user's request. macOS uses NSColorSampler and Windows opens a single-use frozen screen surface that consumes its own click. Convert the result to opaque RGB. Cancellation leaves the color unchanged. The Windows capture stays in memory and is released afterward. Verify Windows, multi-monitor, and HDR sampling on native hardware. Closing discards incomplete exact-input text. Switching between JSON and preview leaves appearance controls available.

Mark a font or one-shot color choice as confirmed only after persistence succeeds. A failed drag commit reverts to the last saved value and shows an error. Resolve uncommitted previews when closing the inspector or window. Use the existing atomic writer, preserve unrelated settings keys, and leave unreadable, oversized, or invalid settings files intact.

## Rendering and export

Fixed `typography.*.color` values are overridden in memory by ink for body/name/itemTitle/summary, muted for meta/sideDate, and accent for label/sectionTitle. A paper override clears fixed text colors from the effective snapshot and recomputes readable semantic colors. Canvas, border, and seal alone leave fixed text colors intact. The toolbar and controls use adjusted global tokens. An explicit `sections.*.entryHeader` line style follows similar rules. A body family override replaces fixed family and italics while preserving supported weight. Ink applies to itemTitle/body lines, muted to meta/sideDate lines, and a paper override clears all fixed line colors. Other style and content fields remain intact and reset restores the original.

Store confirmed and hover state separately. Exports and settings persistence use confirmed values. Invalidate page layout and thumbnails when effective appearance changes, including preview exit and confirmation. Pointer movement within an unchanged preview must not relayout. The JSON editor retains monospace text. Body fonts do not alter toolbar metrics.

Preview, thumbnails, PDF, and DOCX share the same effective theme model. Export snapshots the confirmed theme and appearance at the start and uses the same resolved font data and explicit CJK fallback as preview. Report Unicode code points and reject export if all available faces lack a glyph. Check OS/2 embedding flags. An editable DOCX requires editable embedding, and PDF must embed the full face when subsetting is prohibited. Font licensing still requires separate review. Word lays out the editable result itself.

TXT and Markdown ignore appearance. CLI export uses explicit `--theme <absolute path> --appearance <absolute path>`, independent of desktop settings. `fonts list` enumerates native and app families. `fonts install --file <absolute path>` installs in the app font library. A custom `--dir` only copies to that directory and does not expand app discovery. A missing family on another device generates an explicit error. Appearance files are limited to 64 KiB. Return `APPEARANCE_INVALID` for invalid files without creating or replacing output, and `APPEARANCE_IO` on a read failure.

## Acceptance

Use automated layout tests for five window sizes, intermediate geometry in both switching directions, controls across all 20 bundled themes and light/dark overrides, and form padding and responsive columns at four sizes. Verify stable sidebar geometry, unobscured paper, keyboard reachability in narrow and short windows, snapshot reuse for PDF/DOCX, stale-preview cancellation, and thumbnail invalidation. Verify persistence across restart, separate resets, preservation of theme/resume bytes and other settings, rejection of damaged settings, and isolation of an unfinished color entry. Record native macOS, Windows, and Word results independently. GPUI geometry and event tests do not measure native screenshots, GPU frame rates, or visual polish.
