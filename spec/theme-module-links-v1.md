# Theme Module Declaration Linking v1

**English** · [简体中文](./theme-module-links-v1.zh-CN.md)

The static frontend connects actual imports, re-exports and declarations. It tracks declarations without evaluating the module or claiming that a component or theme has been migrated.

## Inputs and outputs

Use `--inspect --template-entry ... --dependency ...`. All evidence comes from the same bounded snapshot. Derive `syntaxEvidence[].moduleSymbols` from the full Tree-sitter CST and original bytes. The linker does not reopen source files or parse truncated snippets.

`moduleSymbols` uses `resume-one-module-symbols-v1`. It includes imports with local/module/imported identities and `localSpan`, exports with local or re-export bindings, and declarations with IDs, kinds, mutability and initializer evidence. Byte spans and one-based UTF-8 columns preserve position, alongside top-level order, side effects and blockers. An import specifier span and its local identifier span serve different roles. Anonymous default functions have synthetic identities. A function expression's private name is scoped to the function, and const aliases keep their own identity.

`moduleLinks` uses `resume-one-module-links-v1`. Each entry in `requests` preserves the exact local and exported mappings and source location, then returns `declarationResolved`, `trace`, and a `target` or `blocker`. The trace carries package/version, package and file hashes, queried symbol, location and live versus snapshot export form. `statistics` and `allRequestedDeclarationsResolved` describe declaration resolution on a nonempty request set. A found declaration still retains mutability. `bindingValueProven`, `semanticsCompiled`, `moduleEvaluationProven`, `componentSemanticsCompiled`, `dependencyGraphComplete` and `migrationComplete` do not follow from a successful link. A component adapter separately verifies its pinned source and supported domain.

## Resolution rules

1. Resolve a bare import to one explicitly supplied package. Follow only exact selections with verified SHA-256 from the [dependency snapshot](./theme-dependency-bundle-v1.md). Do not guess main/module, conditional exports, or extensions.
2. Normalize relative `.` and `..` lexically inside the snapshot. Reject escapes, node_modules, encoded or escaped separators, URLs, loaders, backslashes and empty segments. A missing exact file stays missing.
3. Follow explicit named and default exports, re-exports, local aliases and declarations. Private names do not match exports by coincidence. Star exports and namespaces are not enumerated. No dynamic factory or unresolved expression is evaluated.
4. Preserve default snapshot and named live binding differences. Reject duplicate or unknown bindings, unsupported import attributes, syntax errors, ambiguity and cycles.
5. Resolving one explicit barrel export can succeed while an unused export remains missing. Keep the other blocker and the unresolved package state.

Symbol evidence is capped at 8 MiB per file, 100,000 CST nodes, depth 128, 8,192 records and three seconds. Linking is capped at 4,096 requests, 16,384 steps, depth 64 per request and ten seconds. The final report still applies the 16 MiB and 4,096 blocker limits. Oversize reports fail. Until layout, components, and visual review are complete, `--require-complete` returns code 3 without writing a candidate.
