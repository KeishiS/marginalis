# Proposal: host-configurable STEM language profile

## Status

Adopted in AdocWeave RC.3.

## Adoption result

AdocWeave provides a host-configurable STEM-language policy with stable diagnostics and equivalent
native, LSP, and WASM behavior.

## Problem

Hosts may need to accept only a subset of otherwise supported STEM languages while preserving the
same validation in native, LSP, and WASM workflows.

## Requested API

Add an optional host-configurable allowed-STEM-language policy. The core must not prescribe which
languages a host enables.

## Acceptance criteria

- A host can allow only `latexmath`.
- Disallowed STEM languages have stable diagnostics and ranges.
- Native, LSP, and WASM apply the same policy.

## Out of scope

Adding a new math syntax or rendering a math language.
