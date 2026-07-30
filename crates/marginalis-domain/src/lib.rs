//! Marginalisの永続化方式から独立した業務モデル。

mod model;
mod policy;

pub use model::*;
pub use policy::{NOTE_POLICY, NotePolicy};
