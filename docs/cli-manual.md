# Resume One CLI Manual

**English** · [简体中文](./cli-manual.zh-CN.md)

The `resume-one` binary operates on local files independently of the desktop app. Pass an explicit absolute path through `--file` or `--dir`. Quote paths containing spaces. To run from source, use `cargo run -p resume-one-cli -- <command>`. Release executables share the fonts in the complete macOS bundle or Windows folder. A symlink to a bundled binary on macOS can be used from your PATH.

## Résumé commands

| Command | Example | Result |
| --- | --- | --- |
| `init` | `resume-one init --file /absolute/resume.json` | Create a minimal valid document, with no overwrite |
| `list` | `resume-one list --dir /absolute/folder` | List regular JSON files in that directory, without reading their contents or recursing |
| `inspect` | `resume-one inspect --file /absolute/resume.json` | Report revision, sections and counts without returning the résumé text |
| `validate` | `resume-one validate --file /absolute/resume.json` | Validate with the bundled JSON Resume Schema 1.3.1 |
| `read` | `resume-one read --file /absolute/resume.json --pointer /work/0/summary` | Read an RFC 6901 JSON Pointer, or the entire document when omitted |
| `patch` | `resume-one patch --file /absolute/resume.json --patch /absolute/change.json --if-match sha256:... --dry-run` | Preview an RFC 6902 patch. Accepts stdin through `--patch -` |
| `structure` | `resume-one structure --file /absolute/resume.json --if-match sha256:... --dry-run add-module skills --before education` | Preview insertion or removal of a standard section or entry |
| `set-field` | `resume-one set-field --file /absolute/resume.json --if-match sha256:... --section work --index 0 --field position --value 'Designer' --dry-run` | Preview an update to a standard text field |
| `export` | `resume-one export --file /absolute/resume.json --format pdf --out /absolute/resume.pdf` | Export PDF, editable DOCX, text, or Markdown |
| `schema` | `resume-one schema` | Return the bundled schema and its version |
| `themes` | `resume-one themes --cache /absolute/theme-cache list --json` | Read a theme catalog with a verified cache |
| `verify-theme-source` | `resume-one verify-theme-source --dir /absolute/source-checkout --json` | Check a candidate source and its manual review records offline |

`--json` can appear before or after the subcommand. It returns exactly one line on stdout. Successful responses use `{"protocolVersion":1,"ok":true,"result":{...}}`. Failures use `{"protocolVersion":1,"ok":false,"error":{"code":"...","message":"..."}}` and may include the file, revision, or diagnostic details. Default output formats success for people and sends failures to stderr. Only `read` returns résumé content. Schema diagnostics identify the JSON Pointer and rule without echoing the invalid value. Keep logs of explicit reads private.

| Exit code | Meaning |
| --- | --- |
| `0` | Success |
| `2` | Invalid arguments |
| `3` | Invalid JSON, schema, structure, or patch |
| `4` | Revision conflict |
| `5` | File I/O or lock contention |
| `6` | Export failure |
| `7` | Theme source, cache, verification, or download failure |

`patch`, `structure`, and `set-field` require the `--if-match` revision from `inspect`. The revision is the SHA-256 of the original bytes, so whitespace changes count. The CLI validates the result, checks the revision, and replaces the file atomically on success. `--dry-run` creates no lock file or document changes. Remove it to commit. If another writer holds the lock, the command returns `RESUME_BUSY` with exit code 5 immediately. Inspect again before retrying. Keep the sidecar lock file.

The four `structure` operations are `add-module ID [--before ID]`, `remove-module ID`, `add-item ID --index N`, and `remove-item ID --index N`. Indices start at zero. New entries and Basics start as `{}`, and other sections start as `[]`. Omitting `--before` appends the section. Missing targets, duplicates, out-of-range indices, and unsupported IDs produce errors. Removing a section removes all entries in it. Check the target with a dry run. Extension fields and other sections keep their order. Results identify changed paths and the new revision without including field values.

`set-field` accepts standard string fields. For local details use a dotted name such as `--section basics --field location.city`. List sections require a zero-based `--index`. Omit it for Basics. An empty `--value` removes the field. Setting an identical value preserves the exact file and revision. Schema validation still applies to URLs, emails and dates. Use JSON Patch for lists and extension fields, preferably with a `test` operation before the edit.

In an agent workflow, first get the revision with `inspect --json` and read only the targeted fields if needed. Use a dry run for the planned mutation. Apply only with the same revision. If the file changed, re-read both the revision and the relevant fields before deciding whether to retry. Use a `test` to confirm array item identity before editing by index.

## Export and appearance

Use `--format pdf|docx|md|txt`, `--out`, and optional `--lang zh|en`. The default is `zh`. Existing output requires explicit `--overwrite`, and output cannot replace the input résumé. All formats use the same semantic projection of standard sections. Unknown extension fields remain in the JSON file. Markdown escapes user text and creates links only for HTTP(S) URLs.

