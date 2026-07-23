//! HTTP、SQLite、ファイルシステムから独立したユースケースとport。

use marginalis_domain::{
    Actor, EntityId, McpAuthorizationGrant, McpClientAuthorization, McpOAuthClient, NoteId,
    NotePage, NotePermission, NoteProjection, NoteSource, OidcIdentity, OidcLoginResult, OidcUser,
    RegistrationPolicy, SourceRevision, UnixMillis, UserId,
};
use std::future::Future;

use async_trait::async_trait;

/// 時刻取得を外部化し、期限・journal復旧を決定的に試験できるようにする。
pub trait Clock: Send + Sync {
    fn now(&self) -> UnixMillis;
}

/// UUIDv7と秘密トークンの生成を外部化する。
///
/// 実装は暗号学的に安全な乱数を使う。テスト実装は決定的な値を供給できる。
pub trait Random: Send + Sync {
    fn uuid_v7(&self) -> EntityId;
    fn opaque_token(&self) -> String;
}

/// OIDC identityと内部ユーザーの原子的な対応付けを担うport。
pub trait OidcIdentityStore: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn register_or_lookup(
        &self,
        identity: OidcIdentity,
        policy: RegistrationPolicy,
        new_user_id: UserId,
        now: UnixMillis,
    ) -> impl Future<Output = Result<OidcLoginResult, Self::Error>> + Send;
}

/// 緊急用root accountの初期化を扱うport。平文passwordは保持しない。
pub trait RootCredentialStore: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;
    fn is_initialized(&self) -> impl Future<Output = Result<bool, Self::Error>> + Send;
    fn initialize_if_missing(
        &self,
        password: String,
        user_id: UserId,
        now: UnixMillis,
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send;
    fn verify_password(
        &self,
        password: String,
    ) -> impl Future<Output = Result<Option<UserId>, Self::Error>> + Send;
}

/// rootが管理するOIDCユーザー状態の一覧および遷移を扱うport。
pub trait OidcUserAdministrationStore: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn list_pending(&self) -> impl Future<Output = Result<Vec<OidcUser>, Self::Error>> + Send;
    fn activate(
        &self,
        user_id: UserId,
        now: UnixMillis,
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send;
}

pub struct RootInitializationService<'a, Store, Entropy, Time> {
    store: &'a Store,
    entropy: &'a Entropy,
    clock: &'a Time,
}

impl<'a, Store, Entropy, Time> RootInitializationService<'a, Store, Entropy, Time>
where
    Store: RootCredentialStore,
    Entropy: Random,
    Time: Clock,
{
    pub const fn new(store: &'a Store, entropy: &'a Entropy, clock: &'a Time) -> Self {
        Self {
            store,
            entropy,
            clock,
        }
    }
    pub async fn initialize_if_missing(&self, password: String) -> Result<bool, Store::Error> {
        self.store
            .initialize_if_missing(
                password,
                UserId::new(self.entropy.uuid_v7()),
                self.clock.now(),
            )
            .await
    }
}

/// OIDC認可リクエストに一度だけ対応するstate、nonceおよびPKCE verifier。
///
/// 値はDB adapterでは平文保存してはならない。applicationではcallbackとの対応だけを表す。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OidcLoginAttempt {
    pub state: String,
    pub nonce: String,
    pub pkce_verifier: String,
    pub expires_at: UnixMillis,
}

pub trait OidcLoginAttemptStore: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn issue(
        &self,
        attempt: OidcLoginAttempt,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
    fn consume(
        &self,
        state: String,
        now: UnixMillis,
    ) -> impl Future<Output = Result<Option<OidcLoginAttempt>, Self::Error>> + Send;
}

