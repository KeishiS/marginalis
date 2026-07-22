# Proposal: structured fixed attributes for external links

## Problem

Hosts commonly need external `http` and `https` links to open in a new tab with
`rel="noopener noreferrer"`. Post-processing HTML is brittle and weakens the renderer's security
boundary.

## Requested API

Add a render-policy option that classifies external URLs and emits only fixed, structured link
attributes. A configuration should be able to request `target="_blank"` and
`rel="noopener noreferrer"` for allowed external links, without arbitrary user-provided attributes.

## Acceptance criteria

- Allowed external links receive the configured fixed attributes.
- Resolved application links do not receive them unless explicitly classified as external.
- Input cannot inject or override `target`, `rel`, event handlers, or style attributes.

## Out of scope

Fetching external URLs or application-specific URL classification.
