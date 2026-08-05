//! Marginalisの永続化方式から独立した業務モデル。

mod bibliography_import;
mod model;
mod policy;

pub use bibliography_import::*;
pub use model::*;
pub use policy::{
    CITATION_STYLE_DOCUMENT_ATTRIBUTE, DOCUMENT_ATTRIBUTE_PREFIX, MAX_GRAPH_DEPTH, NOTE_POLICY,
    NotePolicy, TAGS_DOCUMENT_ATTRIBUTE,
};
