//! Webhook配送のportと、1回分の配送を組み立てるworker。
//!
//! 配送の永続状態（共通変更記録から派生した配送行とlease）はrepository portが、外部URLへのHTTP送信は
//! sender portが担う。workerは両者を組み合わせて「期限の来た配送を取得し、
//! 送信し、結果を記録する」1回分(tick)だけを持ち、常駐のループと停止は
//! composition root(service)が受け持つ。

use async_trait::async_trait;
use marginalis_domain::{Actor, UnixMillis};

use crate::{Clock, Random, StorageError};

/// payloadの契約版。受信側が形の変更を検知できるように本文へ含める。
pub const WEBHOOK_CONTRACT_VERSION: u32 = 1;
/// 配送の試行上限。超えたsubscriptionは停止理由付きで無効化する。
pub const WEBHOOK_MAX_ATTEMPTS: u32 = 10;
/// 再試行間隔の初期値と上限。指数backoffで増やす。
pub const WEBHOOK_BACKOFF_BASE_MS: i64 = 5_000;
pub const WEBHOOK_BACKOFF_MAX_MS: i64 = 3_600_000;
/// 1回の取得で配送を占有する期間。期限が切れた占有は他の取得で引き継げる。
pub const WEBHOOK_LEASE_MS: i64 = 60_000;
/// 配送済みeventの保持期間。
pub const WEBHOOK_RETENTION_MS: i64 = 7 * 24 * 60 * 60 * 1000;
/// 1 tickで取得する配送数の上限(同時配送数)。
pub const WEBHOOK_DELIVERY_BATCH: u32 = 4;

/// 配送するevent。本文やCSL-JSONは含まない。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebhookEvent {
    pub event_id: String,
    pub kind: String,
    pub sequence: i64,
    pub target_id: String,
    pub revision: i64,
    pub occurred_at: UnixMillis,
}

/// 取得した配送1件。subscriptionごとに順序の先頭だけが取得される。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebhookPendingDelivery {
    pub subscription_id: String,
    pub url: String,
    pub secret: String,
    /// 今回を含めない、これまでの試行回数。
    pub attempt_count: u32,
    pub event: WebhookEvent,
}

/// 配送の失敗分類。DBのlast_failureと同じ語彙を使う。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebhookDeliveryFailure {
    NonSuccessStatus,
    ConnectFailed,
    TimedOut,
    DestinationRejected,
}

impl WebhookDeliveryFailure {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NonSuccessStatus => "non_success_status",
            Self::ConnectFailed => "connect_failed",
            Self::TimedOut => "timed_out",
            Self::DestinationRejected => "destination_rejected",
        }
    }
}

/// 配送状態の永続化。実装はSQLite adapterが持つ。
#[async_trait]
pub trait WebhookDeliveryRepository: Send + Sync {
    /// 期限が来た配送を、subscriptionごとの先頭からlease付きで取得する。
    async fn claim_due_deliveries(
        &self,
        now: UnixMillis,
        lease_until: UnixMillis,
        limit: u32,
    ) -> Result<Vec<WebhookPendingDelivery>, StorageError>;
    async fn record_delivered(
        &self,
        subscription_id: &str,
        event_sequence: i64,
        delivered_at: UnixMillis,
    ) -> Result<(), StorageError>;
    /// 失敗を記録して次回試行を予約し、leaseを解放する。
    async fn record_failed(
        &self,
        subscription_id: &str,
        event_sequence: i64,
        failure: WebhookDeliveryFailure,
        attempt_count: u32,
        next_attempt_at: UnixMillis,
        attempted_at: UnixMillis,
    ) -> Result<(), StorageError>;
    /// 試行上限へ達したsubscriptionを無効化する。配送行は表示と再送のため残す。
    async fn disable_exhausted_subscription(
        &self,
        subscription_id: &str,
        disabled_at: UnixMillis,
    ) -> Result<(), StorageError>;
    /// 保持期間を過ぎた配送済み状態と、どの用途からも参照されない変更記録を削除する。
    async fn purge_expired_deliveries(&self, now: UnixMillis) -> Result<u64, StorageError>;
}

/// 外部URLへの送信。実装は送信直前の宛先検査と署名を行う。
#[async_trait]
pub trait WebhookDeliverySender: Send + Sync {
    /// 署名付きで本文を送る。2xx以外は失敗として分類する。
    async fn deliver(
        &self,
        url: &str,
        secret: &str,
        sent_at: UnixMillis,
        body: &str,
    ) -> Result<(), WebhookDeliveryFailure>;
    /// 送信先の所有確認。challengeを署名付きで送り、応答本文への同じ値の
    /// 出現を確認する。
    async fn verify_destination(
        &self,
        url: &str,
        secret: &str,
        sent_at: UnixMillis,
        challenge: &str,
    ) -> Result<(), WebhookDeliveryFailure>;
}

