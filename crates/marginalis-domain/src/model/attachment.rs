//! ノートへ保存する画像と、版ごとの参照。

use core::{fmt, str::FromStr};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use super::{EntityId, NoteId, PrincipalRef, Revision, UnixMillis};

/// 添付画像の入力と保存量に適用する上限。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AttachmentPolicy {
    /// 画像1件の最大バイト数。
    pub max_bytes: usize,
    /// 一つのノートが保持できる画像数。過去版だけが参照する画像も数える。
    pub max_attachments_per_note: usize,
    /// 一つのノートが保持できる画像の合計バイト数。
    pub max_bytes_per_note: usize,
    /// 元のファイル名の最大文字数。
    pub max_file_name_characters: usize,
}

pub const ATTACHMENT_POLICY: AttachmentPolicy = AttachmentPolicy {
    max_bytes: 8 * 1024 * 1024,
    max_attachments_per_note: 32,
    max_bytes_per_note: 64 * 1024 * 1024,
    max_file_name_characters: 200,
};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct AttachmentId(EntityId);

impl AttachmentId {
    pub const fn new(value: EntityId) -> Self {
        Self(value)
    }

    pub const fn entity_id(self) -> EntityId {
        self.0
    }
}

impl FromStr for AttachmentId {
    type Err = super::InvalidEntityId;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.parse().map(Self::new)
    }
}

impl fmt::Display for AttachmentId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// ブラウザーへ能動的な内容として解釈されない、初期対応の静止画像形式。
#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "lowercase")]
pub enum AttachmentMediaType {
    #[serde(rename = "image/png")]
    Png,
    #[serde(rename = "image/jpeg")]
    Jpeg,
    #[serde(rename = "image/webp")]
    WebP,
}

impl AttachmentMediaType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::WebP => "image/webp",
        }
    }

    /// 申告値や拡張子を使わず、内容の固定signatureから形式を判定する。
    pub fn detect(bytes: &[u8]) -> Option<Self> {
        if bytes.len() >= 24
            && bytes.starts_with(b"\x89PNG\r\n\x1a\n")
            && bytes.get(12..16) == Some(b"IHDR")
            && bytes
                .get(16..24)
                .is_some_and(|dimensions| dimensions[..4] != [0; 4] && dimensions[4..] != [0; 4])
        {
            return Some(Self::Png);
        }
        if bytes.len() >= 4
            && bytes.starts_with(&[0xff, 0xd8, 0xff])
            && bytes.ends_with(&[0xff, 0xd9])
        {
            return Some(Self::Jpeg);
        }
        if bytes.len() >= 16
            && bytes.starts_with(b"RIFF")
            && bytes.get(8..12) == Some(b"WEBP")
            && matches!(
                bytes.get(12..16),
                Some(b"VP8 ") | Some(b"VP8L") | Some(b"VP8X")
            )
        {
            return Some(Self::WebP);
        }
        None
    }
}

impl FromStr for AttachmentMediaType {
    type Err = InvalidAttachmentMediaType;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "image/png" => Ok(Self::Png),
            "image/jpeg" => Ok(Self::Jpeg),
            "image/webp" => Ok(Self::WebP),
            _ => Err(InvalidAttachmentMediaType),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("attachment media type is invalid")]
pub struct InvalidAttachmentMediaType;

/// HTTP入力から検査した、まだ保存していない画像。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttachmentDraft {
    file_name: String,
    media_type: AttachmentMediaType,
    bytes: Vec<u8>,
}

