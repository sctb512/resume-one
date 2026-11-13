# Restricted Shared Component Content Mapping v1

**English** · [简体中文](./theme-shared-components-v1.zh-CN.md)

The migration tool maps the content boundary of selected shared components as data. It first identifies the declaration, then checks its pinned source and input domain. Unknown behavior remains a blocker.

## Section source identity

The first adapter targets the actual `Section` function in `src/primitives/Section.jsx` from `@jsonresume/core@0.3.6`, linked by [Module Declaration Linking](./theme-module-links-v1.md). Its full-file SHA-256 is:

```
51a0b42151195761af25b97113e9e838b7275054ec74432263e07e185c2c4c05
```

The source is pinned to jsonresume/jsonresume.org commit `542cd5315fc9995fca3c401d0c818767530bb6bd`. Package, version, path, hash, declaration and true lexical binding must all match. A named or default alias qualifies only when it resolves to the same declaration. The binding ID must match the import specifier in the template IR. Local variables shadowing the name do not qualify. styled(Section), attrs, polymorphic as, providers and arbitrary host forwarding are out of scope.

## Content contract

- Use the element and children IR without opening or changing the user's résumé or evaluating source expressions.
- Keep the section container even for missing children, null, false, empty text or arrays. Preserve numeric zero, child order, conditions, map nodes, and unknown children with their blockers.
- Accept literal string `id` and `className`. The default class is `resume-section`. For an explicit class, preserve the original prefix and ECMAScript trim behavior. Preserve internal spaces and duplicate classes.
- Preserve `key` as React identity without forwarding it to the DOM or discarding unresolved expressions.
- Reject dynamic IDs and classes, explicit children props, unknown props, spread, style/theme/ref/as/forwardedAs, and unsupported types. Strings are data and must be escaped by any consumer.

Mapping the container does not resolve its descendants. The result retains original program positions, bindings and blockers and cannot substitute empty children to pass.

## Report

`componentContent` uses `resume-one-section-content-adapter-v1`. `componentPrograms` identifies the pinned adapter and operations. `instances` records each call's IR location, binding evidence, original props and children, and its mapped `boundary` or blocker. `wrapperContentLowered` describes only the container, while `childrenResolved` stays false. Counts reflect static IR locations. `retainedBlockers` includes inspection, link and frontend gaps. A program with no target calls gains no speculative Section blocker.

Input and output limits are 16 MiB each, with 250,000 input nodes and depth 128. Traversal limits are 50,000 steps, depth 96, 2,048 call sites and three seconds. Up to 64 template programs, 4,096 import requests and eight dependencies are accepted. Reject oversized output rather than truncating it.

## Styling and acceptance

The mapping preserves the need to evaluate StyledSection's `margin-bottom` expression. Its truthiness choice uses `theme.spacing.section` or `var(--resume-space-section, 2rem)`. Numeric zero and `0px` differ. Print also specifies `page-break-inside: avoid`. Inheritance, cascade, variable resolution, units, margin collapse, line boxes and pagination remain layout work. `layoutCompiled` and `migrationComplete` remain false until those semantics and original visual fixtures are verified. Strict migration still refuses output.
