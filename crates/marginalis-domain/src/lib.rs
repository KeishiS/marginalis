//! Marginalisの永続化方式から独立した業務モデル。

mod model;
mod policy;

pub use model::*;
pub use policy::{
    DOCUMENT_ATTRIBUTE_PREFIX, MAX_GRAPH_DEPTH, NOTE_POLICY, NotePolicy, TAGS_DOCUMENT_ATTRIBUTE,
};