PDF and DOCX accept an absolute declarative v1/v2 theme path with `--theme`, including a standalone `*.theme.json` or the `theme.json` in a theme folder. The theme may specify A4 or Letter, margins, spacing, font roles, section styling, and supported header or sidebar arrangements. Unsupported constructs fail rather than silently falling back. Long content wraps, and an oversize split header can reflow across the page. DOCX preserves editable paragraphs and uses tables for supported split and sidebar layouts. Pagination can vary across readers. See the [declarative theme specification](../spec/resume-one-theme-spec-v2.md).

A theme can include up to four static TTF/OTF fonts of at most 8 MiB each. Reference them as `asset:<id>`. The loader verifies relative paths, hashes, file formats, and actual weights. Keep the complete package together. A verified catalog can carry its font and license files. Missing assets, variable fonts, WOFF/WOFF2, collections, and inconsistent face metadata fail. Two distinct font files with the same family and weight cannot be used interchangeably in a process.

```sh
resume-one export --file /absolute/resume.json --format docx --out /absolute/resume.docx --theme /absolute/themes/chosen.theme.json --lang en --json
resume-one export --file /absolute/resume.json --format pdf --out /absolute/resume.pdf --theme /absolute/themes/chosen.theme.json --appearance /absolute/appearance.json --json
```

`--appearance` requires `--theme`. It applies to that export, independently of desktop settings. The file is at most 64 KiB and contains the object directly, for example `{"formatVersion":1,"bodyFont":"font:Arial","headingFont":null,"colors":{"accent":"#52756B"}}`. Fonts can be `cjk-sans`, `cjk-serif`, or `font:<native family>`. Omit a role or set it to null to follow the theme. Color keys are `accent/paper/canvas/ink/muted/border/seal`, with `#RRGGBB` values. Invalid settings or unreadable files report `APPEARANCE_INVALID` or `APPEARANCE_IO` without modifying the output. See [Appearance Settings](../spec/appearance-settings.md).

`resume-one fonts list --json` lists native and app-installed font families. Use `resume-one fonts install --file /absolute/font.ttf --json` to import a TTF, OTF, TTC, or OTC into the app's font library. Optional `--dir` installs to a separate folder which the app does not automatically search. Font embedding permissions and glyph coverage are checked for export.

## Online themes

```sh
resume-one themes --repository owner/repository --ref main --cache /absolute/theme-cache list --json
resume-one themes --repository owner/repository --ref main --cache /absolute/theme-cache --offline download calm-paper --dir /absolute/local-themes --json
resume-one themes --cache /absolute/theme-cache download original-even --dir /absolute/local-themes --json
resume-one export --file /absolute/resume.json --out /absolute/resume.pdf --format pdf --theme /absolute/even.original.theme.json --json
```

A GitHub source defines a repository, ref, and index path. The CLI's default is `sctb512/resume-one-themes`, branch `main`, and `originals/index.json`. Pass `--index migrated-index.json` for the adapted catalog. With an explicit `--repository`, the default index is `themes/index.json`. These commands do not depend on desktop settings. Resolve the ref before downloading or supply a full 40-digit commit. Offline mode uses a verified cache, with no résumé input. A successful list reports metadata with `manifestsValidated: false`. When an older valid cache is used after a failed request, the result includes `stale: true` and a warning.