/// HTTP Cookieに入れる不透明なsession IDと、同一セッションのCSRF token。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebSession {
    pub session_id: String,
    pub csrf_token: String,
    pub actor: Actor,
    pub idle_expires_at: UnixMillis,
    pub absolute_expires_at: UnixMillis,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthenticatedSession {
    pub actor: Actor,
    pub idle_expires_at: UnixMillis,
    pub absolute_expires_at: UnixMillis,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionLifetime {
    pub idle_timeout_ms: i64,
    pub absolute_timeout_ms: i64,
}

pub trait WebSessionStore: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn issue(
        &self,
        session: WebSession,
        now: UnixMillis,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
    fn lookup(
        &self,
        session_id: String,
        now: UnixMillis,
    ) -> impl Future<Output = Result<Option<AuthenticatedSession>, Self::Error>> + Send;
    fn revoke(
        &self,
        session_id: String,
        now: UnixMillis,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
    fn verify_csrf(
        &self,
        session_id: String,
        csrf_token: String,
        now: UnixMillis,
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send;
}

/// Cookie session、外部OIDCおよびroot管理をtransportから隔離する境界。
///
/// Web HTTPとMCP OAuthは異なるcredentialを用いるが、いずれもここで認証済み主体へ変換する。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthenticationUseCaseError {
    Rejected,
    NotFound,
    Unavailable,
}

#[async_trait]
pub trait WebAuthenticationUseCases: Send + Sync {
    async fn begin_oidc_login(&self) -> Result<String, AuthenticationUseCaseError>;
    async fn complete_oidc_login(
        &self,
        code: String,
        state: String,
    ) -> Result<OidcLoginResult, AuthenticationUseCaseError>;
    async fn authenticate_session(
        &self,
        session_id: String,
    ) -> Result<Option<AuthenticatedSession>, AuthenticationUseCaseError>;
    async fn verify_csrf(
        &self,
        session_id: String,
        csrf_token: String,
    ) -> Result<bool, AuthenticationUseCaseError>;
    async fn issue_oidc_session(
        &self,
        user_id: UserId,
    ) -> Result<WebSession, AuthenticationUseCaseError>;
    async fn root_login(
        &self,
        password: String,
    ) -> Result<Option<WebSession>, AuthenticationUseCaseError>;
    async fn revoke_session(&self, session_id: String) -> Result<(), AuthenticationUseCaseError>;
    async fn list_pending_users(&self) -> Result<Vec<OidcUser>, AuthenticationUseCaseError>;
    async fn activate_pending_user(
        &self,
        user_id: UserId,
    ) -> Result<bool, AuthenticationUseCaseError>;
    fn cookie_path(&self) -> &str;
}

/// MCP access tokenを検証済みの一般Actorへ変換する永続化境界。
///
/// token自体はこの境界を越えて保存しない。adapterはhash、resource audience、scope、期限および
/// ユーザー状態を同じ照会で検証する。
pub trait McpAccessTokenStore: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn authenticate(
        &self,
        token: String,
        resource_uri: String,
        required_scope: String,
        now: UnixMillis,
    ) -> impl Future<Output = Result<Option<Actor>, Self::Error>> + Send;
}

/// 二段階削除の確認tokenを短期かつ一回限りで保持する境界。
pub trait DeleteConfirmationStore: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn issue(
        &self,
        token: String,
        actor: Actor,
        note_id: NoteId,
        expected_revision: SourceRevision,
        expires_at: UnixMillis,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn consume(
        &self,
        token: String,
        actor: Actor,
        now: UnixMillis,
    ) -> impl Future<Output = Result<Option<(NoteId, SourceRevision)>, Self::Error>> + Send;
}

/// MCP OAuthのclient、single-use authorization codeおよびtoken familyを扱う境界。
///
/// client metadata取得・同意画面・HTTP token endpointはtransport adapterの責務とし、このportは
/// 検証済みの値だけを永続化する。
pub trait McpOAuthStore: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn upsert_client(
        &self,
        client: McpOAuthClient,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn lookup_client(
        &self,
        client_id: String,
    ) -> impl Future<Output = Result<Option<McpOAuthClient>, Self::Error>> + Send;

    fn issue_authorization_code(
        &self,
        code: String,
        grant: McpAuthorizationGrant,
        code_challenge: String,
        expires_at: UnixMillis,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn consume_authorization_code(
        &self,
        code: String,
        client_id: String,
        redirect_uri: String,
        resource_uri: String,
        now: UnixMillis,
    ) -> impl Future<Output = Result<Option<(McpAuthorizationGrant, String)>, Self::Error>> + Send;

    fn issue_token_pair(
        &self,
        access_token: String,
        refresh_token: String,
        grant: McpAuthorizationGrant,
        access_expires_at: UnixMillis,
        refresh_expires_at: UnixMillis,
        issued_at: UnixMillis,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn revoke_client_tokens(
        &self,
        user_id: UserId,
        client_id: String,
        now: UnixMillis,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn list_client_authorizations(
        &self,
        user_id: UserId,
    ) -> impl Future<Output = Result<Vec<McpClientAuthorization>, Self::Error>> + Send;

    /// refresh tokenを一度だけ消費し、新しいtoken pairを同一transactionで保存する。
    fn rotate_refresh_token(
        &self,
        rotation: McpRefreshTokenRotation,
        now: UnixMillis,
    ) -> impl Future<Output = Result<Option<McpAuthorizationGrant>, Self::Error>> + Send;
}

/// OAuth Authorization Code Flowでtransportから渡す検証済み候補。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct McpAuthorizationRequest {
    pub client_id: String,
    pub redirect_uri: String,
    pub resource_uri: String,
    pub scopes: Vec<String>,
    pub code_challenge: String,
}

/// refresh token rotationでadapterへ渡す、すでに生成済みの新旧tokenとbinding。
pub struct McpRefreshTokenRotation {
    pub refresh_token: String,
    pub client_id: String,
    pub resource_uri: String,
    pub new_access_token: String,
    pub new_refresh_token: String,
    pub access_expires_at: UnixMillis,
    pub refresh_expires_at: UnixMillis,
}

/// token endpointだけが短時間保持するtoken pair。Debugを実装しない。
pub struct McpTokenPair {
    pub access_token: String,
    pub refresh_token: String,
    pub access_expires_in_seconds: u64,
    pub scope: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum McpOAuthUseCaseError {
    Rejected,
    Unavailable,
}

#[async_trait]
pub trait McpOAuthAdministrationUseCases: Send + Sync {
    async fn register_client(
        &self,
        actor: Actor,
        client: McpOAuthClient,
    ) -> Result<(), McpOAuthUseCaseError>;
    async fn revoke_client_authorization(
        &self,
        actor: Actor,
        user_id: UserId,
        client_id: String,
    ) -> Result<(), McpOAuthUseCaseError>;
    async fn list_client_authorizations(
        &self,
        actor: Actor,
        user_id: UserId,
    ) -> Result<Vec<McpClientAuthorization>, McpOAuthUseCaseError>;
}

#[async_trait]
pub trait McpOAuthUseCases: Send + Sync {
    async fn validate_authorization_request(
        &self,
        request: McpAuthorizationRequest,
    ) -> Result<McpOAuthClient, McpOAuthUseCaseError>;
    async fn authorize(
        &self,
        actor: Actor,
        request: McpAuthorizationRequest,
    ) -> Result<String, McpOAuthUseCaseError>;
    async fn exchange_authorization_code(
        &self,
        code: String,
        client_id: String,
        redirect_uri: String,
        resource_uri: String,
        code_verifier: String,
    ) -> Result<McpTokenPair, McpOAuthUseCaseError>;
    async fn refresh_access_token(
        &self,
        refresh_token: String,
        client_id: String,
        resource_uri: String,
    ) -> Result<McpTokenPair, McpOAuthUseCaseError>;
}

/// sessionの有効期限と秘密値を一箇所で決めるユースケース。
pub struct WebSessionService<'a, Store, Entropy, Time> {
    store: &'a Store,
    entropy: &'a Entropy,
    clock: &'a Time,
}

impl<'a, Store, Entropy, Time> WebSessionService<'a, Store, Entropy, Time>
where
    Store: WebSessionStore,
    Entropy: Random,
    Time: Clock,
{
    pub const fn new(store: &'a Store, entropy: &'a Entropy, clock: &'a Time) -> Self {
        Self {
            store,
            entropy,
            clock,
        }
    }

    pub async fn issue(
        &self,
        actor: Actor,
        lifetime: SessionLifetime,
    ) -> Result<WebSession, Store::Error> {
        let now = self.clock.now();
        let session = WebSession {
            session_id: self.entropy.opaque_token(),
            csrf_token: self.entropy.opaque_token(),
            actor,
            idle_expires_at: UnixMillis::new(now.get() + lifetime.idle_timeout_ms),
            absolute_expires_at: UnixMillis::new(now.get() + lifetime.absolute_timeout_ms),
        };
        self.store.issue(session.clone(), now).await?;
        Ok(session)
    }
}

/// OIDC callback adapterが呼ぶ登録ユースケース。
pub struct OidcRegistrationService<'a, Store, Entropy> {
    store: &'a Store,
    entropy: &'a Entropy,
}

impl<'a, Store, Entropy> OidcRegistrationService<'a, Store, Entropy>
where
    Store: OidcIdentityStore,
    Entropy: Random,
{
    pub const fn new(store: &'a Store, entropy: &'a Entropy) -> Self {
        Self { store, entropy }
    }

    pub fn register_or_lookup(
        &self,
        identity: OidcIdentity,
        policy: RegistrationPolicy,
        now: UnixMillis,
    ) -> impl Future<Output = Result<OidcLoginResult, Store::Error>> + Send + '_ {
        self.store
            .register_or_lookup(identity, policy, UserId::new(self.entropy.uuid_v7()), now)
    }
}

/// 一連のファイル・投影更新を復旧可能にする操作ジャーナルの識別子。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OperationId(pub EntityId);

/// application層が扱う、ファイル正本の更新状態。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationState {
    Prepared,
    SourceApplied,
    Completed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoteOperationKind {
    Create,
    Update,
    Delete,
}

/// SQLiteとファイルをまたぐノート更新の復旧に必要な最小情報。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JournalEntry {
    pub operation_id: OperationId,
    pub note_id: NoteId,
    pub kind: NoteOperationKind,
    pub state: OperationState,
    pub source_revision: Option<SourceRevision>,
    pub projection: Option<NoteProjection>,
    pub created_at: UnixMillis,
    pub updated_at: UnixMillis,
}

