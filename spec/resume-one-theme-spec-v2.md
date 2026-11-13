# Resume One Declarative Theme Specification v2

**English** · [简体中文](./resume-one-theme-spec-v2.zh-CN.md)

Resume One v2 themes are UTF-8 JSON files consumed by the shared Rust core. Use them for built-in themes, local packages, and adapted community themes. Preview, PDF, and editable DOCX share normalized layout semantics. See the [JSON Schema](./schemas/resume-one-theme-v2.schema.json) and [structural example](./examples/resume-one-theme-v2.example.json).

## 1. Why a Separate Specification

The [JSON Resume schema](https://github.com/jsonresume/jsonresume.org/tree/master/packages/schema) specifies résumé data. Upstream themes use npm packages and HTML or React rendering conventions. The [upstream theme scaffolding](https://github.com/jsonresume/jsonresume.org/blob/master/docs/CREATING_A_THEME.md) uses a rendering interface, and the [older boilerplate](https://github.com/jsonresume/jsonresume-theme-boilerplate) uses JavaScript, Handlebars, and CSS. Reactive Resume also has its own application model. This document defines Resume One's declarative layout format. Applications can load separately distributed upstream packages through the [original-theme package format](./json-resume-original-theme.md).

The theme supplies typography and color defaults. [Independent appearance settings](./appearance-settings.md) apply personal overrides to an in-memory snapshot for preview and export. Theme files and résumé JSON do not store these overrides. Authors can start with a minimal file and use the schema for structural validation and this document for cross-field semantics.

```json
{
  "formatVersion": 2,
  "id": "calm-paper",
  "name": "Calm Paper",
  "version": "1.0.0",
  "license": "MIT",
  "layout": "classic",
  "colors": { "accent": "#52756B", "seal": "#A14F40", "paper": "#FFFEFA", "canvas": "#F6F3ED" },
  "modules": ["basics", "work", "education", "skills"]
}
```

## 2. Files and Compatibility

Use `*.theme.json` for a standalone theme or `theme.json` in a package directory. Built-in and downloaded declarative themes follow the same format. A GitHub index only locates packages. UTF-8 JSON disallows comments and trailing commas. A theme ID is unique within a source. Online identity includes source, resolved commit, and ID. File name does not determine identity. The index ID and name must match the manifest, and any digest is calculated on the raw file bytes.

The [v1 schema](./schemas/resume-one-theme-v1.schema.json) remains supported. Convert v1 values to v2 defaults in memory and preserve on-disk bytes. V2 retains legacy layout values as fallbacks and adds version, provenance, paper, typography, regions, per-section appearance, and bounded resources. An explicit composition takes precedence. `formatVersion` identifies the format and `version` the individual theme's semantic version. Unknown versions, keys, and enum options produce errors. Keep the applied theme and source configuration, cache, and migration mappings separate from `resume.json`.

## 3. Field Semantics

| Field | Contract |
|---|---|
| `id`, `name`, `version`, `license`, `provenance` | Identity, attribution, and pinned provenance. Review license before redistribution. |
| `layout` | Required v1 fallback, `classic/rules/minimal/modern`. |
| `colors` | Require accent, seal, paper, and canvas. Optional ink, muted, border, and sidebar. Derive chrome colors without allowing theme-controlled toolbar geometry. |
| `modules` | Available insertion candidates from the 12 standard sections. Preserve existing content outside this list and show it with fallback layout. |
| `page` | A4/Letter, physical margins, first line, section spacing, entry and heading break controls. Preview and PDF share the same physical layout. |
| `typography` | Font, size in points, line height, weight, tracking and case for body, name, sectionTitle, itemTitle, label, summary, sideDate, and meta. |
| `composition` | Single column, sidebars, or split header. Placements move standard sections between regions and preserve JSON order within each region. |
| `sections` | Per-section heading, entry layout, and date placement. Empty sections omit printed headings. |
| `localizations` | Optional localized names, descriptions, and section headings. |
| `decorations` | Bounded seal, rule, band, and radius schema. Current rendering rejects unsupported custom decorations. |
| `assets.fonts` | Limited package-local static fonts with hashes and licenses. |

### Fonts and Text Roles

A package may contain four static TTF or OTF faces, up to 8 MiB each. Canonical paths must stay inside the package. Verify SHA-256 before parsing and match declared weight to the actual face. `asset:<id>` requires a loaded resource and the GPUI Latin glyph `m`. Convert variable fonts into explicit static instances during migration. Reject WOFF, WOFF2, collections, and missing resources. Use the shared explicit CJK fallback for other missing glyphs.

Register package faces in the current process only and keep handles out of theme JSON. Equal content is reusable. Two different faces with the same family and weight cannot coexist because GPUI font cache reuse could desynchronize preview and export. Use independently named faces when the license allows. PDF and DOCX also check embedding permissions. Online declarative packages can include fonts and license files through hashed index entries. See [GitHub Sources and Cache](#8-github-sources-and-cache).

`trackingEm` defaults to 0 and accepts -0.05 to 0.5. Convert to points per role and insert spacing between extended grapheme clusters, with zero extra terminal spacing. `textTransform` accepts `none`, `uppercase`, and `lowercase`. Apply Unicode locale-independent casing before measuring and wrapping, including expansions such as ß to SS. Preview and PDF share measuring, grapheme offsets, wrapping, alignment, and fallback. DOCX uses editable text and native character spacing rounded to 1/20 pt. Raw data, text, and Markdown retain their original casing.

`color` accepts optional `#RRGGBB` per role. Item title, label, and summary inherit body color. Independent dates inherit meta. Explicit values apply to document text in preview, PDF, and DOCX, and leave chrome, rules, backgrounds, geometry, and interactions unaffected. Theme authors must verify fixed colors on their applicable backgrounds. Personal ink overrides body/name/itemTitle/summary, muted overrides meta/sideDate, and accent overrides label/sectionTitle. A personal paper override removes all fixed text colors from the effective snapshot so readable semantic colors can be recomputed. Reset recovers the authored values.

`fontStyle` accepts `normal` (default) and `italic`, resolved to real faces with no synthesized slant. Asset metadata must agree. Resolve system `font:` faces independently by posture and reject missing variants. Item title, label, and summary inherit body posture, while side dates inherit meta and may explicitly reset to normal. Name, section title, and meta default to upright. The bundled CJK fallback uses real upright faces. PDF embeds the corresponding face and DOCX uses its regular/bold/italic/boldItalic slot. Unsupported or conflicting slots produce explicit errors. A personal family selection uses real upright faces in its role group. Theme defaults restore the original posture.

`typography.label` styles `basics.label` and `typography.summary` styles `basics.summary`. The `sideDate` style applies only to independent left and right dates. Inline dates remain part of secondary text. Inherit unspecified properties from body for label and summary and from meta for side dates. Independent date metrics affect title width, alignment, first-line height, page breaks, and font checks. All sidebar typography scales once, with a 7 pt minimum. Empty role objects preserve default geometry. Static weights accept multiples of 100 from 100 to 900 and require matching available faces.

DOCX can only embed one face per family in each of its four slots. To preserve multiple weights in one slot, a licensed renaming process may assign independent internal families beforehand. Record original, intermediate, and final hashes in NOTICE and respect reserved font names. Keep naming identifiers and PostScript prefixes coherent. Preserve glyphs, layout metrics, license metadata and embedding flags. Only the name table and head checksum may differ. The conversion helper accepts licensed standalone static TrueType inputs. Collections and variable faces are excluded, and CFF/CFF2 names require separate review. Technical validation does not establish legal clearance or native Word acceptance.

### Theme Localization

`localizations` uses lowercase BCP 47 tags such as `zh`, `en`, and `en-gb`. Each value can contain `name`, `description`, and one-line plain-text `sections` keyed by standard section ID.

```json
{
  "zh": { "name": "知白", "sections": { "work": "职业经历" } },
  "en": { "name": "White Space", "sections": { "work": "Experience" } }
}
```

Lookup tries the requested tag and then parent tags per field. The original manifest provides fallback catalog name and description. Chinese and English document labels use localized section title, then `sections.<id>.title`, then the app's translated standard labels. Preview/PDF/DOCX share this sequence. Text and Markdown use the app's standard labels in the same language. Original-theme rendering replaces recognizable standard headings in Chinese preview and PDF while retaining authored custom text and résumé content.

A package can include `localizations.txt` with the same UTF-8 JSON shape for compatibility. A standalone theme can use a sibling `<id>.localizations.txt`. Hash online extras in `files` and retain them offline. New themes may use the inline object. Publication rejects duplicate inline and sidecar declarations. Localization leaves identity, filename, license, credits, and résumé data unchanged.

### Defaults

| Omitted | Default |
|---|---|
| `description`, `provenance` | Empty description and no migration metadata. |
| `page` | A4, four 16 mm margins, baseline origin, 6 mm section gap, keep entries together where possible, and headings with their first entry. |
| `typography` | Body cjk-sans 10 pt/400/1.4, name cjk-serif 26 pt/600/1.2, section titles cjk-serif 13 pt/600/1.3, meta cjk-sans 8.5 pt/400/1.3, zero tracking. |
| `composition` | Single column with stacked header and inline contact. Place sections in their original JSON order. |
| `sections` | Plain headings, stacked entries, inline dates, and no printed placeholders. |
| `decorations`, `assets` | No custom decorations or resources. |
| `colors.ink/muted/border/sidebar` | Derive from required colors with contrast correction. |

Explicit values override only their own defaults. `provenance.fidelity` can be `native`, `adapted`, or `inspired`. Use verifiable `upstreamUrl/upstreamRevision/upstreamAuthor/upstreamLicense/migratedBy` for adaptations. Record acceptance in the migration report. Do not surface unsolicited provenance notices in theme rows.

### Paper Origin and First Line Positioning

`page.firstLinePosition` accepts `baseline` (legacy default) or `line-box`. In line-box mode the origin is the top of the first ordinary line box, with existing section and entry spacing still in effect. The baseline is offset by `(lineHeight + ascent - descent) × sizePt / 2`. Use real em face metrics with descent positive and advance by each line's actual box height. Main content, sidebar, split header, and continuations have separate origins. Decorated headings and inline pairs reserve their actual line box.

`page.marginsMm` represents final physical spacing of 3–35 mm per side. A migrator can combine supported page and container CSS spacing before emitting the values. The app performs no CSS margin calculation on load. DOCX keeps the physical margin and uses native editable lines without an extra ascent spacer. Viewer-specific geometry remains independently testable. [Adaptation protocol](./theme-adaptation-v1.md)

### Entry Title and Header Lines

`typography.itemTitle` styles titles such as role, education entry, or project. It defaults to body style at 1.06 times body size and inherits omitted properties. Explicit personal body overrides also apply to this role.

An optional `sections.<section>.entryHeader` replaces the default merged entry title and metadata lines. Descriptions, bullets, links, item indexes, and data remain intact. A line has 1–6 parts, and a header has 1–8 lines. No field may appear twice in the same header. Set a required inheritable style role of `itemTitle/body/meta/sideDate` and optionally override individual properties under `style`. A line has a single typography style including actual asset fonts. Header-only fonts must pass the same resource and export checks.

```json
"work": {
  "date": "inline",
  "entryHeader": [
    { "role": "itemTitle", "parts": [{ "field": "position" }], "spaceAfterPt": 3 },
    { "role": "body", "parts": [{ "field": "name" }],
      "style": { "family": "asset:company-italic", "weight": 400, "fontStyle": "italic", "sizePt": 12 } },
    { "role": "meta", "parts": [{ "field": "dateRange" }], "style": { "sizePt": 10, "color": "#666666" } }
  ]
}
```

| Section | Header fields |
|---|---|
| work | position, name, dateRange, location |
| volunteer | position, organization, dateRange |
| education | institution, studyType, area, score, dateRange |
| awards | title, awarder, date |
| certificates | name, issuer, date |
| publications | name, publisher, releaseDate |
| projects | name, dateRange, entity, type, roles, keywords |
| references | name |

Arrays use the standard semantic join. Empty or missing fields omit the entire part including affixes. Optional `prefix/suffix` default to empty and accept at most 32 Unicode characters each, excluding angle brackets, controls, and newlines. An empty line consumes no height. `spaceBeforePt/spaceAfterPt` apply only to nonempty lines, range from 0 to 48 pt, and default to zero. Adjacent spacing adds in preview and PDF. Actual face metrics can increase minimum clearance between lines and before the first body paragraph, bullet, or link. DOCX reserves sufficient native paragraph spacing without relying on Word to add adjacent values. Negative spacing is excluded.

Keep wrapping, pagination, real font measurement, and hit anchors consistent between preview and PDF. Scale sidebar lines once, to at least 7 pt. Keep headings with the first visible line. DOCX writes separate editable paragraphs and includes the entry's normal external spacing on its first line. Personal body fonts and ink/muted overrides apply by line role, and paper removes all fixed line colors. A reset restores authored styles. The only compatible date placement for entry headers is omitted or `inline`. The `sideDate` role affects style without enabling side positioning. Omitted heading fields stay available in JSON/forms/text outputs. Cross-section reads, custom extension fields, descriptions, mixed inline styles, and arbitrary expressions are outside this format.

### Bounded English Date Ranges

A `dateRange` part may use `dateFormat: "raw"` (default) or `"short-en"`. Other fields cannot set it. Short English accepts valid Gregorian ASCII YYYY, YYYY-MM, or YYYY-MM-DD and displays values like `Jun 2020 - Dec 2024`, with year-only inputs in January. It performs no timezone or clock lookup. A missing end shows only start, and a missing start produces an empty result. The low-level projection preserves an explicit null or empty end as `Present`. The JSON Resume 1.3.1 schema still rejects such invalid date inputs. Noncanonical or impossible dates retain raw values. Timestamps, regional calendars, and custom format strings are outside the contract. The change is limited to opted-in header parts in preview, PDF, and DOCX. [Reference implementation attribution](../licenses/THIRD_PARTY_LICENSES.md)

### Community Packages and Attribution

Use the same `<id>/theme.json`, fonts, licenses, and `NOTICE.txt` layout locally and for distributed adaptations. Retain verifiable upstream URL, revision, author, and license. Keep separate font and theme terms. For CC BY-NC-SA 4.0, preserve attribution, noncommercial and share-alike terms, and make them available in the theme notice. Record known adaptation changes without implying endorsement.

Preflight the destination and all existing paths. Reject symlinks, nonregular files, and conflicts without overwriting. Write validated resources and license files first, then publish `theme.json` atomically. An interrupted initial installation must not expose a manifest with missing resources. Loading, migration, and catalog acceptance are separate checks.

### Section Appearance

```json
"sections": {
  "work": { "heading": "filled", "items": "compact", "date": "inline" },
  "projects": { "heading": "accent-line" },
  "education": { "heading": "line" }
}
```

| Setting | Rendering |
|---|---|
| `title` | Optional nonblank single-line plain text up to 120 Unicode characters, excluding angles, line breaks, and controls. Applies to visual document headings only. Omission uses the standard label. |
| `heading: plain` | Accent text with no decoration. |
| `heading: line` | About 0.6 pt border-colored rule below the heading. |
| `heading: accent-line` | About 1.2 pt accent rule below the heading. |
| `heading: filled` | Full-width accent fill, readable paper-colored text by default, 6 pt horizontal and at least 4 pt vertical padding. Authors verify contrast for fixed title color. |
| `headingAlign` | Left by default, or center/right, per line within the actual column. Fill/rules and hit boundaries retain column width. |
| `items: stack` / `compact` | 4 pt / 2 pt before entries and 3 pt / 1 pt before descriptions, preserving type and content. |
| `date: inline` | Date sits in standard metadata order. |
| `date: left` / `right` | Independent date on the first title line with at least 12 pt separation. Left date uses up to 42% of the column. Wrap titles in remaining space. Overflowing dates get their own aligned line before the title at left or after it at right. |

Shared layout keeps decor clear of the first entry, with actual ascents/descents and break rules. Empty entries retain editing indexes. DOCX expresses these styles with native paragraph borders, shading, indentation, tabs, and separate date paragraphs.

A timeline fits work, volunteer, education, or projects, with a 16 pt left content indent, border rail, and accent square. On continuation pages retain the rail and do not repeat the opening marker. Cards use full-column 0.6 pt outlines with 8 pt horizontal body padding and a light accent fill only when ink, muted, and accent reach 4.5:1. Long cards can continue across pages. Editable DOCX uses native left borders for timelines and paragraph borders and shading for cards. Chips are limited to skills, languages, and interests. Keep lists, names, proficiency, descriptions, and links in order. Wrap long chips without truncation or font reduction. DOCX uses editable run shading. None of these choices changes text/Markdown content.

### Sidebars and Basic Information Regions

`composition.preset` can be `sidebar-left` or `sidebar-right`. Each region preserves original module order. `placements` selects sidebar sections and other sections remain in the main column. `sidebarWidthPct` defaults to 30 (20–40) after subtracting `gapMm`, which defaults to 6 mm (0–15 mm). Sidebar content has 6 pt horizontal padding with independently corrected ink, muted, and accent contrast against `colors.sidebar`. PDF and preview wrap and paginate each column independently after the full-width area, restoring the top margin on following pages. Keep one footer per page and full-width form overlays with column-accurate hit regions. Editable DOCX uses a borderless table with gutter cells, natural row height, and separate keep groups. Its sidebar shading follows table height. Check multi-page behavior in Word.

| Field | Default and alternatives |
|---|---|
| `headerRegion` | `page`, or `main/sidebar`, places identity and its inline contacts. |
| `summaryRegion` | `page`, or `main/sidebar/header`. Header places summary after contacts and before the divider. |
| `contactRegion` | `header` follows identity, or `main/sidebar` to show one independent left-aligned contact block. |
| `sidebarTypographyScale` | 1, range 0.7–1.2, all sidebar roles and entry header lines, 7 pt final floor. |

Within a region, render identity, independent contact, summary, then modules. Full-width areas precede columns. Empty fields produce no printed placeholder. Keep distinct hit bounds for basics in separate columns and edit the same `basics` object at each location. Allow left-aligned `summaryRegion: header` in all presets. Other non-default regional or scale settings require a real sidebar with at least one placed section or basics region.

### Header and Contact

`header: stacked` left-aligns identity and contact. `centered` wraps first and centers each line within the content area. The summary stays left-aligned. `split` divides identity and contact 58%/42%, with a 14 pt left inset for the right-aligned contact column. Use `preset: split-header` with omitted or explicit split header, or set split under single-column. The split requires visible identity and attached contact content on both sides. If taller than a full page, preview and PDF fall back to full-width flow to preserve content. Sidebar placement and spacing options do not configure this split. In DOCX use native centered paragraphs or a fixed-width borderless two-cell table.

`contact: inline` joins semantic contacts with a middle dot and wraps groups in a narrow split column without a leading separator. `stacked` gives each contact its own wrapping line. Reject `icons` explicitly. Preserve field order, with no placeholders or generated icons.

## 4. Fixed Rendering Semantics

The core owns schema parsing, safe text and URLs, built-in labels, field mapping, dates, and missing values. Layout uses selected primitives and data. `basics` provides identity and contacts. Keep unknown JSON extension fields intact even when unsupported visually. `modules` only controls insertion candidates. Theme changes cannot remove or reorder document data. A shared document and manifest produce interactive preview and control-free print layout. The app retains control over interaction and accessibility.

Keep `keepHeadingWithFirstItem` with the first visible line, never an empty anchor. Apply `avoidSplitItems` only when an entry fits a page. Keep a heading plus its first entry together when they fit together. Otherwise prioritize the heading and the first visible line and allow continuation. Do not reserve a full page twice or measure card/timeline content without subtracting actual padding.

### DOCX Export

DOCX uses the same validated semantics, palette, paper and margins in editable native paragraphs and tables. Preserve explicit line breaks and keep only intra-entry paragraphs together, plus the heading and first entry when requested. Word independently determines line wrapping and page breaks. Map typography to half-point font sizes, standard regular/bold semantics (400/600), and minimum line height from `sizePt × lineHeight`, allowing extra room for actual glyphs. Embed full Noto faces as obfuscated OOXML fonts under OFL. Encode paper color as document background and repeated vector page shapes. No rendered screenshots stand in for editable text. Verify native Word compatibility independently.

Resolve fallback by extended grapheme cluster, for normal and italic runs. Missing glyphs use the same real upright Noto Sans SC face as preview/PDF, with inherited color, size, and spacing and without the failed Latin face's bold or italic style. Leave supported glyphs in the original face. Explicit fallback avoids depending on a viewer's substitutions.

## 5. Safety and Validation

Validate the [theme schema](./schemas/resume-one-theme-v2.schema.json) and then enforce semantics, cross-field constraints, and resource limits. Check sidebar placement, unique resource IDs, canonical package paths, actual font hashes and metadata, and per-item and aggregate I/O/cache budgets. Reject unsupported timeline and chips sections, external resources, executables, HTML, CSS, Handlebars, and arbitrary SVG. An upstream URL is attribution only.

On validation failure keep the current applied theme and show source diagnostics in management. Built-in, local, and online themes render the current data locally and draw their own thumbnails. Hover previews and leaves the applied theme, its available sections, export choices, and file data intact. Click applies the validated theme. Built-ins ship as real files and can be revealed in Finder or Explorer. Copy to a writable theme directory when editing a bundled file.

## 6. Migration

### High-Fidelity Acceptance

Pin upstream revision, fonts, and public fixtures. Compare at identical language, content, paper, scale, and output mode. Capture screen and print rules separately and record any screen-to-paper mapping. Check a shared paper baseline for preview and PDF and review editable DOCX reflow separately.

- Compare columns, section and field grouping, order, headers, grids, and decorations without changing source data.
- Compare actual families/weights/axes, size, line height, tracking, case, alignment, spacing, and rules using 96 CSS px/in and 72 pt/in.
- Compare composited colors, transparency and gradients. Record any document color change introduced for readability.
- Check each page across six public fixtures for wrapping, break points, orphans, overflow, missing glyphs and absent fields. Pixel difference is a diagnostic and cannot substitute for review of structure and typography. Platform anti-aliasing alone does not establish a layout defect.

Review against the source before publishing a high-fidelity claim. Where a primitive is missing, extend the bounded renderer and run verification again. Adaptation status and NOTICE disclosures are provenance records and cannot establish a fidelity percentage.

| Source | Automated evidence | Follow-up |
|---|---|---|
| Resume One v1 | Read and normalize with schema | Use without conversion or explicitly upgrade. |
| npm / JS / React / Handlebars | Pin source and extract metadata, palette, fonts, and structure | Validate regions, behavior, pagination, locales, and license. |
| Static HTML/CSS | Extract supported sizes, colors, fonts, and regions | Map into bounded primitives and record missing behaviors. |
| Dynamic dependencies or interaction | Discover catalog and dependencies | Redesign unsupported parts or omit the theme from declarative distribution. |

`resume-one-theme-migrate` is separate from the desktop app and agent CLI. Legacy candidates extract opaque hex/integer RGB colors and provenance with up to 20 font declarations plus layout hints. Hints from comments or strings remain evidence only. The [adaptation protocol](./theme-adaptation-v1.md) gives the bounded projection rules. The migrator statically reads upstream source. If executing originals for comparison, use an isolated comparison environment with public fixtures only. Compare short and long English and Chinese, all sections, sparse inputs, and multi-page A4/Letter. Unclear redistribution rights block publication.

## 7. Source Publication and Review

The public [theme repository](https://github.com/sctb512/resume-one-themes) publishes two catalogs. `migrated-index.json` lists adapted declarative packages. `originals/index.json` lists separately packaged upstream themes using the [original format](./json-resume-original-theme.md). Default settings use branch `main` and the reader resolves it to a commit for each catalog snapshot. Saved custom sources, including an explicitly empty list, take precedence. Private sources can supply `RESUME_ONE_THEME_GITHUB_TOKEN` in the process environment, outside persisted settings, caches, and release archives.

For strict declarative release review, supply `themes/index.json`, theme manifests, and `themes/review.json` using the [review schema](./schemas/theme-source-review-v1.schema.json). Include one record per ID with original-byte hash, upstream URL/revision/license, license and visual reviewers, review date, known limitations, six fixtures (short/long Chinese and English, missing fields, multi-page), and PDF review. Supply two PNGs per fixture plus a reviewed multi-page PDF under `qa/`, with hashes for all 13 files. Use public data only. Evidence is capped at 8 MiB per PNG, 30 MiB per PDF, and 512 MiB total. These files are separate from app downloads. Review themes must be v2 adaptations with author and migration metadata and be fully supported by the renderer.

Run `resume-one verify-theme-source --dir /absolute/checkout` to check consistency, hashes, signatures, required evidence, and path safety. File signatures do not fully decode images or PDFs. A reviewer must still inspect the paired evidence and original license, verify the pinned public commit and release regression results, and check offline fallback. The command neither publishes nor changes default settings. The distributed preview catalogs and this stricter review procedure have separate acceptance records.

## 8. GitHub Sources and Cache

A source accepts `repository/ref/indexPath` with optional `name` (up to 60 Unicode characters, no controls). The location triple determines cache identity. Normalize repository to lowercase. Refs preserve case and can be at most 100 bytes. Index paths must end with `.json` and fit 240 bytes. Use ASCII letters, digits, hyphens, underscores and dots, with slash separators and no empty, dot, or parent path segments. Configure at most 16 distinct source locations. Supply no arbitrary endpoint or inline credentials.

```json
{
  "version": 1,
  "themes": [
    { "id": "calm-paper", "name": "Calm Paper", "manifest": "calm-paper.theme.json" }
  ]
}
```

The manifest path is relative to its index. Indexes and manifests are each limited to 1 MiB and catalogs to 500 entries. IDs and manifest paths are unique. A SHA-256 is 64 hexadecimal digits over raw bytes and required for published default sources and resource-bearing manifests. The optional `files` map provides sibling TTF/OTF/TXT/MD assets with hashes. Fonts are limited to 8 MiB each, texts 1 MiB each, and a package to 32 MiB. Check file identity against manifest-declared resources and include them in local installation. [Source index schema](./schemas/resume-one-theme-source-v1.schema.json)

Resolve refs to full commits and request only pinned files via HTTPS GET at api.github.com or raw.githubusercontent.com, with redirects and cookies disabled, no résumé parameters, and a 15-second request timeout. Use ETag/304 only with validated cache of the same location and commit. Network or validation failures may retain a previously valid catalog as stale with diagnostics.

Validate cached bytes on read with source, commit, manifest path, identity, and declared hashes. Budget cache to 256 MiB with access-time eviction and common GUI/CLI advisory locking and atomic writes. Reject unsafe paths and unsupported rendering features. The cache and the local theme directory are separate. Explicit standalone downloads use `<source-fingerprint>-<id>-<content-hash>.theme.json`. Packages include their verified files and a manifest. An identical download is idempotent. Conflicting existing content stays intact. GUI uses the configured directory snapshot and locks changes during installation. CLI uses explicit `--dir`. Refresh the local list after a download without auto-applying it.

The online applied theme is a verified snapshot independent of the current source or directory. Local identity uses canonical paths. Prefetch up to 20 entries centered on the visible list, clamped at its ends, with hover first and no more than three background requests including catalog work. Scrolling discards old queued jobs. Accept completed requests for the cache but update the screen only for the still-current source/commit/hover. Keep up to 32 verified in-memory themes, plus displayed and applied snapshots. An explicit pending application waits for validation. Switching source or view, going local, opening management, or opening a file cancels it. Source errors go to management and explicit download failures can surface directly. Repeated scrolling does not flood retries. A click or download request can retry deliberately.
