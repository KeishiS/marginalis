# Proposal: resolver-provided display text and safe unresolved fallback

## Problem

When an `xref` has an empty label, rendering the raw scheme locator can disclose an internal
identifier or produce an unhelpful fallback. A host resolver may already have an authorized title
or other display text.

## Requested API

Allow a resolved reference to supply an optional escaped-as-text display override. Allow a failed
reference to supply a safe fallback display string that is used only when the authored label is
empty. The renderer must never interpret either string as HTML or inline AsciiDoc.

## Acceptance criteria

- An empty-label xref can render resolver-provided text.
- A host can use a generic non-identifying fallback for unresolved references.
- Explicit authored labels retain existing behavior.
- Override and fallback strings are HTML-escaped in native and WASM output.

## Out of scope

Lookup of titles, authorization, or scheme-specific semantics.