/// adapterが実装する操作ジャーナル境界。
pub trait OperationJournal: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn prepare(&self, entry: JournalEntry) -> impl Future<Output = Result<(), Self::Error>> + Send;
    fn mark_source_applied(
        &self,
        operation_id: OperationId,
        updated_at: UnixMillis,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
    fn complete(
        &self,
        operation_id: OperationId,
        updated_at: UnixMillis,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
    fn incomplete(&self) -> impl Future<Output = Result<Vec<JournalEntry>, Self::Error>> + Send;
}

/// AsciiDoc正本を扱うport。HTTP・SQLite・filesystem adapterはこれを実装する。
pub trait NoteSourceStore: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn read(
        &self,
        note_id: NoteId,
    ) -> impl Future<Output = Result<Option<Vec<u8>>, Self::Error>> + Send;

    fn replace(
        &self,
        note_id: NoteId,
        operation: OperationId,
        source: Vec<u8>,
    ) -> impl Future<Output = Result<SourceRevision, Self::Error>> + Send;

    fn delete(
        &self,
        note_id: NoteId,
        operation: OperationId,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

/// SQLiteなどの検索用投影を、正本更新後に置換するport。
pub trait NoteProjectionStore: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn replace_projection(
        &self,
        projection: NoteProjection,
        revision: SourceRevision,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn delete_projection(
        &self,
        note_id: NoteId,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

/// ACL適用後のノート一覧・検索read model。候補数や順位を返す前に権限を適用する。
pub trait NoteQueryStore: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn list_visible(
        &self,
        actor: Actor,
        offset: u64,
        limit: u32,
    ) -> impl Future<Output = Result<NotePage, Self::Error>> + Send;
    fn search_visible(
        &self,
        actor: Actor,
        query: String,
        offset: u64,
        limit: u32,
    ) -> impl Future<Output = Result<NotePage, Self::Error>> + Send;
}

/// ノートACLの永続化境界。HTTPはこのportを介してのみ権限を問い合わせる。
pub trait NoteAclStore: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn permission_for(
        &self,
        actor: Actor,
        note_id: NoteId,
    ) -> impl Future<Output = Result<Option<NotePermission>, Self::Error>> + Send;

    fn set_permission(
        &self,
        note_id: NoteId,
        user_id: UserId,
        permission: Option<NotePermission>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

#[derive(Debug)]
pub enum NoteAclServiceError {
    Forbidden,
    Store(Box<dyn std::error::Error + Send + Sync>),
}

impl std::fmt::Display for NoteAclServiceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Forbidden => formatter.write_str("note administration is not permitted"),
            Self::Store(_) => formatter.write_str("note ACL storage failed"),
        }
    }
}

impl std::error::Error for NoteAclServiceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Forbidden => None,
            Self::Store(error) => Some(error.as_ref()),
        }
    }
}

