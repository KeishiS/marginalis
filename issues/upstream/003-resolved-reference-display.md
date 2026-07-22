# Proposal: resolver-provided display text and safe unresolved fallback

## Status

AdocWeave RC.3 adds `UnresolvedReferencePresentation` (`Target`、`LabelOnly`、`Hidden`) and
therefore provides a safe non-disclosing fallback policy. Resolver-provided display text for an
empty authored label remains unavailable; this proposal is retained for that part only.

## Problem

When an `xref` has an empty label, rendering the raw scheme locator can disclose an internal
identifier or produce an unhelpful fallback. A host resolver may already have an authorized title
or other display text.

## Requested API

Add an optional plain-text `display_text` field to successful generic reference resolutions.

```rust
ResolutionOutcome::Resolved {
    href: String,
    display_text: Option<String>,
    notices: Vec<ResolutionNotice>,
}
```

The equivalent WASM field is `displayText`. Do not add application schemes, UUID validation, or
title lookup to AdocWeave.

The renderer must apply these rules in order:

1. An authored non-empty label wins.
2. For an empty label, use resolver `display_text` when supplied.
3. Otherwise retain the current target-text fallback.
4. On failure, never use `display_text`; apply `UnresolvedReferencePresentation` instead.

`display_text` is plain text: HTML-escape it and never parse it as HTML or inline AsciiDoc.

## Acceptance criteria

- An empty-label xref can render resolver-provided text.
- An authored non-empty label remains unchanged even when `display_text` is supplied.
- A failed reference cannot render a successful resolver's `display_text`.
- A host can use a generic non-identifying fallback for unresolved references.
- Explicit authored labels retain existing behavior.
- `display_text` is HTML-escaped in native and WASM output.
- Native, WASM, and projection preserve the same successful display-text resolution without
  exposing it for failed resolutions.

## Out of scope

Lookup of titles, authorization, or scheme-specific semantics.