/// 受信側へ送るJSON本文。契約はこの形だけを保証する。
pub fn webhook_delivery_body(event: &WebhookEvent) -> String {
    serde_json::json!({
        "contract_version": WEBHOOK_CONTRACT_VERSION,
        "event_id": event.event_id,
        "kind": event.kind,
        "sequence": event.sequence,
        "target_id": event.target_id,
        "revision": event.revision,
        "occurred_at_ms": event.occurred_at.get(),
    })
    .to_string()
}

/// 失敗後の待ち時間。試行回数に応じて指数的に増やし、上限で頭打ちにする。
pub fn webhook_backoff_ms(attempts_so_far: u32) -> i64 {
    let exponent = attempts_so_far.saturating_sub(1).min(30);
    WEBHOOK_BACKOFF_BASE_MS
        .saturating_mul(1_i64 << exponent)
        .min(WEBHOOK_BACKOFF_MAX_MS)
}

/// 1 tickの結果。ログはcomposition rootがこの値から記録する。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WebhookTickOutcome {
    pub delivered: Vec<String>,
    pub failed: Vec<(String, WebhookDeliveryFailure)>,
    pub disabled: Vec<String>,
}

/// 期限が来た配送を1回分処理する。
///
/// 取得(lease)→送信→記録の順で、1件ずつ直列に扱う。取得上限が同時配送数の
/// 上限を兼ねる。停止シグナルとの選択はループを持つ側が行う。
pub async fn webhook_delivery_tick(
    repository: &dyn WebhookDeliveryRepository,
    sender: &dyn WebhookDeliverySender,
    now: UnixMillis,
) -> Result<WebhookTickOutcome, StorageError> {
    let lease_until = UnixMillis::new(now.get().saturating_add(WEBHOOK_LEASE_MS));
    let claimed = repository
        .claim_due_deliveries(now, lease_until, WEBHOOK_DELIVERY_BATCH)
        .await?;
    let mut outcome = WebhookTickOutcome::default();
    for delivery in claimed {
        let body = webhook_delivery_body(&delivery.event);
        match sender
            .deliver(&delivery.url, &delivery.secret, now, &body)
            .await
        {
            Ok(()) => {
                repository
                    .record_delivered(&delivery.subscription_id, delivery.event.sequence, now)
                    .await?;
                outcome.delivered.push(delivery.subscription_id.clone());
            }
            Err(failure) => {
                let attempts = delivery.attempt_count.saturating_add(1);
                let next_attempt_at =
                    UnixMillis::new(now.get().saturating_add(webhook_backoff_ms(attempts)));
                repository
                    .record_failed(
                        &delivery.subscription_id,
                        delivery.event.sequence,
                        failure,
                        attempts,
                        next_attempt_at,
                        now,
                    )
                    .await?;
                outcome
                    .failed
                    .push((delivery.subscription_id.clone(), failure));
                if attempts >= WEBHOOK_MAX_ATTEMPTS {
                    repository
                        .disable_exhausted_subscription(&delivery.subscription_id, now)
                        .await?;
                    outcome.disabled.push(delivery.subscription_id.clone());
                }
            }
        }
    }
    Ok(outcome)
}

/// 購読できるevent種別。契約と設定画面の一覧に使う。
pub const WEBHOOK_EVENT_KINDS: [&str; 7] = [
    "note.created",
    "note.updated",
    "note.deleted",
    "note.restored",
    "bibliography_item.created",
    "bibliography_item.updated",
    "bibliography_item.deleted",
];

/// subscriptionの状態。DBのstate列と同じ語彙を使う。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebhookSubscriptionState {
    PendingChallenge,
    Active,
    Disabled,
}

impl WebhookSubscriptionState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PendingChallenge => "pending_challenge",
            Self::Active => "active",
            Self::Disabled => "disabled",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending_challenge" => Some(Self::PendingChallenge),
            "active" => Some(Self::Active),
            "disabled" => Some(Self::Disabled),
            _ => None,
        }
    }
}

/// 一覧と設定画面に出すsubscriptionの概要。secretは含めない。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebhookSubscriptionOverview {
    pub subscription_id: String,
    pub url: String,
    pub event_kinds: Vec<String>,
    pub state: WebhookSubscriptionState,
    pub disabled_reason: Option<String>,
    pub created_at: UnixMillis,
    pub updated_at: UnixMillis,
    pub revision: i64,
    /// 直近の配送試行。まだ試行がなければNone。
    pub last_attempted_at: Option<UnixMillis>,
    pub last_failure: Option<String>,
    /// 次に試行する予定の時刻。配送待ちがなければNone。
    pub next_attempt_at: Option<UnixMillis>,
    /// 配送待ちevent数。
    pub pending_count: i64,
}

