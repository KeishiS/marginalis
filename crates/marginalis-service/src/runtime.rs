//! 時刻と暗号学的乱数をproduction環境へ接続するadapter。

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use marginalis_application::{Clock, Random};
use marginalis_domain::{EntityId, UnixMillis};
use uuid::Uuid;

/// production組立時に使うUTC millisecond clock。
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> UnixMillis {
        UnixMillis::new(time::OffsetDateTime::now_utc().unix_timestamp_nanos() as i64 / 1_000_000)
    }
}

/// UUIDv7と暗号学的に安全な不透明tokenを生成する実行環境adapter。
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemRandom;

impl Random for SystemRandom {
    fn uuid_v7(&self) -> EntityId {
        EntityId::try_from_uuid(Uuid::now_v7()).expect("Uuid::now_v7 must generate UUIDv7")
    }

    fn opaque_token(&self) -> String {
        let bytes: [u8; 32] = rand::random();
        URL_SAFE_NO_PAD.encode(bytes)
    }
}
