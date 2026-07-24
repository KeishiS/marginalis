# Proposal: explicit resource-resolution profile

## Status

Adopted in AdocWeave RC.3.

## Adoption result

AdocWeave provides a restrictive resource profile without causing the core parser or renderer to
perform I/O. Native and WASM integrations apply the same restrictions.

## Problem

Pure parsing performs no I/O, but hosts benefit from an explicit profile that prevents accidental
enablement of includes, external images, attachments, or resource fetching as integrations evolve.

## Requested API

Add a declarative host profile that controls include handling, image/resource syntax treatment,
attachment handling, and external-resource resolution. The default remains pure and performs no
I/O.

## Acceptance criteria

- A restrictive profile rejects or diagnoses disabled resource constructs with ranges.
- Enabling the profile never causes core parsing or rendering to fetch a resource.
- Native and WASM have matching behavior.

## Out of scope

Filesystem access, HTTP fetching, attachment storage, and host authorization.
