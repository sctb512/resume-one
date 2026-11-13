# Bounded Template Intermediate Representation v1

**English** · [简体中文](./theme-template-ir-v1.zh-CN.md)

The stand-alone front end converts an explicit JSX entry to a data-only program. `resolved: true` certifies only completion of this bounded front-end stage. The result is separate from app themes, runtime evaluation, final migration, and visual acceptance.

## Inputs and Entry

```sh
resume-one-theme-migrate --source /absolute/upstream-theme --inspect --template-entry src/Resume.jsx
```

Select up to eight explicit entries already included in the [inspection snapshot](theme-migration-inspection-v1.md), with verified file and package SHA-256. Preserve blockers from unsupported inputs. The front end does not run scripts, imports, require, templates, or package metadata.

Require a single default export targeting a function declaration, expression, or synchronous arrow. Analyze the actual return with at most one props parameter, including explicit object destructuring, aliases, and undefined-only defaults. A block body permits bounded const declarations followed by a direct return. Helpers, factory calls, loops, async/generator constructs, conditional returns, and unknown statements block conversion.

A named export and a default export may point to one real binding, for example `export function Section(...) {...}` and `export default Section`. Named helpers retain an uncompiled function blocker. Adding export cannot make a mutable binding valid. Re-exports and export clauses go through separate module linking.

## Program Shape

Each `templatePrograms[]` entry has `entryFile`, `sourceSha256`, and `program`. The program includes:

- `format: "resume-one-template-ir-v1"`, source path and size, `syntaxValid`, and `resolved`.
- `entry`, the actual scoped function and its return, or null if unknown.
- `bindings`, with lexical IDs, names, values, scopes, declaration order, and source spans.
- `dependencies`, with non-executable evidence for imports/exports, styles, functions, and declarations.
- `blockers`, with reasons, original spans, and explicit excerpt truncation.
- `evaluationContract`, the semantics required of bounded data consumers.

The explicit `--fixture` [content evaluator](theme-content-evaluation-v1.md) implements a bounded subset using its own validation of components, types, and provenance. Only that step allows separately verifying unresolved styles, components, and dynamic links. Preserve front-end unresolved status throughout.

Use original UTF-8 byte ranges and one-based line/byte columns. Scope and binding IDs are traceable in the same program. List callback bindings stay lexical and cannot leak across callbacks. Reject read-before-initialization, unknown or conflicting bindings, mutation, and prototype-related fields.

## Nodes

| Node | Semantics |
|---|---|
| `function` | Scoped entry and return structure as data |
| `input` / `binding` | Explicit JSON props or lexical binding reference |
| `literal` / `undefined` / `empty` | Distinct bounded literal, missing value, and absent child |
| `default` | Fallback only for undefined |
| `get` | Explicit property access preserving optional chains, without computed or prototype reads |
| `logical` / `compare` / `not` | Short-circuit and scalar operands for `&&`, `\|\|`, `??`, `>`, `>=`, `<`, `<=`, `===`, `!==`, and `!` |
| `conditional` | Ternary with lazy branch evaluation |
| `group` | Explicit parentheses and optional-chain boundaries, as in `(obj?.a).b` |
| `map` | Dense JSON array path, synchronous lexical arrow, item/index, and optional member or call |
| `join` | Array data path with default comma or an explicit static separator and optional-chain boundaries |
| `text-template` / `to-string` | Untagged template fragments and explicit scalar string conversion |
| `element` / `fragment` | Ordered bounded elements, attributes, children, and original spans |
| `text` | Adjacent raw text and entities with JSX line whitespace rules, retaining segments, raw, and decoded value |
| `attribute` | Allowlisted data attribute. Retain duplicate evidence and block conversion. |
| `intrinsic` / `component-reference` | Allowlisted native tags and component references that still require linking |
| `blocked` | Unconverted source node retained for a consumer to reject |

Inputs are JSON data without getters, proxies, or prototype hooks. Mapping requires a dense array. Preserve optional-chain sentinels separately from undefined. Use finite JS numbers and only explicitly supported string decoding.

Templates normalize CRLF/CR to LF without folding indentation or decoding entities. Unhandled backslash escapes block conversion. Scalar conversion keeps strings, renders booleans and null/undefined as their literal text, and uses ECMAScript finite decimal spelling including -0 as 0. Arrays, objects, JSX, and mapped structures are outside this scalar domain. Unknown input types retain explicit constraints and need schema proof or validation.

Join requires a direct data path, zero arguments or one static string, and a dense scalar array. Null and undefined elements produce empty text. Reject function receivers, non-arrays that did not short-circuit, nested arrays or objects, dynamic separators, and spreads. Keep template literal, join, and JSX text rules independent.

Support explicitly listed named entities and valid numeric code points. Bare ampersands are text. Unknown entities block conversion. Attribute values do not use JSX child whitespace folding.

## Safety and Completion

Use explicit tag and attribute allowlists. Block event handlers, raw HTML, style objects, resource loading, ref, as, spread, dynamic/member components, and unverified URLs. Static URLs use allowlisted protocols and no controls. Dynamic URLs need an independent check. External components, styled bases, and CSS remain unresolved until linked.

Per file, limit size to 1 MiB, AST to 50000 nodes and depth 96, binding/dependency/blocker records to 8192, analysis to 200000 steps and two seconds. The package report stays under 16 MiB. Consumers reject unresolved programs, unknown nodes, fields or versions, and unsatisfied dependencies. Full layout compilation and multi-fixture verification remain separate and strict migration still fails.

## Style Inheritance

An entry may include a `styleGraph` of format `resume-one-style-inheritance-v1`. Link actual same-module styled declarations to module-scope binding IDs using exact name, span, binding type, and uniqueness. Keep source order, base, native terminal or unresolved import, and complete per-layer CSS rules in base-to-derived `layers`. Preserve duplicate declarations, selectors, media, and positions. Reject cycles, forward/uninitialized bases, conflicts, nested or mutable bindings, unsupported helpers, external primitives, dynamic styles, and nonvisual tags.

Limit graphs to 1024 definitions and 64 inheritance levels. `inheritanceLinked` checks the bounded nonempty graph and requires all blockers to be resolved. A node reports only its own linkage. `layoutCompiled` stays false. The graph does not establish CSS cascade, native fonts, margin collapse, line boxes, pagination, resource closure, or migration completion.
