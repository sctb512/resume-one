# Theme Migration Source Inspection v1

**English** · [简体中文](./theme-migration-inspection-v1.zh-CN.md)

This protocol defines the read-only front end of `resume-one-theme-migrate`. It inspects source evidence. Declarative themes still prohibit JavaScript execution, network requests, and arbitrary expressions.

## Commands

```sh
resume-one-theme-migrate --source /absolute/path/to/theme --inspect
resume-one-theme-migrate --source /absolute/path/to/theme --inspect --require-complete
```

Use an absolute directory path. `--inspect` is mutually exclusive with output, dry-run, overwrite, source license, and version override flags. Inspect without executing, installing, or downloading code, rendering private résumés, or modifying the input. Package samples and font files only contribute static source evidence and fingerprints.

Normal output is one JSON line with `protocolVersion: 1`, `ok: true`, and `result`. Invalid arguments exit 2. Read, bounds, parsing budget, and strict completeness failures exit 3 with `ok: false`, `error.code`, and `error.message`. Successful non-strict inspection confirms report generation.

## Source Snapshot

Read all regular files within the inspection boundary including deep components, binary fonts, licenses, and package manifests. Exclude only directories named `.git`, `node_modules`, or `target` and report them. Same-named regular files remain in scope.

Limit paths to 16 levels, files to 1024, directory entries to 4096, each file to 8 MiB, and the total to 64 MiB. Reject symlinks at or above the root and within it, special files, case collisions, changes detected across reads, and nonportable path components. Use slash-separated portable ASCII paths with no Windows reserved names, trailing spaces/dots, or dangerous separators. Exceeding a limit fails the snapshot.

Sort files by relative path and record length, SHA-256, and UTF-8 status. Do not embed binaries. Construct the snapshot SHA-256 from the byte domain `resume-one-source-bundle\0v1\0` followed for each file by 8-byte big-endian path length, UTF-8 relative path, 8-byte big-endian content length, and raw content. Each `\0` denotes one zero byte. `sourceSnapshot.complete` is scoped to these reads. Identity and metadata rechecks detect ordinary concurrent edits and do not provide an atomic filesystem snapshot.

## Syntax Evidence

Pin tree-sitter 0.25.10, the vendored tree-sitter-javascript 0.25.0, and tree-sitter-css 0.25.0. See [third-party notices](../licenses/THIRD_PARTY_LICENSES.md) for the JS grammar hash and patch. Cargo.lock records registry dependencies and hashes. Parse JS, JSX, MJS, CJS, and CSS. Other potential style or template formats require explicit adapter blockers.

Record imports/re-exports, require and dynamic import, styled aliases and bases, declaration order and duplicates, nested selectors, media/print contexts, JSX expressions and direct field paths. Exclude fake declarations from comments and ordinary strings. Evidence alone establishes no cascading or layout behavior.

Use original byte spans `[startByte, endByte)` with one-based lines and UTF-8 byte columns (`columnUnit: "utf8-bytes"`). Limit display excerpts to 2048 bytes at UTF-8 boundaries and mark truncated excerpts. Compile only from original snapshot bytes. Map wrapped CSS coordinates back to the source. The vendored JSX scanner recognizes bare ampersands as original text while preserving character references and source coordinates. It neither rewrites input nor suppresses parser errors. The report identifies `resume-one-jsx-ampersand-v1`. [JSX text grammar](https://github.com/facebook/jsx/blob/main/spec.emu)

Unresolved interpolations, template escapes, non-top-level bindings, and unclassified templates remain explicit. Syntax errors or mismatched JSX tags set `syntaxValid: false`. Bound each parse to two seconds, 100000 named nodes per file, 12000 records per evidence type, and 128 levels of CSS context. Bound the package check to 30 seconds, 4096 blockers, and 16 MiB of report output. Return an error when limits are exceeded.

### Bounded Constant Folding

Evaluate direct styled interpolations from the AST without running JavaScript. Allow unique simple top-level const bindings containing unescaped strings, decimal numbers, previously initialized aliases, grouping, and numeric addition/subtraction/multiplication/division. Reject string concatenation, implicit coercion, mutable or conflicting bindings even elsewhere in the file, reads before initialization, dynamic scope, and explicit eval/Function/global access. Calls remain unresolved.

Limit strings to 4096 bytes, expression/reference depth to 64, steps to 1024 per evaluation, and bindings to 4096. A number must be finite and zero or have magnitude from `1e-6` to `1e12`. Decimal output uses at most 12 significant digits. Record causes for unsupported expressions including templates, calls, members, imports, props, objects/getters, division by zero, or excess precision.

`constantSubstitutions` records original spans and values. `dynamicExpressions` retains unresolved members with reasons. Emit `css: null` if any member or template escape remains unresolved. Cap expanded text at 8 MiB per file before parsing CSS with its grammar. In `sourceMapping: "constant-folded"`, literal ranges map exactly and substitution ranges point to the whole original interpolation. Order and duplicate declarations are kept.

## Dependencies and Status

Relative import checks use bounded lexical paths and existence checks. Escapes, missing files, and unresolved expressions block migration. Bare imports require explicitly locked dependencies. CSS imports do not fetch URLs. License files and manifest declarations need separate review.

The report includes `inspectionVersion`, `sourceSnapshot`, `parsers`, `package`, `statistics`, `cssProperties`, `externalImports`, `licenseFiles`, `syntaxEvidence`, and `blockers`. It tracks `migrationComplete`, `visualVerified`, and `licenseVerified` separately. Inspection keeps all three false and records `DECLARATIVE_COMPILER_REQUIRED` and `VISUAL_ACCEPTANCE_REQUIRED`. `executedSourceCode` and `sourceModified` are always false. Strict mode fails with the report under `inspection` and publishes no theme. Legacy candidate generation remains available with `migrationComplete: false` and also rejects strict output or overwrite.

Use up to eight unique `--template-entry src/Resume.jsx` paths within the frozen snapshot. Only JS/JSX/MJS/CJS with portable relative paths and no empty, dot, parent, or backslash components are accepted. Each `templatePrograms` entry records file, SHA-256, and program. `program.resolved` reflects only front-end resolution. Up to eight explicit absolute dependency directories can be inspected with repeatable `--dependency`. Dependency snapshots do not link imports by themselves. The combined report retains all bounds and strict gates. [Template IR](theme-template-ir-v1.md) · [Dependency bundle](theme-dependency-bundle-v1.md)

## Compilation and Acceptance

Finish dependency and resource closure, bounded template and style semantics, and projection into shared layout before visual checks. Compare the original on identical paper, fonts, and inputs. Unmapped rules retain source locations and block full acceptance. Verify with multiple fixtures, visual metrics, and review. Candidate editing, successful parsing, theme count, and similar colors cannot certify exact migration.