/// ACL更新の認可を、HTTPやSQLiteから独立して適用するユースケース。
pub struct NoteAclService<'a, Store> {
    store: &'a Store,
}

impl<'a, Store> NoteAclService<'a, Store>
where
    Store: NoteAclStore,
{
    pub const fn new(store: &'a Store) -> Self {
        Self { store }
    }

    pub async fn set_permission(
        &self,
        actor: Actor,
        note_id: NoteId,
        user_id: UserId,
        permission: Option<NotePermission>,
    ) -> Result<(), NoteAclServiceError> {
        let current = self
            .store
            .permission_for(actor, note_id)
            .await
            .map_err(|error| NoteAclServiceError::Store(Box::new(error)))?;
        if !actor.is_root && !matches!(current, Some(NotePermission::Admin)) {
            return Err(NoteAclServiceError::Forbidden);
        }
        self.store
            .set_permission(note_id, user_id, permission)
            .await
            .map_err(|error| NoteAclServiceError::Store(Box::new(error)))
    }
}

/// ファイル正本、SQLite投影、操作journalを一貫して更新するユースケース。
pub struct NoteWriteService<'a, Sources, Projections, Journal, Entropy, Time> {
    sources: &'a Sources,
    projections: &'a Projections,
    journal: &'a Journal,
    entropy: &'a Entropy,
    clock: &'a Time,
}