impl AttachmentDraft {
    pub fn new(file_name: String, bytes: Vec<u8>) -> Result<Self, InvalidAttachment> {
        let file_name = file_name.trim().nfc().collect::<String>();
        if file_name.is_empty()
            || file_name.chars().count() > ATTACHMENT_POLICY.max_file_name_characters
            || file_name
                .chars()
                .any(|character| character.is_control() || matches!(character, '/' | '\\'))
            || matches!(file_name.as_str(), "." | "..")
        {
            return Err(InvalidAttachment::InvalidFileName);
        }
        if bytes.is_empty() {
            return Err(InvalidAttachment::Empty);
        }
        if bytes.len() > ATTACHMENT_POLICY.max_bytes {
            return Err(InvalidAttachment::TooLarge);
        }
        let media_type =
            AttachmentMediaType::detect(&bytes).ok_or(InvalidAttachment::UnsupportedMediaType)?;
        Ok(Self {
            file_name,
            media_type,
            bytes,
        })
    }

    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    pub const fn media_type(&self) -> AttachmentMediaType {
        self.media_type
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    pub fn into_stored(
        self,
        attachment_id: AttachmentId,
        note_id: NoteId,
        created_at: UnixMillis,
        created_by: PrincipalRef,
    ) -> StoredAttachment {
        let sha256: [u8; 32] = Sha256::digest(&self.bytes).into();
        let metadata = AttachmentMetadata::new(
            attachment_id,
            note_id,
            self.file_name,
            self.media_type,
            self.bytes.len(),
            sha256,
            created_at,
            created_by,
        )
        .expect("validated attachment draft produces valid metadata");
        StoredAttachment {
            metadata,
            bytes: self.bytes,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum InvalidAttachment {
    #[error("attachment file name is invalid")]
    InvalidFileName,
    #[error("attachment is empty")]
    Empty,
    #[error("attachment is too large")]
    TooLarge,
    #[error("attachment media type is unsupported")]
    UnsupportedMediaType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttachmentMetadata {
    attachment_id: AttachmentId,
    note_id: NoteId,
    file_name: String,
    media_type: AttachmentMediaType,
    byte_length: usize,
    sha256: [u8; 32],
    created_at: UnixMillis,
    created_by: PrincipalRef,
}

impl AttachmentMetadata {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        attachment_id: AttachmentId,
        note_id: NoteId,
        file_name: String,
        media_type: AttachmentMediaType,
        byte_length: usize,
        sha256: [u8; 32],
        created_at: UnixMillis,
        created_by: PrincipalRef,
    ) -> Result<Self, InvalidAttachment> {
        // 復元時もHTTP入力と同じ名前・容量規則を適用する。形式とbytesの一致は
        // `StoredAttachment::new`が検査する。
        let file_name = file_name.trim().nfc().collect::<String>();
        if file_name.is_empty()
            || file_name.chars().count() > ATTACHMENT_POLICY.max_file_name_characters
            || file_name
                .chars()
                .any(|character| character.is_control() || matches!(character, '/' | '\\'))
            || matches!(file_name.as_str(), "." | "..")
        {
            return Err(InvalidAttachment::InvalidFileName);
        }
        if byte_length == 0 {
            return Err(InvalidAttachment::Empty);
        }
        if byte_length > ATTACHMENT_POLICY.max_bytes {
            return Err(InvalidAttachment::TooLarge);
        }
        Ok(Self {
            attachment_id,
            note_id,
            file_name,
            media_type,
            byte_length,
            sha256,
            created_at,
            created_by,
        })
    }

    pub const fn attachment_id(&self) -> AttachmentId {
        self.attachment_id
    }

    pub const fn note_id(&self) -> NoteId {
        self.note_id
    }

    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    pub const fn media_type(&self) -> AttachmentMediaType {
        self.media_type
    }

    pub const fn byte_length(&self) -> usize {
        self.byte_length
    }

    pub const fn sha256(&self) -> &[u8; 32] {
        &self.sha256
    }

    pub const fn created_at(&self) -> UnixMillis {
        self.created_at
    }

    pub const fn created_by(&self) -> &PrincipalRef {
        &self.created_by
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredAttachment {
    metadata: AttachmentMetadata,
    bytes: Vec<u8>,
}

impl StoredAttachment {
    pub fn new(metadata: AttachmentMetadata, bytes: Vec<u8>) -> Result<Self, InvalidAttachment> {
        if bytes.len() != metadata.byte_length
            || AttachmentMediaType::detect(&bytes) != Some(metadata.media_type)
            || <[u8; 32]>::from(Sha256::digest(&bytes)) != metadata.sha256
        {
            return Err(InvalidAttachment::UnsupportedMediaType);
        }
        Ok(Self { metadata, bytes })
    }

    pub const fn metadata(&self) -> &AttachmentMetadata {
        &self.metadata
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NoteRevisionAttachment {
    pub note_id: NoteId,
    pub revision: Revision,
    pub attachment_id: AttachmentId,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_only_the_supported_image_signatures() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01".to_vec();
        png.extend_from_slice(b"payload");
        assert_eq!(
            AttachmentMediaType::detect(&png),
            Some(AttachmentMediaType::Png)
        );
        assert_eq!(
            AttachmentMediaType::detect(b"\xff\xd8\xffpayload\xff\xd9"),
            Some(AttachmentMediaType::Jpeg)
        );
        assert_eq!(
            AttachmentMediaType::detect(b"RIFF\x04\0\0\0WEBPVP8 payload"),
            Some(AttachmentMediaType::WebP)
        );
        assert_eq!(AttachmentMediaType::detect(b"<svg></svg>"), None);
    }

    #[test]
    fn validates_file_names_and_limits() {
        let jpeg = b"\xff\xd8\xffpayload\xff\xd9".to_vec();
        let draft = AttachmentDraft::new(" 図.jpg ".into(), jpeg.clone()).expect("image");
        assert_eq!(draft.file_name(), "図.jpg");
        assert_eq!(draft.media_type(), AttachmentMediaType::Jpeg);
        assert_eq!(
            AttachmentDraft::new("../図.jpg".into(), jpeg.clone()),
            Err(InvalidAttachment::InvalidFileName)
        );
        assert_eq!(
            AttachmentDraft::new("図.svg".into(), b"<svg/>".to_vec()),
            Err(InvalidAttachment::UnsupportedMediaType)
        );
        assert_eq!(
            AttachmentDraft::new("empty.png".into(), Vec::new()),
            Err(InvalidAttachment::Empty)
        );
    }
}
