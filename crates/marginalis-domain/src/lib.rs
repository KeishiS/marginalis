//! Marginalisの永続化方式から独立した業務モデル。

mod model;
mod policy;

pub use model::*;
pub use policy::{MAX_GRAPH_DEPTH, NOTE_POLICY, NotePolicy};
