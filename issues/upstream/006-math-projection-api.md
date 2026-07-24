# Proposal: structured math projection

## Status

Adopted in AdocWeave RC.3.

## Adoption result

AdocWeave exposes structured inline and block math data, including the language, source text, and
source range, through native and WASM projection APIs.

## Problem

Hosts need math language, raw source, inline/block kind, and source range for search, editor
diagnostics, and a separate safe math-rendering boundary. Reinterpreting HTML or reparsing source
loses the core AST boundary.

## Requested API

Expose math nodes through the standard document projection with language, source, display kind, and
source range. Preserve the same result in native and WASM projection APIs.

## Acceptance criteria

- Inline and block math are distinguishable.
- Projection retains exact source ranges and unexecuted raw math source.
- Projection does not require a particular math renderer.

## Out of scope

KaTeX, MathJax, Typst, or any browser-side math execution.
