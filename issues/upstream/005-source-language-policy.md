# Proposal: source block language policy

## Problem

Sanitizing a source language into a `language-*` class is safe but does not let hosts reject or
diagnose unsupported languages consistently across rendering, projection, and editor tooling.

## Requested API

Add a host-configurable language policy with an allowlist and an unknown-language behavior such as
omit class, emit diagnostic, or preserve sanitized class. Do not embed a fixed language catalogue.

## Acceptance criteria

- A host can allow only selected language identifiers.
- Unknown-language behavior is deterministic in native and WASM output.
- The source body remains escaped and is never executed.

## Out of scope

Syntax highlighting implementation or code execution.
