# Migration Dependency Snapshots v1

**English** · [简体中文](./theme-dependency-bundle-v1.zh-CN.md)

The migration tool can inspect explicit dependency source packages without installing them. Its report identifies fixed inputs and outstanding gaps. Limited declaration linking is covered separately in [Module Declaration Linking](./theme-module-links-v1.md).

```sh
resume-one-theme-migrate --source /absolute/theme --inspect --template-entry src/Resume.jsx --dependency /absolute/core --dependency /absolute/utils
```

## Input and limits

Use `--dependency` with `--inspect` and provide at most eight absolute UTF-8 package paths. The tool does not search parents or node_modules, run scripts, execute imports or loaders, or download missing packages. A sync provider may transparently retrieve the files selected by the user.

Apply the [source snapshot rules](./theme-migration-inspection-v1.md) to every package. Reject symlinks, nonportable paths, and detected changes during reading. Dependencies have a combined 64 MiB limit in addition to the root snapshot's own limits. Packages are read sequentially, so fingerprints identify individual snapshots rather than a simultaneous filesystem snapshot.

The root package.json is an object of at most 256 KiB and 4,096 keys, with no duplicate keys after decoding escapes. Names follow a bounded lowercase npm subset. Versions must be exact SemVer versions, without ranges, tags, workspace or link forms. Reject repeated dependency names across versions and collisions with the root package name.

## Exports and imports

Inspect a string `exports` value or explicit `.` and `./subpath` mappings to strings. The target is a regular `./` path inside the package. Escapes, encoded separators, backslashes, URLs, drive letters, query or fragment loaders, wildcards and node_modules paths fail. Conditional objects, arrays and mixed mappings are unsupported. The tool does not infer a Node fallback through main or module.

| Status | Meaning |
| --- | --- |
| `file-present` | Exact export target is in the snapshot and has a SHA-256 |
| `missing` | Target is absent. A neighboring extension or folder entry does not count |
| `unsupported` | Target, branch, or path falls outside the supported rules |

The syntax frontend scans JS, JSX and CSS. Relative imports receive bounded lexical candidate checks. Preserve ambiguity when several extensions might match. A single extension guess still does not establish Node resolution. Bare imports remain unresolved at this stage even if a dependency has the same name. Preserve blockers for compiled formats, CSS imports, dynamic styles and failed parses. Observed imports do not establish closure over fonts, images, or other resources.

## Report and completion

`dependencyBundle` includes `dependencyBundleVersion: 1`, limits, scope, count and total bytes. Name-sorted `fingerprints` contain package name, version, root, source SHA-256 and package.json SHA-256. Each row in `packages` includes the source snapshot, metadata, export records, import evidence and blockers. The summary tracks external imports and attributes blockers to `packageName`.

`dependencyGraphComplete`, `rootExternalImportsResolved`, `originalRenderingVerified`, `migrationComplete`, `visualVerified`, and `licenseVerified` remain false. For actions of this tool, `executedSourceCode`, `sourceModified`, and `networkAccessed` are always false.

The report budget is 30 seconds, 16,384 import records per package, 4,096 aggregate blockers and 16 MiB. The final merged report also obeys the blocker and size limits. Reject errors and oversized output without returning a partial success report. Bad input returns code 3 and `DEPENDENCY_BUNDLE_ERROR` with a diagnostic code. A collision with the root reports `DEPENDENCY_DUPLICATES_SOURCE`. Strict inspection retains completed evidence in `error.inspection` when `--require-complete` fails.
