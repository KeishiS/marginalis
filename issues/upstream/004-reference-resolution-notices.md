# Proposal: successful reference resolutions with structured notices

## Status

Adopted in AdocWeave RC.3.

## Adoption result

AdocWeave provides `ResolutionNoticeKind::Fallback` on successful resolved references and
preserves it across native and WASM rendering and projection. Notice messages remain host-owned,
so the host-supplied message field proposed below is not required by Marginalis.

## Problem

Some hosts resolve a reference successfully with a safe fallback, for example by linking to a
document root when an optional fragment is absent. Current binary success/failure outcomes cannot
carry a warning to HTML, projection, LSP, or WASM consumers.

## Requested API

Allow a resolved reference to include zero or more structured notices containing a severity, stable
code, source range, and host-supplied plain-text message. Keep the href usable while exposing the
notice to HTML and non-HTML consumers.

## Acceptance criteria

- A resolved href can coexist with a warning notice.
- Native and WASM expose the same notice and render it through a safe, documented presentation hook.
- Notice text is escaped and cannot inject markup.

## Out of scope

Application localization, whether a fallback is appropriate, and authorization decisions.