/// 検査済みの送信先。adapterはこの結果のhostとportで接続する。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebhookDestination {
    pub host: String,
    pub port: u16,
    /// 管理者allowlistに含まれ、address検査とHTTPS限定を免除するか。
    pub exempt: bool,
    /// hostがIPのliteralの場合、そのaddress。
    pub literal: Option<std::net::IpAddr>,
}

/// 送信先URLが検査を通らなかったことを示す。理由は応答へ出さない。
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("webhook destination URL is not allowed")]
pub struct InvalidWebhookDestination;

/// 送信先URLの検査。public HTTPS(port 443)以外とuserinfo付きURLを拒否し、
/// IPのliteralは特別用途addressを拒否する。allowlistのhostだけを例外にする。
/// 名前で指定された宛先の解決結果の検査は、送信直前にadapterが行う。
pub fn validate_webhook_destination(
    url: &str,
    allowed_hosts: &[String],
) -> Result<WebhookDestination, InvalidWebhookDestination> {
    let parsed = url::Url::parse(url).map_err(|_| InvalidWebhookDestination)?;
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(InvalidWebhookDestination);
    }
    let Some(host) = parsed.host_str().map(str::to_string) else {
        return Err(InvalidWebhookDestination);
    };
    let exempt = allowed_hosts
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(&host));
    if !exempt {
        if parsed.scheme() != "https" {
            return Err(InvalidWebhookDestination);
        }
        if parsed.port().is_some_and(|port| port != 443) {
            return Err(InvalidWebhookDestination);
        }
    }
    let port = parsed
        .port_or_known_default()
        .ok_or(InvalidWebhookDestination)?;
    let literal = match parsed.host() {
        Some(url::Host::Ipv4(v4)) => Some(std::net::IpAddr::V4(v4)),
        Some(url::Host::Ipv6(v6)) => Some(std::net::IpAddr::V6(v6)),
        _ => None,
    };
    if !exempt && literal.is_some_and(|address| !is_public_webhook_address(address)) {
        return Err(InvalidWebhookDestination);
    }
    Ok(WebhookDestination {
        host,
        port,
        exempt,
        literal,
    })
}

/// 公開networkのaddressだけを許す。特別用途address(RFC 6890)を拒否する。
pub fn is_public_webhook_address(address: std::net::IpAddr) -> bool {
    match address {
        std::net::IpAddr::V4(v4) => {
            let octets = v4.octets();
            !(v4.is_unspecified()
                || v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_multicast()
                // 100.64.0.0/10 (shared address space)
                || (octets[0] == 100 && (octets[1] & 0b1100_0000) == 64)
                // 192.0.0.0/24 (IETF protocol assignments)
                || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
                // 198.18.0.0/15 (benchmarking)
                || (octets[0] == 198 && (octets[1] & 0b1111_1110) == 18)
                // 240.0.0.0/4 (reserved)
                || octets[0] >= 240)
        }
        std::net::IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_public_webhook_address(std::net::IpAddr::V4(mapped));
            }
            let segments = v6.segments();
            !(v6.is_unspecified()
                || v6.is_loopback()
                || v6.is_multicast()
                // fc00::/7 (unique local)
                || (segments[0] & 0xfe00) == 0xfc00
                // fe80::/10 (link local)
                || (segments[0] & 0xffc0) == 0xfe80
                // 2001:db8::/32 (documentation)
                || (segments[0] == 0x2001 && segments[1] == 0x0db8))
        }
    }
}

/// subscription管理の永続化。実装はSQLite adapterが持つ。
#[async_trait]
pub trait WebhookSubscriptionRepository: Send + Sync {
    async fn list_owned_subscriptions(
        &self,
        actor: &Actor,
    ) -> Result<Vec<WebhookSubscriptionOverview>, StorageError>;
    async fn create_owned_subscription(
        &self,
        actor: &Actor,
        subscription_id: &str,
        url: &str,
        event_kinds: &[String],
        secret: &str,
        now: UnixMillis,
    ) -> Result<(), StorageError>;
    /// 検証に使うため、所有するsubscriptionのURLとsecretを読む。
    async fn owned_subscription_credentials(
        &self,
        actor: &Actor,
        subscription_id: &str,
    ) -> Result<Option<(String, String)>, StorageError>;
    async fn activate_owned_subscription(
        &self,
        actor: &Actor,
        subscription_id: &str,
        now: UnixMillis,
    ) -> Result<bool, StorageError>;
    async fn delete_owned_subscription(
        &self,
        actor: &Actor,
        subscription_id: &str,
    ) -> Result<bool, StorageError>;
    async fn replace_owned_secret(
        &self,
        actor: &Actor,
        subscription_id: &str,
        secret: &str,
        now: UnixMillis,
    ) -> Result<bool, StorageError>;
    /// 失敗中の先頭の配送を即時再試行へ戻す。無効化済みなら有効へ戻す。
    async fn retry_owned_head_delivery(
        &self,
        actor: &Actor,
        subscription_id: &str,
        now: UnixMillis,
    ) -> Result<bool, StorageError>;
    /// 失敗中の先頭の配送を破棄し、後続を進められるようにする。
    async fn discard_owned_head_delivery(
        &self,
        actor: &Actor,
        subscription_id: &str,
        now: UnixMillis,
    ) -> Result<bool, StorageError>;
}

