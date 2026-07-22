# Proposal: separate authored and resolver-produced URL policy, with WASM parity

## Problem

An authored link and a URL returned by a trusted host resolver have different trust boundaries.
Hosts need to allow only `http` and `https` in source text while allowing a resolver to return a
validated absolute HTTPS application URL. Applying one policy to both either over-permits authored
input or rejects valid resolved references. Native and WASM rendering must apply the same policy.

## Requested API

Add a structured URL context such as `AuthoredLink`, `ResolvedReference`, and `ResolvedResource`,
with independently configurable policies. The render policy supplied through the WASM API must be
the same policy used for native HTML output.

## Acceptance criteria

- An authored `javascript:` URL is rejected in native and WASM output.
- A resolver-produced absolute HTTPS URL is accepted without enabling broader authored URLs.
- Native and WASM produce identical HTML and diagnostics for the same AST, inputs, and policy.

## Out of scope

Host routing, URL construction, authentication, and database lookup.
