# Proposal: public typed queries for document attribute occurrences

## Status

Adopted in AdocWeave v0.6.0.

## Problem

A host sometimes needs more than the final resolved document-attribute map. Validation and
source-preserving edits require the occurrences in source order, including whether each occurrence
sets or unsets an attribute and the ranges of its name, value, and complete line.

AdocWeave v0.5.0 exposes final values through
`analysis.presentation().attributes()`. It also returns source occurrences from
`analysis.ast().attributes()`, but the concrete `DocumentAttribute` and `AttributeOperation` types
have no public module path. A consumer can infer an occurrence value and read some fields, but it
cannot name the type in an adapter API or portably match the set/unset variants.

Hosts should not need to parse attribute lines again or depend on private modules to implement
duplicate diagnostics, protected-value checks, or source-preserving edits.

## Requested API

Expose a stable typed query from the semantic facade. One possible shape is:

```rust
pub enum DocumentAttributeOperation {
    Set,
    Unset,
}

pub struct DocumentAttributeOccurrence<'a> {
    pub name: &'a str,
    pub raw_value: &'a str,
    pub operation: DocumentAttributeOperation,
    pub range: TextRange,
    pub name_range: TextRange,
    pub value_range: TextRange,
}

impl Analysis {
    pub fn document_attribute_occurrences(
        &self,
    ) -> impl ExactSizeIterator<Item = DocumentAttributeOccurrence<'_>>;
}
```

The exact ownership and iterator shape may differ. The required contract is that the query is
publicly nameable, preserves source order and duplicates, distinguishes set from unset, and returns
source ranges without performing I/O.

The final resolved map remains the preferred API for consumers that do not need occurrence
information.

## Acceptance criteria

- A host can inspect duplicate definitions in source order.
- Set, `:name!:`, and `:!name:` occurrences are distinguishable through a public enum or equivalent
  typed method.
- The full, name, and value ranges use the same source coordinates as diagnostics and the syntax
  tree.
- Empty set values remain distinguishable from unset operations.
- The API is available through a documented public facade and can be named in a host adapter's
  private implementation without importing a private module.
- Native consumers do not need to reparse source lines to perform source-preserving edits.
- Existing final-value queries remain unchanged.

## Out of scope

Application-specific attribute names, UUID or timestamp validation, database values, authorization,
and a generic source rewriting engine.

## Adoption result

AdocWeave v0.6.0 exposes
`Analysis::document_attribute_occurrences() -> &[DocumentAttributeOccurrence]` and re-exports
`DocumentAttributeOccurrence` and `DocumentAttributeOperation` from the `semantic` facade.
The public occurrence owns the name and raw value and preserves source order, duplicates, empty
sets, both unset forms, and the full, name, and value ranges. WASM and the browser library expose
the same facts as `attributeOccurrences`.