/// subscription管理の失敗。webはこれをHTTPの失敗表現へ写す。
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum WebhookUseCaseError {
    #[error("webhook subscription was not found")]
    NotFound,
    #[error("webhook destination URL is not allowed")]
    InvalidDestination,
    #[error("webhook event kinds are empty or unknown")]
    InvalidEventKinds,
    #[error("storage failed: {0}")]
    Storage(#[from] StorageError),
}

/// 所有確認の結果。失敗は分類ごと画面へ返す。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebhookVerificationOutcome {
    Activated,
    Failed(WebhookDeliveryFailure),
}

/// subscription管理のユースケース。webはこのtraitだけへ依存する。
#[async_trait]
pub trait WebhookUseCases: Send + Sync {
    async fn list_subscriptions(
        &self,
        actor: &Actor,
    ) -> Result<Vec<WebhookSubscriptionOverview>, WebhookUseCaseError>;
    /// 登録する。返り値は(概要, secretの平文)。secretはこの応答でだけ返す。
    async fn create_subscription(
        &self,
        actor: &Actor,
        url: &str,
        event_kinds: Vec<String>,
    ) -> Result<(WebhookSubscriptionOverview, String), WebhookUseCaseError>;
    /// 署名付きchallengeを送信先へ送り、応答を確認して有効化する。
    async fn verify_subscription(
        &self,
        actor: &Actor,
        subscription_id: &str,
    ) -> Result<WebhookVerificationOutcome, WebhookUseCaseError>;
    /// secretを再生成し、新しい平文を返す。
    async fn regenerate_secret(
        &self,
        actor: &Actor,
        subscription_id: &str,
    ) -> Result<String, WebhookUseCaseError>;
    async fn delete_subscription(
        &self,
        actor: &Actor,
        subscription_id: &str,
    ) -> Result<(), WebhookUseCaseError>;
    async fn retry_delivery(
        &self,
        actor: &Actor,
        subscription_id: &str,
    ) -> Result<(), WebhookUseCaseError>;
    async fn discard_delivery(
        &self,
        actor: &Actor,
        subscription_id: &str,
    ) -> Result<(), WebhookUseCaseError>;
}

/// subscription管理の実装。repositoryとsenderを組み合わせる。
pub struct WebhookSubscriptionApplication {
    repository: std::sync::Arc<dyn WebhookSubscriptionRepository>,
    sender: std::sync::Arc<dyn WebhookDeliverySender>,
    clock: std::sync::Arc<dyn Clock>,
    random: std::sync::Arc<dyn Random>,
    allowed_hosts: Vec<String>,
}

impl WebhookSubscriptionApplication {
    pub fn new(
        repository: std::sync::Arc<dyn WebhookSubscriptionRepository>,
        sender: std::sync::Arc<dyn WebhookDeliverySender>,
        clock: std::sync::Arc<dyn Clock>,
        random: std::sync::Arc<dyn Random>,
        allowed_hosts: Vec<String>,
    ) -> Self {
        Self {
            repository,
            sender,
            clock,
            random,
            allowed_hosts,
        }
    }

    fn validated_event_kinds(event_kinds: Vec<String>) -> Result<Vec<String>, WebhookUseCaseError> {
        if event_kinds.is_empty()
            || event_kinds
                .iter()
                .any(|kind| !WEBHOOK_EVENT_KINDS.contains(&kind.as_str()))
        {
            return Err(WebhookUseCaseError::InvalidEventKinds);
        }
        let mut kinds = event_kinds;
        kinds.sort_unstable();
        kinds.dedup();
        Ok(kinds)
    }
}

#[async_trait]
impl WebhookUseCases for WebhookSubscriptionApplication {
    async fn list_subscriptions(
        &self,
        actor: &Actor,
    ) -> Result<Vec<WebhookSubscriptionOverview>, WebhookUseCaseError> {
        Ok(self.repository.list_owned_subscriptions(actor).await?)
    }

