# Best-Effort Theme Adaptation v1

**English** · [简体中文](./theme-adaptation-v1.zh-CN.md)

## Scope

The stand-alone migrator's `--adapt` mode uses a bounded source snapshot and CSS/JSX evidence to project supported typography, palette, paper layout, section titles, and section order into the declarative v2 format. Mapping components and simple selectors to roles is an inference with recorded evidence. The tool leaves the resume file intact and performs no third-party code execution, dependency installation, network requests, or template rendering. It preserves all 12 standard sections. Custom fields remain in the JSON without any guarantee that the renderer understands them.

Only unconditional, literal `:root/html` variables from a static `createGlobalStyle` in the same file can supply a static value. Record their source. Keep component-local variables local. A local module margin cannot replace the global spacing token. If near-zero spacing depends on an unsupported box model, use 4 mm and record the adaptation. Project logical longhand margins using horizontal left-to-right coordinates. A section's vertical margin can provide one spacing value without modeling margin collapse. Logical block start/end borders and inline start borders may map to built-in dividers. Exact weight, line style, placement, and writing modes remain differences.

Preview and export target the print profile. Project applicable `@media print`, `all`, and `only print`, including simple comma-separated alternatives alongside `screen`. Preserve source order within a block, importance, and source spans. Skip feature conditions, `not`, unsupported media, `supports/layer/keyframes`, and blocks under unknown enclosing conditions. Keep `@page` as a paper scope. Record projections, skipped rules, and inferred cross-file order in `printProfile`. This projection applies only to adaptation.

Prefer an explicit document container for text inheritance. Local paragraphs and descriptions may supply only the relevant text role. Recognize bounded class prefixes such as `.pro-name`. A single unknown class declaring font, size, width, and padding may qualify as a document container with recorded evidence. An `aside` used for a heading is insufficient to identify a page sidebar. An unknown grid column with a left, right, or sidebar area and a 20–40% width may qualify. Direction or alignment by itself is insufficient. General module and sidebar heading rules cannot override the person's name.

Read plain-text section names, order, and heading levels from real JSX and literal HTML. Exclude fake tags in comments, scripts, and attributes, as well as dynamic headings. Infer a heading level only if it is unambiguous and covers at least two modules. Use the source file containing the most distinct standard sections for ordering and append the remaining sections. Decoded JS literals report the raw span start and decoded offset separately. Record unsupported constructs, defaults, semantic fallbacks, and automatic readability corrections with their source evidence.

## Static Values and Font Context

Project bounded `calc()`, `min()`, `max()`, and `clamp()`, including parentheses and nested expressions. Length units are pt, px, mm, cm, in, pc, em, rem, and %. Use 1 px = 0.75 pt. Only determinate dimensional arithmetic is allowed, including multiplying or dividing a length by a unitless number. Binary plus and minus require CSS whitespace. Reject unknown units/functions, unresolved variables, length times length, division by zero, non-finite values, and non-zero unitless final lengths. Bound expressions to 8192 bytes, 32 nesting levels, 512 nodes, and 64 arguments per function.

Resolve the `:root/html` font size against 12 pt, the body size against the root, and the inferred document size against body. Rem uses the root. Em and percentage font sizes use the corresponding parent context. A paragraph cannot change other roles' contexts. Valid context sizes are 0.1–96 pt. Outputs must still meet v2 type limits. An unresolved or out-of-range root uses 12 pt and records `ROOT_FONT_SIZE_CONTEXT_UNRESOLVED`. Document all context assumptions in `font-size-context`. Resolve inherited absolute line height and tracking before projecting child roles. A unitless line height remains a multiplier.

Use percentages only with a known font-size reference. Page margins, padding, and entry line spacing cannot use font size as a stand-in for container width. Record values requiring viewport, container, or dynamic inputs as differences. Literal variable resolution and arithmetic are separate bounded static steps.

## Paper Boxes and First Baselines

Select `@page margin`, `html padding`, `body padding`, and a single inferred document container's padding in separate groups, then add the values for each side. Do not stack alternative containers or descendant padding. Record the selected container with `DOCUMENT_PADDING_CONTAINER_INFERRED` and per-side inputs, sources, millimeters, and results in `page-box-composition`. The static precedence rules do not model the complete CSS cascade or pagination.