/// transportが利用するノート操作の境界。HTTP、MCPおよびCLIは具体adapterを参照しない。
#[async_trait]
pub trait NoteUseCases: Send + Sync {
    async fn list_notes(
        &self,
        actor: Actor,
        offset: u64,
        limit: u32,
    ) -> Result<NotePage, NoteUseCaseError>;
    async fn search_notes(
        &self,
        actor: Actor,
        query: String,
        offset: u64,
        limit: u32,
    ) -> Result<NotePage, NoteUseCaseError>;
    async fn read_source(
        &self,
        actor: Actor,
        note_id: NoteId,
    ) -> Result<NoteSource, NoteUseCaseError>;
    async fn create_source(&self, actor: Actor, source: String)
    -> Result<NoteId, NoteUseCaseError>;
    async fn create_note(
        &self,
        actor: Actor,
        draft: NoteDraft,
    ) -> Result<NoteSource, NoteUseCaseError>;
    async fn update_source(
        &self,
        actor: Actor,
        note_id: NoteId,
        source: String,
        expected_revision: SourceRevision,
    ) -> Result<(), NoteUseCaseError>;
    async fn update_note(
        &self,
        actor: Actor,
        note_id: NoteId,
        draft: NoteDraft,
        expected_revision: SourceRevision,
    ) -> Result<NoteSource, NoteUseCaseError>;
    async fn delete_note(
        &self,
        actor: Actor,
        note_id: NoteId,
        expected_revision: SourceRevision,
    ) -> Result<(), NoteUseCaseError>;
    async fn prepare_delete_note(
        &self,
        actor: Actor,
        note_id: NoteId,
        expected_revision: SourceRevision,
    ) -> Result<DeletePreparation, NoteUseCaseError>;
    async fn confirm_delete_note(
        &self,
        actor: Actor,
        confirmation_token: String,
    ) -> Result<(), NoteUseCaseError>;
    async fn set_permission(
        &self,
        actor: Actor,
        note_id: NoteId,
        user_id: UserId,
        permission: Option<NotePermission>,
    ) -> Result<(), NoteUseCaseError>;
}