    async fn create_subscription(
        &self,
        actor: &Actor,
        url: &str,
        event_kinds: Vec<String>,
    ) -> Result<(WebhookSubscriptionOverview, String), WebhookUseCaseError> {
        validate_webhook_destination(url, &self.allowed_hosts)
            .map_err(|InvalidWebhookDestination| WebhookUseCaseError::InvalidDestination)?;
        let kinds = Self::validated_event_kinds(event_kinds)?;
        let subscription_id = self.random.uuid_v7().to_string();
        let secret = self.random.opaque_token();
        let now = self.clock.now();
        self.repository
            .create_owned_subscription(actor, &subscription_id, url, &kinds, &secret, now)
            .await?;
        let overview = self
            .repository
            .list_owned_subscriptions(actor)
            .await?
            .into_iter()
            .find(|subscription| subscription.subscription_id == subscription_id)
            .ok_or(WebhookUseCaseError::NotFound)?;
        Ok((overview, secret))
    }

    async fn verify_subscription(
        &self,
        actor: &Actor,
        subscription_id: &str,
    ) -> Result<WebhookVerificationOutcome, WebhookUseCaseError> {
        let Some((url, secret)) = self
            .repository
            .owned_subscription_credentials(actor, subscription_id)
            .await?
        else {
            return Err(WebhookUseCaseError::NotFound);
        };
        let challenge = self.random.opaque_token();
        let now = self.clock.now();
        match self
            .sender
            .verify_destination(&url, &secret, now, &challenge)
            .await
        {
            Ok(()) => {
                if !self
                    .repository
                    .activate_owned_subscription(actor, subscription_id, now)
                    .await?
                {
                    return Err(WebhookUseCaseError::NotFound);
                }
                Ok(WebhookVerificationOutcome::Activated)
            }
            Err(failure) => Ok(WebhookVerificationOutcome::Failed(failure)),
        }
    }

    async fn regenerate_secret(
        &self,
        actor: &Actor,
        subscription_id: &str,
    ) -> Result<String, WebhookUseCaseError> {
        let secret = self.random.opaque_token();
        let now = self.clock.now();
        if !self
            .repository
            .replace_owned_secret(actor, subscription_id, &secret, now)
            .await?
        {
            return Err(WebhookUseCaseError::NotFound);
        }
        Ok(secret)
    }

    async fn delete_subscription(
        &self,
        actor: &Actor,
        subscription_id: &str,
    ) -> Result<(), WebhookUseCaseError> {
        if !self
            .repository
            .delete_owned_subscription(actor, subscription_id)
            .await?
        {
            return Err(WebhookUseCaseError::NotFound);
        }
        Ok(())
    }

    async fn retry_delivery(
        &self,
        actor: &Actor,
        subscription_id: &str,
    ) -> Result<(), WebhookUseCaseError> {
        let now = self.clock.now();
        if !self
            .repository
            .retry_owned_head_delivery(actor, subscription_id, now)
            .await?
        {
            return Err(WebhookUseCaseError::NotFound);
        }
        Ok(())
    }

