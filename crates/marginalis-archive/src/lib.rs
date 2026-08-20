//! Marginalisの保存用archiveと、他の道具で読むための文書出力。
//!
//! 公開入口には、archiveのJSON表現と、検証済みsnapshotとの相互変換だけを置きます。
//! 保存契約の検証と移行は非公開moduleが担当し、呼び出し側が内部の処理順序へ依存しないようにします。

mod archive;
pub mod documents;

pub use archive::*;
