//! 高entropyの不透明tokenを永続化前に一方向変換する。

use sha2::{Digest, Sha256};

pub(crate) fn hash_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}