    async fn discard_delivery(
        &self,
        actor: &Actor,
        subscription_id: &str,
    ) -> Result<(), WebhookUseCaseError> {
        let now = self.clock.now();
        if !self
            .repository
            .discard_owned_head_delivery(actor, subscription_id, now)
            .await?
        {
            return Err(WebhookUseCaseError::NotFound);
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    fn delivery(attempts: u32) -> WebhookPendingDelivery {
        WebhookPendingDelivery {
            subscription_id: "sub-a".into(),
            url: "https://receiver.example.test/hook".into(),
            secret: "secret".into(),
            attempt_count: attempts,
            event: WebhookEvent {
                event_id: "event-1".into(),
                kind: "note.created".into(),
                sequence: 7,
                target_id: "note-1".into(),
                revision: 3,
                occurred_at: UnixMillis::new(1_000),
            },
        }
    }

    #[derive(Default)]
    struct StubRepository {
        due: Mutex<Vec<WebhookPendingDelivery>>,
        delivered: Mutex<Vec<i64>>,
        failed: Mutex<Vec<(u32, i64)>>,
        disabled: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl WebhookDeliveryRepository for StubRepository {
        async fn claim_due_deliveries(
            &self,
            _now: UnixMillis,
            _lease_until: UnixMillis,
            _limit: u32,
        ) -> Result<Vec<WebhookPendingDelivery>, StorageError> {
            Ok(self.due.lock().expect("due lock").drain(..).collect())
        }
        async fn record_delivered(
            &self,
            _subscription_id: &str,
            event_sequence: i64,
            _delivered_at: UnixMillis,
        ) -> Result<(), StorageError> {
            self.delivered
                .lock()
                .expect("delivered lock")
                .push(event_sequence);
            Ok(())
        }
        async fn record_failed(
            &self,
            _subscription_id: &str,
            _event_sequence: i64,
            _failure: WebhookDeliveryFailure,
            attempt_count: u32,
            next_attempt_at: UnixMillis,
            _attempted_at: UnixMillis,
        ) -> Result<(), StorageError> {
            self.failed
                .lock()
                .expect("failed lock")
                .push((attempt_count, next_attempt_at.get()));
            Ok(())
        }
        async fn disable_exhausted_subscription(
            &self,
            subscription_id: &str,
            _disabled_at: UnixMillis,
        ) -> Result<(), StorageError> {
            self.disabled
                .lock()
                .expect("disabled lock")
                .push(subscription_id.into());
            Ok(())
        }
        async fn purge_expired_deliveries(&self, _now: UnixMillis) -> Result<u64, StorageError> {
            Ok(0)
        }
    }

    struct StubSender {
        result: Result<(), WebhookDeliveryFailure>,
        bodies: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl WebhookDeliverySender for StubSender {
        async fn deliver(
            &self,
            _url: &str,
            _secret: &str,
            _sent_at: UnixMillis,
            body: &str,
        ) -> Result<(), WebhookDeliveryFailure> {
            self.bodies.lock().expect("body lock").push(body.into());
            self.result
        }
        async fn verify_destination(
            &self,
            _url: &str,
            _secret: &str,
            _sent_at: UnixMillis,
            _challenge: &str,
        ) -> Result<(), WebhookDeliveryFailure> {
            self.result
        }
    }

    /// 成功した配送は記録され、本文は契約の項目だけを含む。
    #[tokio::test]
    async fn tick_records_success_and_sends_the_contract_payload() {
        let repository = StubRepository::default();
        repository.due.lock().expect("due lock").push(delivery(0));
        let sender = StubSender {
            result: Ok(()),
            bodies: Mutex::new(Vec::new()),
        };
        let outcome = webhook_delivery_tick(&repository, &sender, UnixMillis::new(10_000))
            .await
            .expect("tick");
        assert_eq!(outcome.delivered, vec!["sub-a".to_string()]);
        assert_eq!(
            *repository.delivered.lock().expect("delivered lock"),
            vec![7]
        );
        let bodies = sender.bodies.lock().expect("body lock");
        let payload: serde_json::Value = serde_json::from_str(&bodies[0]).expect("payload is JSON");
        assert_eq!(
            payload,
            serde_json::json!({
                "contract_version": 1,
                "event_id": "event-1",
                "kind": "note.created",
                "sequence": 7,
                "target_id": "note-1",
                "revision": 3,
                "occurred_at_ms": 1_000,
            })
        );
    }

    /// 失敗はbackoff付きで再試行を予約し、上限に達したら無効化する。
    #[tokio::test]
    async fn tick_schedules_backoff_and_disables_after_the_final_attempt() {
        let repository = StubRepository::default();
        repository.due.lock().expect("due lock").push(delivery(0));
        let sender = StubSender {
            result: Err(WebhookDeliveryFailure::TimedOut),
            bodies: Mutex::new(Vec::new()),
        };
        let outcome = webhook_delivery_tick(&repository, &sender, UnixMillis::new(10_000))
            .await
            .expect("tick");
        assert_eq!(
            outcome.failed,
            vec![("sub-a".to_string(), WebhookDeliveryFailure::TimedOut)]
        );
        assert!(outcome.disabled.is_empty());
        // 1回目の失敗は5秒後に再試行する。
        assert_eq!(
            *repository.failed.lock().expect("failed lock"),
            vec![(1, 15_000)]
        );

        // 最後の試行の失敗で無効化する。
        repository
            .due
            .lock()
            .expect("due lock")
            .push(delivery(WEBHOOK_MAX_ATTEMPTS - 1));
        let outcome = webhook_delivery_tick(&repository, &sender, UnixMillis::new(20_000))
            .await
            .expect("final tick");
        assert_eq!(outcome.disabled, vec!["sub-a".to_string()]);
        assert_eq!(
            *repository.disabled.lock().expect("disabled lock"),
            vec!["sub-a".to_string()]
        );
    }

    /// backoffは指数的に増え、上限で頭打ちになる。
    #[test]
    fn backoff_grows_exponentially_and_caps() {
        assert_eq!(webhook_backoff_ms(1), 5_000);
        assert_eq!(webhook_backoff_ms(2), 10_000);
        assert_eq!(webhook_backoff_ms(5), 80_000);
        assert_eq!(webhook_backoff_ms(10), 2_560_000);
        assert_eq!(webhook_backoff_ms(11), WEBHOOK_BACKOFF_MAX_MS);
        assert_eq!(webhook_backoff_ms(30), WEBHOOK_BACKOFF_MAX_MS);
    }

    /// 送信先URLの検査は、公開HTTPS以外とuserinfo付きURLを拒否する。
    #[test]
    fn destination_validation_permits_only_public_https() {
        let none: &[String] = &[];
        assert!(validate_webhook_destination("https://receiver.example.test/hook", none).is_ok());
        assert!(validate_webhook_destination("http://receiver.example.test/hook", none).is_err());
        assert!(validate_webhook_destination("https://receiver.example.test:8443/", none).is_err());
        assert!(
            validate_webhook_destination("https://user:pass@receiver.example.test/", none).is_err()
        );
        assert!(validate_webhook_destination("https://10.0.0.8/hook", none).is_err());
        assert!(validate_webhook_destination("https://[::1]/hook", none).is_err());
        assert!(validate_webhook_destination("https://203.0.113.9/hook", none).is_err());
        assert!(validate_webhook_destination("https://93.184.216.34/hook", none).is_ok());
    }

    /// allowlistのhostはhttpとprivate addressも許し、検査の免除が記録される。
    #[test]
    fn destination_validation_exempts_allowlisted_hosts() {
        let allowed = vec!["receiver.internal".to_string(), "127.0.0.1".to_string()];
        let destination =
            validate_webhook_destination("http://receiver.internal:8080/hook", &allowed)
                .expect("allowlisted host");
        assert!(destination.exempt);
        assert_eq!(destination.port, 8080);
        assert!(validate_webhook_destination("http://127.0.0.1:3000/hook", &allowed).is_ok());
        assert!(validate_webhook_destination("http://127.0.0.2:3000/hook", &allowed).is_err());
    }

    struct FixedClock;

    impl Clock for FixedClock {
        fn now(&self) -> UnixMillis {
            UnixMillis::new(1_700_000_000_000)
        }
    }

    struct FixedRandom;

    impl Random for FixedRandom {
        fn uuid_v7(&self) -> marginalis_domain::EntityId {
            use std::str::FromStr;
            marginalis_domain::EntityId::from_str("01890f3c-6a4d-7cc2-98b3-84b68f68c6e1")
                .expect("fixed UUIDv7")
        }

        fn opaque_token(&self) -> String {
            "opaque-token-1".into()
        }
    }

    fn actor() -> Actor {
        Actor::for_single_identity(
            marginalis_domain::PrincipalId::new(1).expect("ID"),
            marginalis_domain::Identity::new("https://idp.example.test/".into(), "owner-1".into())
                .expect("identity"),
        )
    }

    fn overview(
        subscription_id: &str,
        state: WebhookSubscriptionState,
    ) -> WebhookSubscriptionOverview {
        WebhookSubscriptionOverview {
            subscription_id: subscription_id.into(),
            url: "https://receiver.example.test/hook".into(),
            event_kinds: vec!["note.created".into()],
            state,
            disabled_reason: None,
            created_at: UnixMillis::new(1_000),
            updated_at: UnixMillis::new(1_000),
            revision: 0,
            last_attempted_at: None,
            last_failure: None,
            next_attempt_at: None,
            pending_count: 0,
        }
    }

    struct CreatedSubscription {
        subscription_id: String,
        event_kinds: Vec<String>,
    }

    #[derive(Default)]
    struct StubSubscriptions {
        created: Mutex<Vec<CreatedSubscription>>,
        activated: Mutex<Vec<String>>,
        credentials: Mutex<Option<(String, String)>>,
    }

    #[async_trait]
    impl WebhookSubscriptionRepository for StubSubscriptions {
        async fn list_owned_subscriptions(
            &self,
            _actor: &Actor,
        ) -> Result<Vec<WebhookSubscriptionOverview>, StorageError> {
            Ok(self
                .created
                .lock()
                .expect("created lock")
                .iter()
                .map(|created| {
                    overview(
                        &created.subscription_id,
                        WebhookSubscriptionState::PendingChallenge,
                    )
                })
                .collect())
        }
        async fn create_owned_subscription(
            &self,
            _actor: &Actor,
            subscription_id: &str,
            url: &str,
            event_kinds: &[String],
            secret: &str,
            _now: UnixMillis,
        ) -> Result<(), StorageError> {
            let _ = (url, secret);
            self.created
                .lock()
                .expect("created lock")
                .push(CreatedSubscription {
                    subscription_id: subscription_id.into(),
                    event_kinds: event_kinds.to_vec(),
                });
            Ok(())
        }
        async fn owned_subscription_credentials(
            &self,
            _actor: &Actor,
            _subscription_id: &str,
        ) -> Result<Option<(String, String)>, StorageError> {
            Ok(self.credentials.lock().expect("credentials lock").clone())
        }
        async fn activate_owned_subscription(
            &self,
            _actor: &Actor,
            subscription_id: &str,
            _now: UnixMillis,
        ) -> Result<bool, StorageError> {
            self.activated
                .lock()
                .expect("activated lock")
                .push(subscription_id.into());
            Ok(true)
        }
        async fn delete_owned_subscription(
            &self,
            _actor: &Actor,
            _subscription_id: &str,
        ) -> Result<bool, StorageError> {
            Ok(false)
        }
        async fn replace_owned_secret(
            &self,
            _actor: &Actor,
            _subscription_id: &str,
            _secret: &str,
            _now: UnixMillis,
        ) -> Result<bool, StorageError> {
            Ok(true)
        }
        async fn retry_owned_head_delivery(
            &self,
            _actor: &Actor,
            _subscription_id: &str,
            _now: UnixMillis,
        ) -> Result<bool, StorageError> {
            Ok(true)
        }
        async fn discard_owned_head_delivery(
            &self,
            _actor: &Actor,
            _subscription_id: &str,
            _now: UnixMillis,
        ) -> Result<bool, StorageError> {
            Ok(true)
        }
    }

    fn subscription_application(
        repository: std::sync::Arc<StubSubscriptions>,
        result: Result<(), WebhookDeliveryFailure>,
    ) -> WebhookSubscriptionApplication {
        WebhookSubscriptionApplication::new(
            repository,
            std::sync::Arc::new(StubSender {
                result,
                bodies: Mutex::new(Vec::new()),
            }),
            std::sync::Arc::new(FixedClock),
            std::sync::Arc::new(FixedRandom),
            Vec::new(),
        )
    }

    /// 登録は送信先とevent種別を検査し、secretをこの応答でだけ返す。
    #[tokio::test]
    async fn create_subscription_validates_input_and_returns_the_secret_once() {
        let repository = std::sync::Arc::new(StubSubscriptions::default());
        let application = subscription_application(repository.clone(), Ok(()));
        let (subscription, secret) = application
            .create_subscription(
                &actor(),
                "https://receiver.example.test/hook",
                vec!["note.created".into(), "note.created".into()],
            )
            .await
            .expect("create");
        assert_eq!(secret, "opaque-token-1");
        assert_eq!(
            subscription.state,
            WebhookSubscriptionState::PendingChallenge
        );
        {
            let created = repository.created.lock().expect("created lock");
            // 重複したevent種別は1つへ畳む。
            assert_eq!(created[0].event_kinds, vec!["note.created".to_string()]);
        }

        let error = application
            .create_subscription(
                &actor(),
                "http://receiver.example.test/",
                vec!["note.created".into()],
            )
            .await
            .expect_err("insecure URL");
        assert_eq!(error, WebhookUseCaseError::InvalidDestination);
        let error = application
            .create_subscription(
                &actor(),
                "https://receiver.example.test/",
                vec!["unknown.kind".into()],
            )
            .await
            .expect_err("unknown kind");
        assert_eq!(error, WebhookUseCaseError::InvalidEventKinds);
    }

    /// 所有確認は成功した場合だけ購読を有効化し、失敗は分類を返す。
    #[tokio::test]
    async fn verification_activates_only_after_the_challenge_succeeds() {
        let repository = std::sync::Arc::new(StubSubscriptions::default());
        *repository.credentials.lock().expect("credentials lock") = Some((
            "https://receiver.example.test/hook".into(),
            "secret-1".into(),
        ));
        let application = subscription_application(repository.clone(), Ok(()));
        let outcome = application
            .verify_subscription(&actor(), "sub-a")
            .await
            .expect("verify");
        assert_eq!(outcome, WebhookVerificationOutcome::Activated);
        assert_eq!(
            *repository.activated.lock().expect("activated lock"),
            vec!["sub-a".to_string()]
        );

        let repository = std::sync::Arc::new(StubSubscriptions::default());
        *repository.credentials.lock().expect("credentials lock") = Some((
            "https://receiver.example.test/hook".into(),
            "secret-1".into(),
        ));
        let application = subscription_application(
            repository.clone(),
            Err(WebhookDeliveryFailure::NonSuccessStatus),
        );
        let outcome = application
            .verify_subscription(&actor(), "sub-a")
            .await
            .expect("verify");
        assert_eq!(
            outcome,
            WebhookVerificationOutcome::Failed(WebhookDeliveryFailure::NonSuccessStatus)
        );
        assert!(
            repository
                .activated
                .lock()
                .expect("activated lock")
                .is_empty()
        );

        let repository = std::sync::Arc::new(StubSubscriptions::default());
        let application = subscription_application(repository, Ok(()));
        let error = application
            .verify_subscription(&actor(), "missing")
            .await
            .expect_err("unknown subscription");
        assert_eq!(error, WebhookUseCaseError::NotFound);
    }
}