Download to an existing absolute `--dir`. Downloads verify package identity, hashes, resources and supported layout. A package with assets is installed as a folder, and `file` points to its `theme.json`. Equal bytes are reused. Conflicts and symlinks are rejected. The 256 MiB cache is separate from installed themes. Catalog and manifest limits are 1 MiB each and 500 catalog entries. Requests time out after 15 seconds and cannot fetch external theme references. [Catalog and cache contract](../spec/resume-one-theme-spec-v2.md#8-github-sources-and-cache)

Downloaded `.original.theme.json` packages can export PDF with local Chrome, Edge or Chromium. Set `RESUME_ONE_CHROMIUM` to its executable's absolute path if necessary. The original theme provides the print styles and font declarations. Appearance files apply to declarative themes. See [Original JSON Resume Theme Packages](../spec/json-resume-original-theme.md).

`verify-theme-source` checks `themes/index.json`, `themes/review.json`, listed manifests, and review evidence under `qa/`. It requires matching SHA-256 digests, v2 adapted provenance, compatible rendering, and traceable human review. Each theme requires 13 evidence files covering six English and Chinese examples and a multipage PDF. Screenshots are capped at 8 MiB each, PDFs at 30 MiB, and combined evidence at 512 MiB. The command rejects links, missing files, duplicate paths, and mismatched digests. Success reports `evidenceFilesVerified` and `structurallyVerified: true`. It checks signatures, not image similarity or independent proof of the review. Use public test résumés for evidence. See the [review schema](../spec/schemas/theme-source-review-v1.schema.json) and [release review contract](../spec/resume-one-theme-spec-v2.md#high-fidelity-acceptance).

## Independent theme migration tool

Use `resume-one-theme-migrate` for v1-to-v2 candidate conversion, static inspection of third-party source, and explicit approximation. A successfully parsed package can still need layout, visual, and licensing review.

```sh
resume-one-theme-migrate --source /absolute/old.theme.json --out /absolute/new.theme.json --license MIT --upstream-url https://example.com/theme --upstream-revision abc123 --dry-run
resume-one-theme-migrate --source /absolute/jsonresume-theme-example --out /absolute/example.theme.json --upstream-url https://github.com/owner/jsonresume-theme-example --dry-run
```

The source is never overwritten. A normal candidate write requires an output path, and overwriting an existing target requires `--overwrite`. v1 conversion preserves supported layout fields. Third-party candidates record provenance, safe visual clues and explicit gaps. The tool does not execute scripts. `--require-complete` fails when semantic compilation or required review remains incomplete.

### Adapt a theme or catalog

```sh
resume-one-theme-migrate --source /absolute/upstream --adapt --out /absolute/example.theme.json --verify-fixture /absolute/public-en.json --verify-fixture /absolute/public-zh.json
resume-one-theme-migrate --source /absolute/catalog.json --adapt --batch --out /absolute/new-theme-packages --font-library /absolute/font-library.json --verify-fixture /absolute/public-en.json --verify-fixture /absolute/public-zh.json --verify-fixture /absolute/public-long.json --verify-fixture /absolute/public-missing.json --verify-exports
```

`--adapt` projects static color, typography, heading and layout evidence into v2. It can combine supported print margins and padding, map genuine entry field bindings to `entryHeader`, and approximate recognized date formatting. Unknown CSS, dynamic styling and layout mismatches remain in the report. For example 19.05 mm of page margin plus 7.94 mm of container padding gives 26.99 mm and can use `page.firstLinePosition: "line-box"`. See [Theme Adaptation](../spec/theme-adaptation-v1.md) and [Module Display Order](../spec/theme-module-order-v1.md).

Batch mode uses a manifest of 1–256 source directories and optional provenance. It writes into a new output folder with per-theme packages, warnings, and `migration-report.json`. `--jobs` accepts 1–4 workers. The font library supports pinned static faces and accompanying licenses. Successful packages are preserved if some items fail, with exit code 3 and `ADAPT_BATCH_PARTIAL`. `--verify-fixture` checks public samples, recording hashes, page and section counts without copying content into the report. `--verify-exports` also checks PDF and DOCX serialization. Human review remains necessary. Adaptation is incompatible with `--require-complete`.

### Inspect complete source and compile a restricted template

```sh
resume-one-theme-migrate --inspect --source /absolute/upstream-theme
resume-one-theme-migrate --inspect --source /absolute/upstream-theme --require-complete
resume-one-theme-migrate --source /absolute/upstream-theme --inspect --template-entry src/Resume.jsx
```

Inspection reads a bounded source snapshot and emits one JSON report with `inspectionVersion: 1`, `sourceSnapshot`, `statistics`, `syntaxEvidence`, and `blockers`. It accepts a directory and does not write an output theme. A complete snapshot proves only that files within the declared boundary were read. Parse evidence, theme fidelity and licensing are separate. `--require-complete` retains the inspection evidence in the error when the operation cannot finish. See [Source Inspection](../spec/theme-migration-inspection-v1.md).

Up to eight named entry files can be requested. `templatePrograms` describes supported JSX nodes, lexical bindings, original positions, and remaining blockers. `program.resolved` applies only to the limited frontend. Defaults, optional chaining and parentheses retain their individual semantics. See the [Template IR](../spec/theme-template-ir-v1.md).

### Inspect explicit dependency packages and evaluate a sample

```sh
resume-one-theme-migrate --source /absolute/theme --inspect --template-entry src/Resume.jsx --dependency /absolute/core --dependency /absolute/utils
resume-one-theme-migrate --source /absolute/theme --inspect --template-entry src/Resume.jsx --dependency /absolute/core --dependency /absolute/utils --fixture /absolute/public-resume.json
```

Provide up to eight absolute dependency directories. The tool tracks exact export paths, fingerprints, and supported imports. `declarationResolved` identifies a declaration, while `dependencyGraphComplete` and `rootExternalImportsResolved` remain false. See [Dependency Snapshots](../spec/theme-dependency-bundle-v1.md) and [Module Declaration Linking](../spec/theme-module-links-v1.md).

`componentContent` maps the supported container of the pinned core Section after checking the actual binding, file hash and input shape. It preserves children and blockers. `wrapperContentLowered` does not certify styles or descendants. See [Shared Components](../spec/theme-shared-components-v1.md).

With `--fixture`, `contentEvaluation` produces an unstyled tree for the supplied, schema-valid public sample. It can preserve conditions, arrays, null and missing values, and the supported domains of Section, SectionTitle, DateRange and ContactInfo. Failed entries have no partial tree. This command can report the supplied sample text, so use public or redacted data. Content evaluation does not establish CSS, pagination, visual fidelity or complete migration. See [Content Evaluation](../spec/theme-content-evaluation-v1.md).