For example, a 19.05 mm page top margin and 7.94 mm document top padding yield `page.marginsMm.top: 26.99`. Each input must be 0–35 mm and each resulting side 3–35 mm. Use 16 mm and record `NUMERIC_VALUE_UNREPRESENTABLE` when a value is negative, percentage-based, unresolved, or out of bounds.

When source styles are extracted, emit `page.firstLinePosition: "line-box"`. From the top of each line box, position the baseline at `(lineHeight + ascent - descent) × sizePt / 2`, with ascent and positive descent in em. Each line uses its own face and size. Advance by line-box height. Main, sidebar, split header, and continuation regions have independent origins. See [Paper Origin and First Line Positioning](./resume-one-theme-spec-v2.md#paper-origin-and-first-line-positioning).

Top clearance in line-box mode accounts only for ink overflow above the line box. The adapter uses built-in fallback metrics and records corrections as `BASELINE_TOP_CLEARANCE_ADAPTED`. Check actual font extents during final review. DOCX preserves the physical top margin and uses native editable Word lines. Wrapping, baseline placement, and pagination are determined by the viewer.

## Field Bindings and Entry Lines

Build `styles.entryTemplates` from the tool's template IR. Trace the actual section collection, mapped item's lexical binding, supported aliases and destructuring, and direct reads on the same item. Names in strings, shadowed bindings, map indexes, unresolved calls, and unrelated items do not establish a binding. Native tags and statically linked styled elements provide source grouping evidence. A `br` can split lines without altering JSON.

Project an unambiguous candidate with one style source per line into `sections.<section>.entryHeader`. Preserve supported source field order, literal affixes, before/after spacing, and static text styles. Respect 1–8 lines per entry, 1–6 fields per line, and single use of any field. On mixed styles, ambiguous candidates, or out-of-domain input, keep standard section layout and report differences. Descriptions, bullets, and URLs remain in the standard content layer with stable item indexes. Only supported field-presence predicates can influence output. Report other punctuation or conditional differences.

`fieldBindingsVerified` and `sourceOrderVerified` certify the bounded field and order evidence. `computedLineGroupingVerified` and `styleInheritanceVerified` remain false. The projection does not compute DOM display, wrapping, runtime branches, or full component semantics. A projected entry header leaves `layoutCompiled`, `visualVerified`, `migrationComplete`, and `equivalent` false. `layoutValidated: true` means the shared renderer accepted the output. When binding evidence connects same-file `styled(Base)` layers to the same element, merge static declaration layers base to derived before resolving relative size. Keep DOM ancestor inheritance separate. Differing sources in a line count as mixed style.

Project `@jsonresume/core`'s DateRange to `dateRange` with `dateFormat: "short-en"` only with a verified import and direct startDate/endDate reads on the same item, without options or children. Report `DATE_FORMATTER_ADAPTED`, import and argument evidence, and snapshot match status. Formatter implementation, date semantics, and equivalence verification remain false even for a matched snapshot. This adaptation cannot satisfy strict migration.

## Single Theme

```sh
resume-one-theme-migrate --source /absolute/source-package --adapt --out /absolute/example.theme.json --verify-fixture /absolute/public-en.json --verify-fixture /absolute/public-zh.json
```

`--dry-run` leaves files untouched. Existing output requires `--overwrite`. Use the [source snapshot limits](./theme-migration-inspection-v1.md) for source directories and reject destination or parent symlinks. Use batch mode to include theme font assets.

## Batch Packages

```sh
resume-one-theme-migrate --source /absolute/catalog.json --adapt --batch --out /absolute/new-output-directory --font-library /absolute/fonts.json --verify-fixture /absolute/public-en.json --verify-fixture /absolute/public-zh.json --verify-fixture /absolute/public-long.json --verify-fixture /absolute/public-missing.json --verify-exports
```

Read a JSON catalog up to 4 MiB with 1–256 `items`. Each requires a unique `slug` and absolute `sourceDirectory`, and may set `license`, `licenseFile`, `upstreamUrl`, and `upstreamRevision`. Slugs are lowercase ASCII letters, digits, or hyphens, begin with a letter or digit, and are at most 53 characters. Prefix generated IDs and directories with `community-`.

Require a new output directory and forbid containment in either direction with any source. Each item stages and validates its theme before publishing `theme.json`, `NOTICE.txt`, `MIGRATION.json`, and any referenced fonts and licenses. A failed item has no final manifest. Keep successful siblings and report each failure in `migration-report.json`, returning `ADAPT_BATCH_PARTIAL` and exit 3 for incomplete batches. `--jobs` accepts 1–4 and defaults to 1. Validate and publish independently while retaining source item order in the summary.

Extract CSS only from real strings and interpolation-free style regions in the snapshot, retaining source path, span, and hash. If the grammar rejects part of the CSS, use only independently reparsable declarations and report the gaps. Apply bounded literal escape decoding without stitching across template interpolations. For packages declaring Twind/Tailwind, project only explicitly supported text utility classes in static native JSX `className`. Report projected and unresolved classes with the raw attribute span. Custom configuration, arbitrary values, plugins, combinations, and responsive variants remain unresolved. `configurationVerified` and `equivalent` stay false.

`allGeneratedAndValidated` means all packages passed the requested automatic checks. Preserve representable package authors in `provenance.upstreamAuthor`, and original author and repository declarations in the report. Include an author name and representable HTTP(S) upstream link in the notice. Exclude email-bearing npm person declarations from display names. Static metadata is recorded without network verification or any implication of endorsement.

## Font Library

The explicit library is `{ "formatVersion": 1, "fonts": [...] }` with up to 128 static faces. Each entry has `family`, `weight`, `fontStyle`, absolute `path`, `sha256`, `license`, and absolute `licensePath`. An explicitly renamed face can declare its real internal `fontFamily`. Validate true family, weight, posture, and hash, and record aliases. A matching alias does not certify identical glyph provenance. Without an alias, the face's family must match exactly.

DOCX supplies only one embedding slot per family for each of regular/bold/italic/boldItalic. Concurrent 400/500 or 600/700 in the same slot is rejected. Where licensing permits, library authors may give distinct internal families to static faces ahead of time, with source, original hash, change statement, and verified unchanged shapes, weights and metrics. An alias alone cannot resolve an internal family collision. The migrator copies exact font bytes.

Limit the library to 1 MiB, each font to 8 MiB, and each license to 1 MiB. Reject symlinks, relative paths, variable faces, collections, WOFF/WOFF2, missing licenses, hash mismatches, and input changes during reads. Validate the full library and select at most four faces in body, name, section title, and entry title priority, followed by labels, summary, dates, and meta. Report any built-in substitute.

A request can supply an ordered `families` CSS stack up to 16 entries. Select the first exact matching family, weight, and posture. Stop at a generic or system placeholder. Record `FONT_STACK_FALLBACK` with skipped names and the selected face when using a later family. Older requests with no stack use only exact `family/weight/fontStyle`. A library face may carry a plain-text `notice` up to 16384 UTF-8 bytes, allowing LF, CR, and tabs as controls. This can document source, hashes at each conversion stage, and renaming. Deduplicate selected faces by hash. Include notices in `fonts.notices` and package NOTICE, labeled as supplied and independently unverified. Reject invalid notices before changing output.

## Verification and Status

Always validate schema, resource fingerprints, and renderer support. `--verify-fixture` takes an explicit public/de-identified JSON Resume of up to 1 MiB and uses the bundled schema. Record its path, hash, page, line, and module-target counts. Omit resume body text. Reject non-finite positions, off-page text, or disappearing standard sections. Check that fixture bytes did not change. `--verify-exports` checks generated PDF and DOCX format signatures. Independent reader checks, visual review, and native Word acceptance are separate requirements.

`visualVerified`, `licenseVerified`, and `exactMigrationComplete` remain false. `--adapt` conflicts with `--require-complete`. Complete compilation and visual verification remain necessary for strict acceptance. Before bundling a theme, review its differences, visual identity, readability, and real exports. Totals, schema checks, and pixel similarity dominated by blank paper cannot establish acceptance.