/// transportがtitle、body、tagsとして受け取る、server生成metadataを含まないノート内容。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteDraft {
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
}

/// confirmation tokenは返却時だけ平文であり、永続化adapterはhashのみを保存する。
pub struct DeletePreparation {
    pub note_id: NoteId,
    pub title: String,
    pub revision: SourceRevision,
    pub confirmation_token: String,
}

/// transportに公開するノート操作の失敗分類。内部adapterの詳細はここから漏らさない。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoteUseCaseError {
    NotFound,
    Forbidden,
    Conflict,
    Validation,
    Unavailable,
}

impl std::fmt::Display for NoteUseCaseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::NotFound => "note is not available",
            Self::Forbidden => "note operation is not permitted",
            Self::Conflict => "note operation conflicts",
            Self::Validation => "note source is invalid",
            Self::Unavailable => "note operation is unavailable",
        })
    }
}

impl std::error::Error for NoteUseCaseError {}

#[derive(Debug)]
pub enum NoteWriteError {
    Journal(Box<dyn std::error::Error + Send + Sync>),
    Source(Box<dyn std::error::Error + Send + Sync>),
    Projection(Box<dyn std::error::Error + Send + Sync>),
}

impl std::fmt::Display for NoteWriteError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Journal(_) => formatter.write_str("note operation journal failed"),
            Self::Source(_) => formatter.write_str("note source update failed"),
            Self::Projection(_) => formatter.write_str("note projection update failed"),
        }
    }
}

impl std::error::Error for NoteWriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Journal(error) | Self::Source(error) | Self::Projection(error) => {
                Some(error.as_ref())
            }
        }
    }
}

impl<'a, Sources, Projections, Journal, Entropy, Time>
    NoteWriteService<'a, Sources, Projections, Journal, Entropy, Time>
where
    Sources: NoteSourceStore,
    Projections: NoteProjectionStore,
    Journal: OperationJournal,
    Entropy: Random,
    Time: Clock,
{
    pub const fn new(
        sources: &'a Sources,
        projections: &'a Projections,
        journal: &'a Journal,
        entropy: &'a Entropy,
        clock: &'a Time,
    ) -> Self {
        Self {
            sources,
            projections,
            journal,
            entropy,
            clock,
        }
    }

    /// sourceは先にfsyncされ、投影失敗時にはjournalを残して起動時復旧の対象にする。
    pub async fn replace(
        &self,
        kind: NoteOperationKind,
        projection: NoteProjection,
        source: Vec<u8>,
    ) -> Result<SourceRevision, NoteWriteError> {
        let operation_id = OperationId(self.entropy.uuid_v7());
        let now = self.clock.now();
        let expected_revision = SourceRevision::from_source(&source);
        self.journal
            .prepare(JournalEntry {
                operation_id,
                note_id: projection.note_id,
                kind,
                state: OperationState::Prepared,
                source_revision: Some(expected_revision),
                projection: Some(projection.clone()),
                created_at: now,
                updated_at: now,
            })
            .await
            .map_err(|error| NoteWriteError::Journal(Box::new(error)))?;
        let revision = self
            .sources
            .replace(projection.note_id, operation_id, source)
            .await
            .map_err(|error| NoteWriteError::Source(Box::new(error)))?;
        self.journal
            .mark_source_applied(operation_id, self.clock.now())
            .await
            .map_err(|error| NoteWriteError::Journal(Box::new(error)))?;
        self.projections
            .replace_projection(projection, revision)
            .await
            .map_err(|error| NoteWriteError::Projection(Box::new(error)))?;
        self.journal
            .complete(operation_id, self.clock.now())
            .await
            .map_err(|error| NoteWriteError::Journal(Box::new(error)))?;
        Ok(revision)
    }

    /// 正本を物理削除してから投影を削除する。投影処理が止まってもjournalにより再実行できる。
    pub async fn delete(&self, note_id: NoteId) -> Result<(), NoteWriteError> {
        let operation_id = OperationId(self.entropy.uuid_v7());
        let now = self.clock.now();
        self.journal
            .prepare(JournalEntry {
                operation_id,
                note_id,
                kind: NoteOperationKind::Delete,
                state: OperationState::Prepared,
                source_revision: None,
                projection: None,
                created_at: now,
                updated_at: now,
            })
            .await
            .map_err(|error| NoteWriteError::Journal(Box::new(error)))?;
        self.sources
            .delete(note_id, operation_id)
            .await
            .map_err(|error| NoteWriteError::Source(Box::new(error)))?;
        self.journal
            .mark_source_applied(operation_id, self.clock.now())
            .await
            .map_err(|error| NoteWriteError::Journal(Box::new(error)))?;
        self.projections
            .delete_projection(note_id)
            .await
            .map_err(|error| NoteWriteError::Projection(Box::new(error)))?;
        self.journal
            .complete(operation_id, self.clock.now())
            .await
            .map_err(|error| NoteWriteError::Journal(Box::new(error)))?;
        Ok(())
    }

    /// sourceを書込み済みで止まった操作だけを再投影する。preparedは正本変更前なので残す。
    pub async fn recover(&self) -> Result<(), NoteWriteError> {
        for entry in self
            .journal
            .incomplete()
            .await
            .map_err(|error| NoteWriteError::Journal(Box::new(error)))?
        {
            if entry.state != OperationState::SourceApplied {
                continue;
            }
            if entry.kind == NoteOperationKind::Delete {
                if self
                    .sources
                    .read(entry.note_id)
                    .await
                    .map_err(|error| NoteWriteError::Source(Box::new(error)))?
                    .is_some()
                {
                    continue;
                }
                self.projections
                    .delete_projection(entry.note_id)
                    .await
                    .map_err(|error| NoteWriteError::Projection(Box::new(error)))?;
                self.journal
                    .complete(entry.operation_id, self.clock.now())
                    .await
                    .map_err(|error| NoteWriteError::Journal(Box::new(error)))?;
                continue;
            }
            let Some(projection) = entry.projection else {
                continue;
            };
            let Some(source) = self
                .sources
                .read(entry.note_id)
                .await
                .map_err(|error| NoteWriteError::Source(Box::new(error)))?
            else {
                continue;
            };
            let revision = SourceRevision::from_source(&source);
            if entry.source_revision != Some(revision) {
                continue;
            }
            self.projections
                .replace_projection(projection, revision)
                .await
                .map_err(|error| NoteWriteError::Projection(Box::new(error)))?;
            self.journal
                .complete(entry.operation_id, self.clock.now())
                .await
                .map_err(|error| NoteWriteError::Journal(Box::new(error)))?;
        }
        Ok(())
    }
}
