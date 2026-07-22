//! SQLite上のアプリ固有投影と参照解決を扱う永続化境界。

use core::fmt;
use std::{collections::BTreeMap, sync::Arc, time::Duration};

use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng as PasswordOsRng},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{RngCore, rngs::OsRng};
use sha2::{Digest, Sha256};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};
use time::{
    Duration as TimeDuration, OffsetDateTime, UtcOffset, format_description::well_known::Rfc3339,
    macros::format_description,
};
use tokio::sync::Mutex;
use url::Url;
use uuid::Uuid;

use adocweave::{
    html::{
        ExternalLinkPresentation, HtmlOutput, MathLanguagePolicy, RenderPolicy,
        ResourceCapabilities, SourceLanguagePolicy, UnknownSourceLanguage,
        UnresolvedReferencePresentation, render_with_inputs,
    },
    inline::MathLanguage,
    reference::{
        ResolutionFailureKind, ResolutionNotice, ResolutionNoticeKind, ResolvedReference,
        ResolverFailure,
    },
    render::RenderInputs,
    source::TextRange,
};
use notebook_adoc::{
    DEFAULT_SOURCE_LANGUAGES, NoteContentError, NoteProfileError, NoteReferenceError,
    extract_note_references, validate_note_content_profile, validate_note_metadata,
};

/// ノート表示に固定するAdocWeaveの汎用描画policy。
///
/// ここで許可するのは標準AsciiDocの表示上の選択だけであり、UUID、ACL、Base URLなどの
/// アプリ固有の判断はこの外側のResolverが担う。
fn note_render_policy() -> RenderPolicy {
    RenderPolicy {
        external_links: ExternalLinkPresentation::NewContext { noreferrer: true },
        source_languages: SourceLanguagePolicy {
            allowed: Some(
                DEFAULT_SOURCE_LANGUAGES
                    .iter()
                    .map(|language| (*language).to_owned())
                    .collect(),
            ),
            unknown: UnknownSourceLanguage::Diagnostic,
        },
        math_languages: MathLanguagePolicy {
            allowed: [MathLanguage::Latex].into_iter().collect(),
        },
        // 権限なしと対象不在では、xref本文・labelともに出力しない。
        unresolved_references: UnresolvedReferencePresentation::Hidden,
        resources: ResourceCapabilities {
            images: false,
            media: false,
        },
        ..RenderPolicy::default()
    }
}

/// ノート参照を解決する利用者の認可済み文脈。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Viewer {
    pub user_id: String,
    pub is_root: bool,
}

/// 初回OIDCログイン時に適用する登録ポリシー。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RegistrationPolicy {
    Open,
    #[default]
    Approval,
    InviteOnly,
}

/// アプリケーション内部のユーザー状態。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UserStatus {
    Pending,
    Active,
    Disabled,
}

impl UserStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Disabled => "disabled",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "active" => Some(Self::Active),
            "disabled" => Some(Self::Disabled),
            _ => None,
        }
    }
}

/// OIDCの検証済みID Tokenから得た、永続化に必要な最小の本人情報。
///
/// `issuer`と`subject`だけが本人同定に使われる。`display_name`は表示用の可変属性であり、
/// メールアドレスを含めても同一性判定には使用しない。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OidcIdentity {
    pub issuer: String,
    pub subject: String,
    pub display_name: String,
}

/// OIDC identityに対応するアプリケーション内ユーザー。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OidcUser {
    pub user_id: String,
    pub status: UserStatus,
    pub display_name: String,
}

/// OIDCログイン後にブラウザセッションを発行できるかどうか。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OidcLoginResult {
    Active(OidcUser),
    PendingApproval(OidcUser),
    RegistrationDenied,
    Disabled(OidcUser),
}

/// OIDC認可要求とcallbackを結び付ける一回限りの情報。
///
/// `state`はブラウザ経由で往復し、`nonce`と`pkce_verifier`はcallbackでだけ使う。いずれも
/// `Debug`を実装せず、ログへ出力しない。
pub struct PendingOidcLogin {
    state: String,
    nonce: String,
    pkce_verifier: String,
}

impl PendingOidcLogin {
    pub fn state(&self) -> &str {
        &self.state
    }

    pub fn nonce(&self) -> &str {
        &self.nonce
    }

    pub fn pkce_verifier(&self) -> &str {
        &self.pkce_verifier
    }
}

/// callbackでstateを一度だけ検証した後に使用できるOIDC情報。
pub struct ConsumedOidcLogin {
    nonce: String,
    pkce_verifier: String,
}

impl ConsumedOidcLogin {
    pub fn nonce(&self) -> &str {
        &self.nonce
    }

    pub fn pkce_verifier(&self) -> &str {
        &self.pkce_verifier
    }
}

/// OIDC認可要求の保存・消費に関するエラー。
#[derive(Debug)]
pub enum OidcLoginError {
    InvalidLifetime,
    Database(sqlx::Error),
}

/// サーバ側Webセッションの無操作・絶対有効期限。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WebSessionLifetime {
    pub idle: TimeDuration,
    pub absolute: TimeDuration,
}

impl WebSessionLifetime {
    pub const GENERAL_USER: Self = Self {
        idle: TimeDuration::hours(24),
        absolute: TimeDuration::days(7),
    };

    pub const ROOT: Self = Self {
        idle: TimeDuration::minutes(30),
        absolute: TimeDuration::hours(8),
    };

    pub fn is_valid(self) -> bool {
        self.idle.is_positive() && self.absolute.is_positive() && self.idle <= self.absolute
    }
}

/// 発行直後だけブラウザへ渡すセッションIDとCSRFトークン。
///
/// SQLiteには両方のハッシュだけを保存する。ログ出力を避けるため`Debug`を実装しない。
pub struct IssuedWebSession {
    session_id: String,
    csrf_token: String,
}

impl IssuedWebSession {
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn csrf_token(&self) -> &str {
        &self.csrf_token
    }
}

/// 有効なWebセッションから得た認可済みの利用者文脈。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebSessionActor {
    pub viewer: Viewer,
    pub csrf_token_valid: bool,
}

/// Webセッション操作のエラー。
#[derive(Debug)]
pub enum WebSessionError {
    InvalidLifetime,
    InvalidStoredExpiration,
    Database(sqlx::Error),
}

/// 緊急管理者`root`の初期化・認証エラー。
#[derive(Debug)]
pub enum RootAccountError {
    EmptyPassword,
    PasswordHash,
    InvalidStoredPasswordHash,
    Database(sqlx::Error),
}

impl fmt::Display for RootAccountError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPassword => formatter.write_str("root password must not be empty"),
            Self::PasswordHash => formatter.write_str("root password could not be hashed"),
            Self::InvalidStoredPasswordHash => {
                formatter.write_str("stored root password hash is invalid")
            }
            Self::Database(error) => {
                write!(formatter, "SQLite root account operation failed: {error}")
            }
        }
    }
}

impl std::error::Error for RootAccountError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::EmptyPassword | Self::PasswordHash | Self::InvalidStoredPasswordHash => None,
        }
    }
}

impl From<sqlx::Error> for RootAccountError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

impl fmt::Display for WebSessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLifetime => formatter.write_str("web session lifetime is invalid"),
            Self::InvalidStoredExpiration => {
                formatter.write_str("stored session expiration is invalid")
            }
            Self::Database(error) => {
                write!(formatter, "SQLite web session operation failed: {error}")
            }
        }
    }
}

impl std::error::Error for WebSessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::InvalidLifetime | Self::InvalidStoredExpiration => None,
        }
    }
}

impl From<sqlx::Error> for WebSessionError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

impl fmt::Display for OidcLoginError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLifetime => formatter.write_str("OIDC login lifetime is invalid"),
            Self::Database(error) => {
                write!(formatter, "SQLite OIDC login operation failed: {error}")
            }
        }
    }
}

impl std::error::Error for OidcLoginError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidLifetime => None,
            Self::Database(error) => Some(error),
        }
    }
}

impl From<sqlx::Error> for OidcLoginError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

/// OIDC identityとアプリケーションユーザーの対応を処理するときのエラー。
#[derive(Debug)]
pub enum OidcUserError {
    InvalidIdentity,
    InvalidStoredStatus,
    Database(sqlx::Error),
}

impl fmt::Display for OidcUserError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdentity => {
                formatter.write_str("OIDC issuer and subject must not be empty")
            }
            Self::InvalidStoredStatus => formatter.write_str("stored user status is invalid"),
            Self::Database(error) => {
                write!(formatter, "SQLite OIDC user operation failed: {error}")
            }
        }
    }
}

impl std::error::Error for OidcUserError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::InvalidIdentity | Self::InvalidStoredStatus => None,
        }
    }
}

impl From<sqlx::Error> for OidcUserError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

/// ノートへ直接付与する権限。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(i64)]
pub enum NotePermission {
    Read = 1,
    Write = 2,
    Admin = 3,
}

/// ノートACLの直接変更で守る不変条件に関するエラー。
#[derive(Debug)]
pub enum UpdateNoteAclError {
    LastAdmin,
    Database(sqlx::Error),
}

impl fmt::Display for UpdateNoteAclError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LastAdmin => formatter.write_str("cannot remove the last note administrator"),
            Self::Database(error) => write!(formatter, "SQLite ACL update failed: {error}"),
        }
    }
}

impl std::error::Error for UpdateNoteAclError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::LastAdmin => None,
            Self::Database(error) => Some(error),
        }
    }
}

impl From<sqlx::Error> for UpdateNoteAclError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

/// 絶対HTTPS Base URLから得たアプリ内ノートURLの生成規則。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteUrlBase(Url);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvalidNoteUrlBase;

impl fmt::Display for InvalidNoteUrlBase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(
            "Base URL must be an absolute HTTPS URL without credentials, query, or fragment",
        )
    }
}

impl std::error::Error for InvalidNoteUrlBase {}

impl NoteUrlBase {
    pub fn new(value: impl AsRef<str>) -> Result<Self, InvalidNoteUrlBase> {
        let mut url = Url::parse(value.as_ref()).map_err(|_| InvalidNoteUrlBase)?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(InvalidNoteUrlBase);
        }
        let path = url.path().trim_end_matches('/').to_owned();
        url.set_path(&path);
        Ok(Self(url))
    }

    pub fn note_href(&self, note_id: &str, anchor: Option<&str>) -> String {
        let mut url = self.0.clone();
        let base_path = url.path().trim_end_matches('/').to_owned();
        url.set_path(&format!("{base_path}/note/{note_id}"));
        url.set_fragment(anchor);
        url.into()
    }
}

/// 本文中の一箇所に対応する、永続化する参照投影。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredNoteReference {
    pub source_start: i64,
    pub source_end: i64,
    pub target_note_id: String,
    pub target_anchor: Option<String>,
}

/// 一つの解析revisionから得た、参照解決用のアンカー投影。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredNoteAnchor {
    pub anchor_id: String,
}

/// 同一解析revisionから、SQLiteへ保存する参照先アンカー投影を作る。
///
/// 明示アンカーだけでなく、AdocWeaveが生成する見出しIDも含める。これによりxrefの
/// `#anchor`解決は、HTMLレンダラーが出力するIDと同じ集合を参照する。
pub fn extract_stored_note_anchors(analysis: &adocweave::Analysis) -> Vec<StoredNoteAnchor> {
    analysis
        .reference_targets()
        .iter()
        .map(|target| StoredNoteAnchor {
            anchor_id: target.id.clone(),
        })
        .collect()
}

/// 一つの解析revisionから、SQLiteへ保存する位置付き参照投影を作る。
pub fn extract_stored_note_references(
    analysis: &adocweave::Analysis,
) -> Result<Vec<StoredNoteReference>, Vec<NoteReferenceError>> {
    extract_note_references(analysis).map(|references| {
        references
            .into_iter()
            .map(|reference| StoredNoteReference {
                source_start: i64::from(reference.range.start().to_u32()),
                source_end: i64::from(reference.range.end().to_u32()),
                target_note_id: reference.note_id,
                target_anchor: reference.anchor,
            })
            .collect()
    })
}

/// ACLを適用したノート参照の解決結果。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NoteReferenceResolution {
    /// 参照先とアンカーを閲覧できる。
    Resolved { href: String, title: String },
    /// 参照先は閲覧できるがアンカーがないため、ノート先頭へフォールバックした。
    AnchorFallback { href: String, title: String },
    /// 対象不在と権限なしを区別せず、対象の存在を秘匿する。
    NotFound {
        /// `root`だけが受け取る詳細。通常利用者には常に`None`を返す。
        detail: Option<ReferenceFailureDetail>,
    },
}

/// 権限を持つ運用文脈だけに返す未解決参照の詳細。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReferenceFailureDetail {
    MissingTarget,
}

/// HTML表示へ渡す前の、アプリ固有で位置付きの参照表示情報。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferencePresentation {
    pub range: TextRange,
    /// 空labelの標準xrefに使う、閲覧権限確認済みの解決先タイトル。
    pub display_label: Option<String>,
    /// 解決には成功したが利用者へ伝えるべき警告。
    pub warning: Option<ReferenceWarning>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferenceWarning {
    pub code: &'static str,
    pub message: String,
}

/// 一つの解析revisionに対応する、描画へ渡す参照解決結果。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedNoteReferences {
    pub render_inputs: RenderInputs,
    /// `RenderInputs`の公開契約にはまだ含まれない、アプリ側の表示情報。
    pub presentations: BTreeMap<TextRange, ReferencePresentation>,
}

/// AsciiDoc上の形式検証とSQLite照会を分離する参照解決エラー。
#[derive(Debug)]
pub enum ResolveReferencesError {
    InvalidReferences(Vec<NoteReferenceError>),
    Database(sqlx::Error),
}

/// 一つのAsciiDoc解析revisionをSQLite投影へ反映するときのエラー。
#[derive(Debug)]
pub enum PersistNoteProjectionError {
    Metadata(Vec<NoteProfileError>),
    Content(Vec<NoteContentError>),
    References(Vec<NoteReferenceError>),
    Database(sqlx::Error),
}

impl fmt::Display for PersistNoteProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Metadata(_) => formatter.write_str("invalid note metadata"),
            Self::Content(_) => formatter.write_str("invalid note content"),
            Self::References(_) => formatter.write_str("invalid note references"),
            Self::Database(error) => write!(formatter, "SQLite projection update failed: {error}"),
        }
    }
}

impl std::error::Error for PersistNoteProjectionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::Metadata(_) | Self::Content(_) | Self::References(_) => None,
        }
    }
}

impl From<sqlx::Error> for PersistNoteProjectionError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

impl fmt::Display for ResolveReferencesError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidReferences(_) => formatter.write_str("invalid note reference"),
            Self::Database(error) => {
                write!(formatter, "SQLite reference resolution failed: {error}")
            }
        }
    }
}

impl std::error::Error for ResolveReferencesError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidReferences(_) => None,
            Self::Database(error) => Some(error),
        }
    }
}

impl From<sqlx::Error> for ResolveReferencesError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

/// 解決済みノート参照を使ってHTMLを描画し、フォールバック警告を安全に追記する。
///
/// 警告はアプリが生成したplain textだけを出力する。Resolver由来の表示ラベルをxref本文へ
/// 差し込むAPIは現在のAdocWeave契約にはないため、この関数はその置換を行わない。
pub fn render_note_html(
    analysis: &adocweave::Analysis,
    resolved: &ResolvedNoteReferences,
) -> Result<HtmlOutput, Vec<NoteContentError>> {
    let content_errors = validate_note_content_profile(analysis);
    if !content_errors.is_empty() {
        return Err(content_errors);
    }
    let mut output = render_with_inputs(
        analysis.ast(),
        &note_render_policy(),
        &resolved.render_inputs,
    );
    for presentation in resolved.presentations.values() {
        let Some(warning) = &presentation.warning else {
            continue;
        };
        output
            .html
            .push_str("<aside class=\"note-reference-warning\" role=\"status\" data-code=\"");
        escape_html_into(&mut output.html, warning.code);
        output.html.push_str("\">");
        escape_html_into(&mut output.html, &warning.message);
        output.html.push_str("</aside>\n");
    }
    Ok(output)
}

fn escape_html_into(output: &mut String, value: &str) {
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#39;"),
            _ => output.push(character),
        }
    }
}

fn oidc_user_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<OidcUser, OidcUserError> {
    let status: String = row.try_get("status")?;
    Ok(OidcUser {
        user_id: row.try_get("user_id")?,
        status: UserStatus::parse(&status).ok_or(OidcUserError::InvalidStoredStatus)?,
        display_name: row.try_get("display_name")?,
    })
}

fn random_opaque_token() -> String {
    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn hash_opaque_token(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

fn format_timestamp(value: OffsetDateTime) -> Result<String, WebSessionError> {
    value
        .to_offset(UtcOffset::UTC)
        .replace_nanosecond(value.nanosecond() / 1_000_000 * 1_000_000)
        .map_err(|_| WebSessionError::InvalidStoredExpiration)?
        .format(format_description!(
            "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"
        ))
        .map_err(|_| WebSessionError::InvalidStoredExpiration)
}

/// SQLite上のノート投影ストア。
#[derive(Clone, Debug)]
pub struct NotebookStore {
    pool: SqlitePool,
    write_lock: Arc<Mutex<()>>,
}

impl NotebookStore {
    pub async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        let options = database_url
            .parse::<SqliteConnectOptions>()?
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;
        let store = Self {
            pool,
            write_lock: Arc::new(Mutex::new(())),
        };
        store.migrate().await?;
        Ok(store)
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// SQLiteの参照投影に必要な最小スキーマを作成する。
    pub async fn migrate(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS users (
                user_id TEXT PRIMARY KEY NOT NULL,
                authentication_kind TEXT NOT NULL CHECK (authentication_kind IN ('oidc', 'root')),
                status TEXT NOT NULL CHECK (status IN ('pending', 'active', 'disabled')),
                display_name TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            ) STRICT",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS oidc_identities (
                issuer TEXT NOT NULL,
                subject TEXT NOT NULL,
                user_id TEXT NOT NULL REFERENCES users(user_id),
                PRIMARY KEY (issuer, subject),
                UNIQUE (user_id)
            ) STRICT",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS oidc_login_attempts (
                state_hash BLOB PRIMARY KEY NOT NULL,
                nonce TEXT NOT NULL,
                pkce_verifier TEXT NOT NULL,
                expires_at TEXT NOT NULL
            ) STRICT",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS web_sessions (
                session_id_hash BLOB PRIMARY KEY NOT NULL,
                csrf_token_hash BLOB NOT NULL,
                user_id TEXT NOT NULL REFERENCES users(user_id),
                idle_timeout_seconds INTEGER NOT NULL CHECK (idle_timeout_seconds > 0),
                issued_at TEXT NOT NULL,
                last_seen_at TEXT NOT NULL,
                idle_expires_at TEXT NOT NULL,
                absolute_expires_at TEXT NOT NULL,
                revoked_at TEXT
            ) STRICT",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS root_credentials (
                user_id TEXT PRIMARY KEY NOT NULL REFERENCES users(user_id),
                password_hash TEXT NOT NULL
            ) STRICT",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS web_sessions_user_idx ON web_sessions(user_id)")
            .execute(&self.pool)
            .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS notes (
                note_id TEXT PRIMARY KEY NOT NULL,
                title TEXT NOT NULL DEFAULT '',
                deleted_at TEXT
            ) STRICT",
        )
        .execute(&self.pool)
        .await?;
        let note_columns = sqlx::query("PRAGMA table_info(notes)")
            .fetch_all(&self.pool)
            .await?;
        let has_title = note_columns.iter().any(|column| {
            column
                .try_get::<String, _>("name")
                .is_ok_and(|name| name == "title")
        });
        if !has_title {
            sqlx::query("ALTER TABLE notes ADD COLUMN title TEXT NOT NULL DEFAULT ''")
                .execute(&self.pool)
                .await?;
        }
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS note_acl (
                note_id TEXT NOT NULL REFERENCES notes(note_id) ON DELETE CASCADE,
                user_id TEXT NOT NULL,
                permission INTEGER NOT NULL CHECK (permission BETWEEN 1 AND 3),
                PRIMARY KEY (note_id, user_id)
            ) STRICT",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS note_anchors (
                note_id TEXT NOT NULL REFERENCES notes(note_id) ON DELETE CASCADE,
                anchor_id TEXT NOT NULL,
                PRIMARY KEY (note_id, anchor_id)
            ) STRICT",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS note_references (
                source_note_id TEXT NOT NULL REFERENCES notes(note_id) ON DELETE CASCADE,
                source_start INTEGER NOT NULL CHECK (source_start >= 0),
                source_end INTEGER NOT NULL CHECK (source_end > source_start),
                target_note_id TEXT NOT NULL,
                target_anchor TEXT,
                PRIMARY KEY (source_note_id, source_start, source_end)
            ) STRICT",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS note_references_target_idx
             ON note_references(target_note_id, target_anchor)",
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// 未初期化のDBへ、OIDCと独立した緊急管理者`root`を一度だけ作成する。
    ///
    /// パスワードはArgon2idでハッシュ化し、二度目以降の呼出しでは既存のhashを変更しない。
    pub async fn initialize_root(
        &self,
        password: &str,
        now: &str,
    ) -> Result<bool, RootAccountError> {
        if password.is_empty() {
            return Err(RootAccountError::EmptyPassword);
        }
        let salt = SaltString::generate(&mut PasswordOsRng);
        let password_hash = Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map_err(|_| RootAccountError::PasswordHash)?
            .to_string();
        let _write_guard = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        if sqlx::query("SELECT 1 FROM root_credentials LIMIT 1")
            .fetch_optional(&mut *transaction)
            .await?
            .is_some()
        {
            transaction.commit().await?;
            return Ok(false);
        }
        let user_id = Uuid::now_v7().to_string();
        sqlx::query(
            "INSERT INTO users
             (user_id, authentication_kind, status, display_name, created_at, updated_at)
             VALUES (?, 'root', 'active', 'root', ?, ?)",
        )
        .bind(&user_id)
        .bind(now)
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        sqlx::query("INSERT INTO root_credentials (user_id, password_hash) VALUES (?, ?)")
            .bind(user_id)
            .bind(password_hash)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(true)
    }

    /// `root`のパスワードを検証し、成功時だけrootのViewerを返す。
    pub async fn authenticate_root(
        &self,
        password: &str,
    ) -> Result<Option<Viewer>, RootAccountError> {
        let row = sqlx::query(
            "SELECT users.user_id, root_credentials.password_hash
             FROM root_credentials JOIN users ON users.user_id = root_credentials.user_id
             WHERE users.authentication_kind = 'root' AND users.status = 'active'",
        )
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let password_hash: String = row.try_get("password_hash")?;
        let parsed = PasswordHash::new(&password_hash)
            .map_err(|_| RootAccountError::InvalidStoredPasswordHash)?;
        if Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_err()
        {
            return Ok(None);
        }
        Ok(Some(Viewer {
            user_id: row.try_get("user_id")?,
            is_root: true,
        }))
    }

    /// 新しいOIDC認可要求を保存する。
    ///
    /// `state`はDBへハッシュだけを保存する。PKCE verifierはtoken endpointで一度使うまで必要な
    /// ため、短期の一回限り情報として保存し、消費時に必ず削除する。
    pub async fn begin_oidc_login(
        &self,
        expires_at: &str,
    ) -> Result<PendingOidcLogin, OidcLoginError> {
        let pending = PendingOidcLogin {
            state: random_opaque_token(),
            nonce: random_opaque_token(),
            pkce_verifier: random_opaque_token(),
        };
        let _write_guard = self.write_lock.lock().await;
        sqlx::query(
            "INSERT INTO oidc_login_attempts (state_hash, nonce, pkce_verifier, expires_at)
             VALUES (?, ?, ?, ?)",
        )
        .bind(hash_opaque_token(&pending.state).to_vec())
        .bind(&pending.nonce)
        .bind(&pending.pkce_verifier)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;
        Ok(pending)
    }

    /// 現在時刻から指定時間だけ有効なOIDC認可要求を作る。
    pub async fn begin_oidc_login_for(
        &self,
        lifetime: TimeDuration,
    ) -> Result<PendingOidcLogin, OidcLoginError> {
        if !lifetime.is_positive() {
            return Err(OidcLoginError::InvalidLifetime);
        }
        let expires_at = format_timestamp(OffsetDateTime::now_utc() + lifetime)
            .map_err(|_| OidcLoginError::InvalidLifetime)?;
        self.begin_oidc_login(&expires_at).await
    }

    /// callbackのstateを有効期限内に一度だけ消費する。
    ///
    /// state不一致と期限切れは同じ`None`で扱い、外部へログイン試行の存在を示さない。
    pub async fn consume_oidc_login(
        &self,
        state: &str,
        now: &str,
    ) -> Result<Option<ConsumedOidcLogin>, OidcLoginError> {
        let state_hash = hash_opaque_token(state).to_vec();
        let _write_guard = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let attempt = sqlx::query(
            "SELECT nonce, pkce_verifier FROM oidc_login_attempts
             WHERE state_hash = ? AND expires_at > ?",
        )
        .bind(&state_hash)
        .bind(now)
        .fetch_optional(&mut *transaction)
        .await?;
        sqlx::query("DELETE FROM oidc_login_attempts WHERE state_hash = ?")
            .bind(state_hash)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        attempt
            .map(|row| -> Result<ConsumedOidcLogin, sqlx::Error> {
                Ok(ConsumedOidcLogin {
                    nonce: row.try_get("nonce")?,
                    pkce_verifier: row.try_get("pkce_verifier")?,
                })
            })
            .transpose()
            .map_err(OidcLoginError::from)
    }

    /// 有効な内部ユーザーへ新しいサーバ側Webセッションを発行する。
    pub async fn create_web_session(
        &self,
        user_id: &str,
        lifetime: WebSessionLifetime,
    ) -> Result<Option<IssuedWebSession>, WebSessionError> {
        self.create_web_session_at(user_id, lifetime, OffsetDateTime::now_utc())
            .await
    }

    /// テスト可能な時刻指定版のWebセッション発行。
    pub async fn create_web_session_at(
        &self,
        user_id: &str,
        lifetime: WebSessionLifetime,
        now: OffsetDateTime,
    ) -> Result<Option<IssuedWebSession>, WebSessionError> {
        if !lifetime.is_valid() {
            return Err(WebSessionError::InvalidLifetime);
        }
        let _write_guard = self.write_lock.lock().await;
        let active = sqlx::query("SELECT 1 FROM users WHERE user_id = ? AND status = 'active'")
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await?
            .is_some();
        if !active {
            return Ok(None);
        }
        let session = IssuedWebSession {
            session_id: random_opaque_token(),
            csrf_token: random_opaque_token(),
        };
        let issued_at = format_timestamp(now)?;
        let idle_expires_at = format_timestamp(now + lifetime.idle)?;
        let absolute_expires_at = format_timestamp(now + lifetime.absolute)?;
        sqlx::query(
            "INSERT INTO web_sessions
             (session_id_hash, csrf_token_hash, user_id, idle_timeout_seconds, issued_at,
              last_seen_at, idle_expires_at, absolute_expires_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(hash_opaque_token(&session.session_id).to_vec())
        .bind(hash_opaque_token(&session.csrf_token).to_vec())
        .bind(user_id)
        .bind(lifetime.idle.whole_seconds())
        .bind(&issued_at)
        .bind(&issued_at)
        .bind(idle_expires_at)
        .bind(absolute_expires_at)
        .execute(&self.pool)
        .await?;
        Ok(Some(session))
    }

    /// セッションIDとCSRF tokenを検証し、無操作期限を絶対期限の範囲内で延長する。
    pub async fn authenticate_web_session(
        &self,
        session_id: &str,
        csrf_token: Option<&str>,
    ) -> Result<Option<WebSessionActor>, WebSessionError> {
        self.authenticate_web_session_at(session_id, csrf_token, OffsetDateTime::now_utc())
            .await
    }

    /// テスト可能な時刻指定版のセッション認証。
    pub async fn authenticate_web_session_at(
        &self,
        session_id: &str,
        csrf_token: Option<&str>,
        now: OffsetDateTime,
    ) -> Result<Option<WebSessionActor>, WebSessionError> {
        let now = format_timestamp(now)?;
        let session_hash = hash_opaque_token(session_id).to_vec();
        let _write_guard = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let session = sqlx::query(
            "SELECT users.user_id, users.authentication_kind, web_sessions.csrf_token_hash,
                    web_sessions.idle_timeout_seconds, web_sessions.absolute_expires_at
             FROM web_sessions JOIN users ON users.user_id = web_sessions.user_id
             WHERE web_sessions.session_id_hash = ?
               AND web_sessions.revoked_at IS NULL
               AND web_sessions.idle_expires_at > ?
               AND web_sessions.absolute_expires_at > ?
               AND users.status = 'active'",
        )
        .bind(&session_hash)
        .bind(&now)
        .bind(&now)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(session) = session else {
            transaction.commit().await?;
            return Ok(None);
        };
        let absolute_expires_at: String = session.try_get("absolute_expires_at")?;
        let absolute_expires_at = OffsetDateTime::parse(&absolute_expires_at, &Rfc3339)
            .map_err(|_| WebSessionError::InvalidStoredExpiration)?;
        let idle_timeout_seconds: i64 = session.try_get("idle_timeout_seconds")?;
        let current_time = OffsetDateTime::parse(&now, &Rfc3339)
            .map_err(|_| WebSessionError::InvalidStoredExpiration)?;
        let desired_idle_expiry = current_time + TimeDuration::seconds(idle_timeout_seconds);
        let idle_expires_at = format_timestamp(if desired_idle_expiry < absolute_expires_at {
            desired_idle_expiry
        } else {
            absolute_expires_at
        })?;
        sqlx::query(
            "UPDATE web_sessions SET last_seen_at = ?, idle_expires_at = ? WHERE session_id_hash = ?",
        )
        .bind(&now)
        .bind(idle_expires_at)
        .bind(session_hash)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        let csrf_hash: Vec<u8> = session.try_get("csrf_token_hash")?;
        let authentication_kind: String = session.try_get("authentication_kind")?;
        Ok(Some(WebSessionActor {
            viewer: Viewer {
                user_id: session.try_get("user_id")?,
                is_root: authentication_kind == "root",
            },
            csrf_token_valid: csrf_token
                .is_some_and(|token| csrf_hash == hash_opaque_token(token).as_slice()),
        }))
    }

    /// 一つのWebセッションを即時失効させる。存在しない識別子も成功として扱う。
    pub async fn revoke_web_session(&self, session_id: &str) -> Result<(), WebSessionError> {
        let _write_guard = self.write_lock.lock().await;
        sqlx::query("UPDATE web_sessions SET revoked_at = ? WHERE session_id_hash = ?")
            .bind(format_timestamp(OffsetDateTime::now_utc())?)
            .bind(hash_opaque_token(session_id).to_vec())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// 検証済みOIDC identityを既存ユーザーへ対応付け、初回だけ登録ポリシーを適用する。
    ///
    /// identity対応は削除・再利用しないため、同じ`(issuer, subject)`を別ユーザーへ割り当てる
    /// ことはできない。`now`はUTC RFC 3339・ミリ秒精度の時刻を呼び出し側から渡す。
    pub async fn register_or_lookup_oidc_user(
        &self,
        identity: &OidcIdentity,
        policy: RegistrationPolicy,
        now: &str,
    ) -> Result<OidcLoginResult, OidcUserError> {
        if identity.issuer.trim().is_empty() || identity.subject.trim().is_empty() {
            return Err(OidcUserError::InvalidIdentity);
        }
        let _write_guard = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let existing = sqlx::query(
            "SELECT users.user_id, users.status, users.display_name
             FROM oidc_identities
             JOIN users ON users.user_id = oidc_identities.user_id
             WHERE oidc_identities.issuer = ? AND oidc_identities.subject = ?",
        )
        .bind(&identity.issuer)
        .bind(&identity.subject)
        .fetch_optional(&mut *transaction)
        .await?;
        let user = if let Some(row) = existing {
            let user = oidc_user_from_row(&row)?;
            sqlx::query("UPDATE users SET display_name = ?, updated_at = ? WHERE user_id = ?")
                .bind(&identity.display_name)
                .bind(now)
                .bind(&user.user_id)
                .execute(&mut *transaction)
                .await?;
            OidcUser {
                display_name: identity.display_name.clone(),
                ..user
            }
        } else {
            if policy == RegistrationPolicy::InviteOnly {
                transaction.rollback().await?;
                return Ok(OidcLoginResult::RegistrationDenied);
            }
            let status = match policy {
                RegistrationPolicy::Open => UserStatus::Active,
                RegistrationPolicy::Approval => UserStatus::Pending,
                RegistrationPolicy::InviteOnly => unreachable!("handled before user creation"),
            };
            let user = OidcUser {
                user_id: Uuid::now_v7().to_string(),
                status,
                display_name: identity.display_name.clone(),
            };
            sqlx::query(
                "INSERT INTO users
                 (user_id, authentication_kind, status, display_name, created_at, updated_at)
                 VALUES (?, 'oidc', ?, ?, ?, ?)",
            )
            .bind(&user.user_id)
            .bind(status.as_str())
            .bind(&user.display_name)
            .bind(now)
            .bind(now)
            .execute(&mut *transaction)
            .await?;
            sqlx::query("INSERT INTO oidc_identities (issuer, subject, user_id) VALUES (?, ?, ?)")
                .bind(&identity.issuer)
                .bind(&identity.subject)
                .bind(&user.user_id)
                .execute(&mut *transaction)
                .await?;
            user
        };
        transaction.commit().await?;
        Ok(match user.status {
            UserStatus::Active => OidcLoginResult::Active(user),
            UserStatus::Pending => OidcLoginResult::PendingApproval(user),
            UserStatus::Disabled => OidcLoginResult::Disabled(user),
        })
    }

    /// 同じノートに属する参照位置をトランザクションで全置換する。
    pub async fn replace_references(
        &self,
        source_note_id: &str,
        references: &[StoredNoteReference],
    ) -> Result<(), sqlx::Error> {
        let _write_guard = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        sqlx::query("DELETE FROM note_references WHERE source_note_id = ?")
            .bind(source_note_id)
            .execute(&mut *transaction)
            .await?;
        for reference in references {
            sqlx::query(
                "INSERT INTO note_references \
                 (source_note_id, source_start, source_end, target_note_id, target_anchor) \
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(source_note_id)
            .bind(reference.source_start)
            .bind(reference.source_end)
            .bind(&reference.target_note_id)
            .bind(&reference.target_anchor)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await
    }

    /// 同じノートに属するアンカーをトランザクションで全置換する。
    pub async fn replace_anchors(
        &self,
        note_id: &str,
        anchors: &[StoredNoteAnchor],
    ) -> Result<(), sqlx::Error> {
        let _write_guard = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        sqlx::query("DELETE FROM note_anchors WHERE note_id = ?")
            .bind(note_id)
            .execute(&mut *transaction)
            .await?;
        for anchor in anchors {
            sqlx::query("INSERT INTO note_anchors (note_id, anchor_id) VALUES (?, ?)")
                .bind(note_id)
                .bind(&anchor.anchor_id)
                .execute(&mut *transaction)
                .await?;
        }
        transaction.commit().await
    }

    /// 同一解析revisionから得たアンカーと参照位置を、一つのトランザクションで置換する。
    ///
    /// 通常のノート保存・投影再構築ではこのメソッドを使う。個別の置換メソッドは、段階的な
    /// 移行や保守操作のために残す。
    pub async fn replace_note_link_projection(
        &self,
        note_id: &str,
        anchors: &[StoredNoteAnchor],
        references: &[StoredNoteReference],
    ) -> Result<(), sqlx::Error> {
        let _write_guard = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        sqlx::query("DELETE FROM note_anchors WHERE note_id = ?")
            .bind(note_id)
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM note_references WHERE source_note_id = ?")
            .bind(note_id)
            .execute(&mut *transaction)
            .await?;
        for anchor in anchors {
            sqlx::query("INSERT INTO note_anchors (note_id, anchor_id) VALUES (?, ?)")
                .bind(note_id)
                .bind(&anchor.anchor_id)
                .execute(&mut *transaction)
                .await?;
        }
        for reference in references {
            sqlx::query(
                "INSERT INTO note_references \
                 (source_note_id, source_start, source_end, target_note_id, target_anchor) \
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(note_id)
            .bind(reference.source_start)
            .bind(reference.source_end)
            .bind(&reference.target_note_id)
            .bind(&reference.target_anchor)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await
    }

    /// 保存可能なノートを検証し、同じ解析revisionから得たSQLite投影を原子的に更新する。
    ///
    /// この境界はファイル書込みを行わない。呼び出し側は、AsciiDoc正本のアトミックな置換と
    /// 成功後のこの投影更新を調停する。
    pub async fn persist_note_projection(
        &self,
        analysis: &adocweave::Analysis,
    ) -> Result<(), PersistNoteProjectionError> {
        let metadata =
            validate_note_metadata(analysis).map_err(PersistNoteProjectionError::Metadata)?;
        let content_errors = validate_note_content_profile(analysis);
        if !content_errors.is_empty() {
            return Err(PersistNoteProjectionError::Content(content_errors));
        }
        let references = extract_stored_note_references(analysis)
            .map_err(PersistNoteProjectionError::References)?;
        let anchors = extract_stored_note_anchors(analysis);

        let _write_guard = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let note_exists = sqlx::query("SELECT 1 FROM notes WHERE note_id = ?")
            .bind(&metadata.note_id)
            .fetch_optional(&mut *transaction)
            .await?
            .is_some();
        sqlx::query(
            "INSERT INTO notes(note_id, title) VALUES (?, ?) \
             ON CONFLICT(note_id) DO UPDATE SET title = excluded.title",
        )
        .bind(&metadata.note_id)
        .bind(&metadata.title)
        .execute(&mut *transaction)
        .await?;
        if !note_exists {
            sqlx::query("INSERT INTO note_acl(note_id, user_id, permission) VALUES (?, ?, ?)")
                .bind(&metadata.note_id)
                .bind(&metadata.creator_id)
                .bind(NotePermission::Admin as i64)
                .execute(&mut *transaction)
                .await?;
        }
        sqlx::query("DELETE FROM note_anchors WHERE note_id = ?")
            .bind(&metadata.note_id)
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM note_references WHERE source_note_id = ?")
            .bind(&metadata.note_id)
            .execute(&mut *transaction)
            .await?;
        for anchor in &anchors {
            sqlx::query("INSERT INTO note_anchors (note_id, anchor_id) VALUES (?, ?)")
                .bind(&metadata.note_id)
                .bind(&anchor.anchor_id)
                .execute(&mut *transaction)
                .await?;
        }
        for reference in &references {
            sqlx::query(
                "INSERT INTO note_references \
                 (source_note_id, source_start, source_end, target_note_id, target_anchor) \
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(&metadata.note_id)
            .bind(reference.source_start)
            .bind(reference.source_end)
            .bind(&reference.target_note_id)
            .bind(&reference.target_anchor)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    /// 一人の利用者へノートの直接権限を設定または解除する。
    ///
    /// 認可（呼び出し元が管理者またはrootであること）の確認はアプリケーションサービス層が
    /// 行う。この層では、最後の直接管理者を解除・降格できない不変条件だけを原子的に守る。
    pub async fn update_note_acl(
        &self,
        note_id: &str,
        user_id: &str,
        permission: Option<NotePermission>,
    ) -> Result<(), UpdateNoteAclError> {
        let _write_guard = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let current_permission =
            sqlx::query("SELECT permission FROM note_acl WHERE note_id = ? AND user_id = ?")
                .bind(note_id)
                .bind(user_id)
                .fetch_optional(&mut *transaction)
                .await?
                .map(|row| row.try_get::<i64, _>("permission"))
                .transpose()?;
        let removes_admin = current_permission == Some(NotePermission::Admin as i64)
            && permission != Some(NotePermission::Admin);
        if removes_admin {
            let admin_count: i64 = sqlx::query(
                "SELECT COUNT(*) AS count FROM note_acl WHERE note_id = ? AND permission = ?",
            )
            .bind(note_id)
            .bind(NotePermission::Admin as i64)
            .fetch_one(&mut *transaction)
            .await?
            .try_get("count")?;
            if admin_count <= 1 {
                return Err(UpdateNoteAclError::LastAdmin);
            }
        }
        match permission {
            Some(permission) => {
                sqlx::query(
                    "INSERT INTO note_acl(note_id, user_id, permission) VALUES (?, ?, ?) \
                     ON CONFLICT(note_id, user_id) DO UPDATE SET permission = excluded.permission",
                )
                .bind(note_id)
                .bind(user_id)
                .bind(permission as i64)
                .execute(&mut *transaction)
                .await?;
            }
            None => {
                sqlx::query("DELETE FROM note_acl WHERE note_id = ? AND user_id = ?")
                    .bind(note_id)
                    .bind(user_id)
                    .execute(&mut *transaction)
                    .await?;
            }
        }
        transaction.commit().await?;
        Ok(())
    }

    /// 対象不在とACL拒否を同じ結果に畳み込み、存在を推測できないようにする。
    pub async fn resolve_note_reference(
        &self,
        viewer: &Viewer,
        urls: &NoteUrlBase,
        note_id: &str,
        anchor: Option<&str>,
    ) -> Result<NoteReferenceResolution, sqlx::Error> {
        let accessible = sqlx::query(
            "SELECT note_id, title FROM notes
             WHERE note_id = ? AND deleted_at IS NULL
             AND (? = 1 OR EXISTS (
                 SELECT 1 FROM note_acl
                 WHERE note_acl.note_id = notes.note_id
                 AND user_id = ? AND permission >= 1
             ))",
        )
        .bind(note_id)
        .bind(viewer.is_root)
        .bind(&viewer.user_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(accessible) = accessible else {
            return Ok(NoteReferenceResolution::NotFound {
                detail: viewer
                    .is_root
                    .then_some(ReferenceFailureDetail::MissingTarget),
            });
        };
        let title: String = accessible.try_get("title")?;
        let Some(anchor) = anchor else {
            return Ok(NoteReferenceResolution::Resolved {
                href: urls.note_href(note_id, None),
                title,
            });
        };
        let anchor_exists =
            sqlx::query("SELECT anchor_id FROM note_anchors WHERE note_id = ? AND anchor_id = ?")
                .bind(note_id)
                .bind(anchor)
                .fetch_optional(&self.pool)
                .await?
                .is_some();
        if anchor_exists {
            Ok(NoteReferenceResolution::Resolved {
                href: urls.note_href(note_id, Some(anchor)),
                title,
            })
        } else {
            Ok(NoteReferenceResolution::AnchorFallback {
                href: urls.note_href(note_id, None),
                title,
            })
        }
    }

    /// 解析済みAsciiDocの`xref:note:`を同一revisionの描画入力へ変換する。
    pub async fn resolve_render_inputs(
        &self,
        analysis: &adocweave::Analysis,
        viewer: &Viewer,
        urls: &NoteUrlBase,
    ) -> Result<ResolvedNoteReferences, ResolveReferencesError> {
        let references =
            extract_note_references(analysis).map_err(ResolveReferencesError::InvalidReferences)?;
        let mut render_references = Vec::with_capacity(references.len());
        let mut presentations = BTreeMap::new();
        for reference in references {
            match self
                .resolve_note_reference(
                    viewer,
                    urls,
                    &reference.note_id,
                    reference.anchor.as_deref(),
                )
                .await?
            {
                NoteReferenceResolution::Resolved { href, title } => {
                    if reference.label_is_empty {
                        presentations.insert(
                            reference.range,
                            ReferencePresentation {
                                range: reference.range,
                                display_label: Some(title),
                                warning: None,
                            },
                        );
                    }
                    render_references.push(ResolvedReference::resolved(reference.range, href));
                }
                NoteReferenceResolution::AnchorFallback { href, title } => {
                    presentations.insert(
                        reference.range,
                        ReferencePresentation {
                            range: reference.range,
                            display_label: reference.label_is_empty.then_some(title),
                            warning: Some(ReferenceWarning {
                                code: "missing-reference-anchor",
                                message:
                                    "参照先アンカーが見つからないため、ノート先頭を表示します。"
                                        .into(),
                            }),
                        },
                    );
                    render_references.push(ResolvedReference::resolved_with_notices(
                        reference.range,
                        href,
                        vec![ResolutionNotice {
                            kind: ResolutionNoticeKind::Fallback,
                        }],
                    ));
                }
                NoteReferenceResolution::NotFound { .. } => {
                    render_references.push(ResolvedReference::failed(
                        reference.range,
                        ResolverFailure {
                            kind: ResolutionFailureKind::MissingTarget,
                            message: "note reference unavailable".into(),
                        },
                    ));
                }
            }
        }
        Ok(ResolvedNoteReferences {
            render_inputs: RenderInputs::new(render_references, Vec::new()),
            presentations,
        })
    }
}

#[cfg(test)]
mod tests {
    use adocweave::{
        Engine,
        reference::{
            ResolutionFailureKind, ResolutionNotice, ResolutionNoticeKind, ResolutionOutcome,
            ResolverFailure,
        },
        render::RenderInputs,
    };
    use sqlx::Row;
    use time::{Duration as TimeDuration, OffsetDateTime};

    use super::{
        NotePermission, NoteReferenceResolution, NoteUrlBase, NotebookStore, OidcIdentity,
        OidcLoginResult, ReferenceFailureDetail, RegistrationPolicy, StoredNoteAnchor,
        StoredNoteReference, UpdateNoteAclError, UserStatus, Viewer, extract_stored_note_anchors,
        extract_stored_note_references, render_note_html,
    };

    #[tokio::test]
    async fn oidc_identity_uses_issuer_and_subject_not_mutable_display_claims() {
        let store = NotebookStore::connect("sqlite::memory:")
            .await
            .expect("open store");
        let first = store
            .register_or_lookup_oidc_user(
                &OidcIdentity {
                    issuer: "https://id.sandi05.com".into(),
                    subject: "user-123".into(),
                    display_name: "First name".into(),
                },
                RegistrationPolicy::Open,
                "2026-07-22T00:00:00.000Z",
            )
            .await
            .expect("register user");
        let OidcLoginResult::Active(first) = first else {
            panic!("open registration must activate the user");
        };

        let second = store
            .register_or_lookup_oidc_user(
                &OidcIdentity {
                    issuer: "https://id.sandi05.com".into(),
                    subject: "user-123".into(),
                    display_name: "Renamed user".into(),
                },
                RegistrationPolicy::Approval,
                "2026-07-22T00:01:00.000Z",
            )
            .await
            .expect("look up user");
        let OidcLoginResult::Active(second) = second else {
            panic!("existing active user must remain active");
        };

        assert_eq!(second.user_id, first.user_id);
        assert_eq!(second.display_name, "Renamed user");
        assert_eq!(second.status, UserStatus::Active);
        assert!(second.user_id.contains("-"));
        let identity_count: i64 = sqlx::query("SELECT COUNT(*) AS count FROM oidc_identities")
            .fetch_one(store.pool())
            .await
            .expect("count identities")
            .get("count");
        assert_eq!(identity_count, 1);
    }

    #[tokio::test]
    async fn approval_creates_pending_user_and_invite_only_does_not_create_one() {
        let store = NotebookStore::connect("sqlite::memory:")
            .await
            .expect("open store");
        let pending = store
            .register_or_lookup_oidc_user(
                &OidcIdentity {
                    issuer: "https://id.sandi05.com".into(),
                    subject: "pending-user".into(),
                    display_name: "Pending".into(),
                },
                RegistrationPolicy::Approval,
                "2026-07-22T00:00:00.000Z",
            )
            .await
            .expect("create pending user");
        assert!(matches!(
            pending,
            OidcLoginResult::PendingApproval(ref user) if user.status == UserStatus::Pending
        ));

        let denied = store
            .register_or_lookup_oidc_user(
                &OidcIdentity {
                    issuer: "https://id.sandi05.com".into(),
                    subject: "uninvited-user".into(),
                    display_name: "Uninvited".into(),
                },
                RegistrationPolicy::InviteOnly,
                "2026-07-22T00:00:00.000Z",
            )
            .await
            .expect("deny uninvited user");
        assert_eq!(denied, OidcLoginResult::RegistrationDenied);
        let count: i64 = sqlx::query("SELECT COUNT(*) AS count FROM users")
            .fetch_one(store.pool())
            .await
            .expect("count users")
            .get("count");
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn oidc_login_state_is_hashed_and_can_only_be_consumed_once() {
        let store = NotebookStore::connect("sqlite::memory:")
            .await
            .expect("open store");
        let pending = store
            .begin_oidc_login("2026-07-22T00:05:00.000Z")
            .await
            .expect("begin OIDC login");
        let stored_hash: Vec<u8> = sqlx::query("SELECT state_hash FROM oidc_login_attempts")
            .fetch_one(store.pool())
            .await
            .expect("read stored state hash")
            .get("state_hash");
        assert_ne!(stored_hash, pending.state().as_bytes());

        let consumed = store
            .consume_oidc_login(pending.state(), "2026-07-22T00:01:00.000Z")
            .await
            .expect("consume login")
            .expect("state must exist once");
        assert_eq!(consumed.nonce(), pending.nonce());
        assert_eq!(consumed.pkce_verifier(), pending.pkce_verifier());
        assert!(
            store
                .consume_oidc_login(pending.state(), "2026-07-22T00:01:00.000Z")
                .await
                .expect("repeat callback")
                .is_none()
        );
    }

    #[tokio::test]
    async fn expired_oidc_login_state_is_not_accepted_and_is_removed() {
        let store = NotebookStore::connect("sqlite::memory:")
            .await
            .expect("open store");
        let pending = store
            .begin_oidc_login("2026-07-22T00:00:00.000Z")
            .await
            .expect("begin OIDC login");

        assert!(
            store
                .consume_oidc_login(pending.state(), "2026-07-22T00:00:00.000Z")
                .await
                .expect("consume expired state")
                .is_none()
        );
        let count: i64 = sqlx::query("SELECT COUNT(*) AS count FROM oidc_login_attempts")
            .fetch_one(store.pool())
            .await
            .expect("count attempts")
            .get("count");
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn web_session_stores_only_hashes_and_extends_idle_expiration() {
        let store = NotebookStore::connect("sqlite::memory:")
            .await
            .expect("open store");
        let user = store
            .register_or_lookup_oidc_user(
                &OidcIdentity {
                    issuer: "https://id.sandi05.com".into(),
                    subject: "session-user".into(),
                    display_name: "Session user".into(),
                },
                RegistrationPolicy::Open,
                "2026-07-22T00:00:00.000Z",
            )
            .await
            .expect("create user");
        let OidcLoginResult::Active(user) = user else {
            panic!("open registration must activate the user");
        };
        let now = OffsetDateTime::UNIX_EPOCH + TimeDuration::days(20_000);
        let session = store
            .create_web_session_at(
                &user.user_id,
                super::WebSessionLifetime {
                    idle: TimeDuration::minutes(10),
                    absolute: TimeDuration::hours(1),
                },
                now,
            )
            .await
            .expect("create session")
            .expect("active user receives a session");
        let stored_hash: Vec<u8> = sqlx::query("SELECT session_id_hash FROM web_sessions")
            .fetch_one(store.pool())
            .await
            .expect("read session hash")
            .get("session_id_hash");
        assert_ne!(stored_hash, session.session_id().as_bytes());

        let authenticated = store
            .authenticate_web_session_at(
                session.session_id(),
                Some(session.csrf_token()),
                now + TimeDuration::minutes(9),
            )
            .await
            .expect("authenticate session")
            .expect("session remains active");
        assert_eq!(authenticated.viewer.user_id, user.user_id);
        assert!(!authenticated.viewer.is_root);
        assert!(authenticated.csrf_token_valid);

        assert!(
            store
                .authenticate_web_session_at(
                    session.session_id(),
                    None,
                    now + TimeDuration::hours(1),
                )
                .await
                .expect("check absolute expiry")
                .is_none()
        );
    }

    #[tokio::test]
    async fn pending_user_cannot_receive_web_session() {
        let store = NotebookStore::connect("sqlite::memory:")
            .await
            .expect("open store");
        let user = store
            .register_or_lookup_oidc_user(
                &OidcIdentity {
                    issuer: "https://id.sandi05.com".into(),
                    subject: "pending-session-user".into(),
                    display_name: "Pending user".into(),
                },
                RegistrationPolicy::Approval,
                "2026-07-22T00:00:00.000Z",
            )
            .await
            .expect("create user");
        let OidcLoginResult::PendingApproval(user) = user else {
            panic!("approval registration must create a pending user");
        };
        assert!(
            store
                .create_web_session_at(
                    &user.user_id,
                    super::WebSessionLifetime::GENERAL_USER,
                    OffsetDateTime::UNIX_EPOCH,
                )
                .await
                .expect("try create session")
                .is_none()
        );
    }

    #[tokio::test]
    async fn initializes_root_once_with_a_non_plaintext_password_hash() {
        let store = NotebookStore::connect("sqlite::memory:")
            .await
            .expect("open store");
        assert!(
            store
                .initialize_root("correct horse battery staple", "2026-07-22T00:00:00.000Z")
                .await
                .expect("initialize root")
        );
        assert!(
            !store
                .initialize_root("replacement password", "2026-07-22T00:01:00.000Z")
                .await
                .expect("do not replace root")
        );
        let password_hash: String = sqlx::query("SELECT password_hash FROM root_credentials")
            .fetch_one(store.pool())
            .await
            .expect("read hash")
            .get("password_hash");
        assert_ne!(password_hash, "correct horse battery staple");
        assert!(password_hash.starts_with("$argon2id$"));
        assert!(
            store
                .authenticate_root("correct horse battery staple")
                .await
                .expect("authenticate root")
                .is_some()
        );
        assert!(
            store
                .authenticate_root("incorrect password")
                .await
                .expect("reject bad password")
                .is_none()
        );
    }

    async fn insert_note(store: &NotebookStore, note_id: &str) {
        sqlx::query("INSERT INTO notes(note_id, title) VALUES (?, ?)")
            .bind(note_id)
            .bind(format!("title:{note_id}"))
            .execute(store.pool())
            .await
            .expect("insert note");
    }

    #[tokio::test]
    async fn stores_each_reference_position_separately() {
        let store = NotebookStore::connect("sqlite::memory:")
            .await
            .expect("open store");
        insert_note(&store, "source").await;
        store
            .replace_references(
                "source",
                &[
                    StoredNoteReference {
                        source_start: 3,
                        source_end: 14,
                        target_note_id: "target".into(),
                        target_anchor: None,
                    },
                    StoredNoteReference {
                        source_start: 21,
                        source_end: 37,
                        target_note_id: "target".into(),
                        target_anchor: Some("details".into()),
                    },
                ],
            )
            .await
            .expect("store positions");

        let count: i64 = sqlx::query("SELECT COUNT(*) AS count FROM note_references")
            .fetch_one(store.pool())
            .await
            .expect("count positions")
            .get("count");
        assert_eq!(count, 2);
    }

    #[test]
    fn preserves_note_reference_byte_ranges_for_storage() {
        let target = "01800000-0000-7000-8000-000000000001";
        let source = format!("xref:note:{target}[first] xref:note:{target}#part[second]\n");
        let analysis = Engine::new(Default::default())
            .analyze(&source)
            .expect("valid AsciiDoc");

        let references = extract_stored_note_references(&analysis).expect("valid references");
        assert_eq!(references.len(), 2);
        assert_eq!(references[0].source_start, 0);
        assert!(references[0].source_end > references[0].source_start);
        assert_eq!(references[0].target_note_id, target);
        assert_eq!(references[0].target_anchor, None);
        assert_eq!(references[1].target_anchor.as_deref(), Some("part"));
    }

    #[tokio::test]
    async fn rebuilds_generated_and_explicit_anchors() {
        let store = NotebookStore::connect("sqlite::memory:")
            .await
            .expect("open store");
        insert_note(&store, "source").await;
        let analysis = Engine::new(Default::default())
            .analyze("== Generated heading\n\n[[stable]]\n== Explicit heading\n")
            .expect("valid AsciiDoc");
        let anchors = extract_stored_note_anchors(&analysis);
        assert!(
            anchors
                .iter()
                .any(|anchor| anchor.anchor_id == "_generated_heading")
        );
        assert!(anchors.iter().any(|anchor| anchor.anchor_id == "stable"));

        store
            .replace_anchors("source", &anchors)
            .await
            .expect("store anchors");
        let count: i64 =
            sqlx::query("SELECT COUNT(*) AS count FROM note_anchors WHERE note_id = 'source'")
                .fetch_one(store.pool())
                .await
                .expect("count anchors")
                .get("count");
        assert_eq!(count, anchors.len() as i64);

        store
            .replace_anchors(
                "source",
                &[StoredNoteAnchor {
                    anchor_id: "replacement".into(),
                }],
            )
            .await
            .expect("replace anchors");
        let replaced: String =
            sqlx::query("SELECT anchor_id FROM note_anchors WHERE note_id = 'source'")
                .fetch_one(store.pool())
                .await
                .expect("read replacement")
                .get("anchor_id");
        assert_eq!(replaced, "replacement");
    }

    #[tokio::test]
    async fn replaces_anchors_and_references_from_one_revision() {
        let store = NotebookStore::connect("sqlite::memory:")
            .await
            .expect("open store");
        insert_note(&store, "source").await;
        let target = "01800000-0000-7000-8000-000000000001";
        let analysis = Engine::new(Default::default())
            .analyze(&format!("== Target\n\nxref:note:{target}[]\n"))
            .expect("valid AsciiDoc");

        store
            .replace_note_link_projection(
                "source",
                &extract_stored_note_anchors(&analysis),
                &extract_stored_note_references(&analysis).expect("valid references"),
            )
            .await
            .expect("replace link projection");
        let anchor_count: i64 =
            sqlx::query("SELECT COUNT(*) AS count FROM note_anchors WHERE note_id = 'source'")
                .fetch_one(store.pool())
                .await
                .expect("count anchors")
                .get("count");
        let reference_count: i64 = sqlx::query(
            "SELECT COUNT(*) AS count FROM note_references WHERE source_note_id = 'source'",
        )
        .fetch_one(store.pool())
        .await
        .expect("count references")
        .get("count");
        assert_eq!(anchor_count, 1);
        assert_eq!(reference_count, 1);
    }

    #[tokio::test]
    async fn persists_validated_metadata_and_link_projection_together() {
        let store = NotebookStore::connect("sqlite::memory:")
            .await
            .expect("open store");
        let note_id = "01800000-0000-7000-8000-000000000001";
        let target_id = "01800000-0000-7000-8000-000000000002";
        let source = format!(
            "= Initial title\n\
             :note-id: {note_id}\n\
             :creator-id: 01800000-0000-7000-8000-000000000003\n\
             :created-at: 2026-07-21T00:00:00.000Z\n\
             :updated-at: 2026-07-22T00:00:00.000Z\n\
             :tags: integration\n\n\
             [[stable]]\n== Section\n\n\
             xref:note:{target_id}[]\n"
        );
        let analysis = Engine::new(Default::default())
            .analyze(&source)
            .expect("valid note");

        store
            .persist_note_projection(&analysis)
            .await
            .expect("persist projection");
        let title: String = sqlx::query("SELECT title FROM notes WHERE note_id = ?")
            .bind(note_id)
            .fetch_one(store.pool())
            .await
            .expect("read title")
            .get("title");
        assert_eq!(title, "Initial title");
        let creator_permission: i64 =
            sqlx::query("SELECT permission FROM note_acl WHERE note_id = ? AND user_id = ?")
                .bind(note_id)
                .bind("01800000-0000-7000-8000-000000000003")
                .fetch_one(store.pool())
                .await
                .expect("read creator permission")
                .get("permission");
        assert_eq!(creator_permission, NotePermission::Admin as i64);
        let anchor_count: i64 =
            sqlx::query("SELECT COUNT(*) AS count FROM note_anchors WHERE note_id = ?")
                .bind(note_id)
                .fetch_one(store.pool())
                .await
                .expect("count anchors")
                .get("count");
        let reference_count: i64 =
            sqlx::query("SELECT COUNT(*) AS count FROM note_references WHERE source_note_id = ?")
                .bind(note_id)
                .fetch_one(store.pool())
                .await
                .expect("count references")
                .get("count");
        assert_eq!(anchor_count, 2);
        assert_eq!(reference_count, 1);
    }

    #[tokio::test]
    async fn prevents_removing_or_demoting_the_last_note_admin() {
        let store = NotebookStore::connect("sqlite::memory:")
            .await
            .expect("open store");
        insert_note(&store, "note").await;
        store
            .update_note_acl("note", "owner", Some(NotePermission::Admin))
            .await
            .expect("grant initial admin");

        assert!(matches!(
            store
                .update_note_acl("note", "owner", Some(NotePermission::Write))
                .await,
            Err(UpdateNoteAclError::LastAdmin)
        ));
        assert!(matches!(
            store.update_note_acl("note", "owner", None).await,
            Err(UpdateNoteAclError::LastAdmin)
        ));

        store
            .update_note_acl("note", "second-admin", Some(NotePermission::Admin))
            .await
            .expect("grant second admin");
        store
            .update_note_acl("note", "owner", None)
            .await
            .expect("remove non-final admin");
    }

    #[tokio::test]
    async fn hides_forbidden_notes_and_falls_back_for_missing_anchors() {
        let store = NotebookStore::connect("sqlite::memory:")
            .await
            .expect("open store");
        insert_note(&store, "visible").await;
        insert_note(&store, "private").await;
        sqlx::query(
            "INSERT INTO note_acl(note_id, user_id, permission) VALUES ('visible', 'reader', 1)",
        )
        .execute(store.pool())
        .await
        .expect("grant reader");
        sqlx::query("INSERT INTO note_anchors(note_id, anchor_id) VALUES ('visible', 'known')")
            .execute(store.pool())
            .await
            .expect("insert anchor");
        let viewer = Viewer {
            user_id: "reader".into(),
            is_root: false,
        };
        let urls = NoteUrlBase::new("https://notebook.example/app/").expect("valid Base URL");

        assert_eq!(
            store
                .resolve_note_reference(&viewer, &urls, "private", None)
                .await
                .expect("resolve private"),
            NoteReferenceResolution::NotFound { detail: None }
        );
        assert_eq!(
            store
                .resolve_note_reference(&viewer, &urls, "visible", Some("missing"))
                .await
                .expect("resolve missing anchor"),
            NoteReferenceResolution::AnchorFallback {
                href: "https://notebook.example/app/note/visible".into(),
                title: "title:visible".into(),
            }
        );
        assert_eq!(
            store
                .resolve_note_reference(&viewer, &urls, "visible", Some("known"))
                .await
                .expect("resolve known anchor"),
            NoteReferenceResolution::Resolved {
                href: "https://notebook.example/app/note/visible#known".into(),
                title: "title:visible".into(),
            }
        );
        assert_eq!(
            NoteUrlBase::new("https://notebook.example")
                .expect("valid root Base URL")
                .note_href("visible", None),
            "https://notebook.example/note/visible"
        );
        assert!(NoteUrlBase::new("http://notebook.example").is_err());
        assert!(NoteUrlBase::new("https://notebook.example/app?debug=true").is_err());

        let root = Viewer {
            user_id: "root".into(),
            is_root: true,
        };
        assert_eq!(
            store
                .resolve_note_reference(&root, &urls, "missing", None)
                .await
                .expect("resolve missing as root"),
            NoteReferenceResolution::NotFound {
                detail: Some(ReferenceFailureDetail::MissingTarget),
            }
        );
    }

    #[tokio::test]
    async fn creates_render_inputs_without_disclosing_forbidden_targets() {
        let store = NotebookStore::connect("sqlite::memory:")
            .await
            .expect("open store");
        let visible = "01800000-0000-7000-8000-000000000001";
        let private = "01800000-0000-7000-8000-000000000002";
        insert_note(&store, visible).await;
        insert_note(&store, private).await;
        sqlx::query("INSERT INTO note_acl(note_id, user_id, permission) VALUES (?, 'reader', 1)")
            .bind(visible)
            .execute(store.pool())
            .await
            .expect("grant reader");
        let analysis = Engine::new(Default::default())
            .analyze(&format!(
                "xref:note:{visible}#missing[] xref:note:{private}[秘匿]\n\n\
                 https://example.com[external]\n"
            ))
            .expect("valid AsciiDoc");
        let viewer = Viewer {
            user_id: "reader".into(),
            is_root: false,
        };
        let urls = NoteUrlBase::new("https://notebook.example/app").expect("valid Base URL");

        let result = store
            .resolve_render_inputs(&analysis, &viewer, &urls)
            .await
            .expect("resolve inputs");
        assert_eq!(result.presentations.len(), 1);
        let presentation = result
            .presentations
            .values()
            .next()
            .expect("fallback presentation");
        let expected_title = format!("title:{visible}");
        assert_eq!(
            presentation.display_label.as_deref(),
            Some(expected_title.as_str())
        );
        assert_eq!(
            presentation.warning.as_ref().map(|warning| warning.code),
            Some("missing-reference-anchor")
        );
        assert_eq!(result.render_inputs.references().len(), 2);
        assert_eq!(
            result.render_inputs.references()[0].outcome,
            ResolutionOutcome::Resolved {
                href: format!("https://notebook.example/app/note/{visible}"),
                notices: vec![ResolutionNotice {
                    kind: ResolutionNoticeKind::Fallback,
                }],
            }
        );
        assert_eq!(
            result.render_inputs.references()[1].outcome,
            ResolutionOutcome::Failed(ResolverFailure {
                kind: ResolutionFailureKind::MissingTarget,
                message: "note reference unavailable".into(),
            })
        );
        let html = render_note_html(&analysis, &result).expect("safe note HTML");
        assert!(
            html.html
                .contains(&format!("https://notebook.example/app/note/{visible}"))
        );
        assert!(html.html.contains("note-reference-warning"));
        assert!(html.html.contains("missing-reference-anchor"));
        assert!(!html.html.contains(private));
        assert!(html.html.contains("target=\"_blank\""));
        assert!(html.html.contains("rel=\"noopener noreferrer\""));
    }

    #[test]
    fn refuses_to_render_unsafe_note_content() {
        let analysis = Engine::new(Default::default())
            .analyze("++++\n<script>alert(1)</script>\n++++\n")
            .expect("recoverable AsciiDoc");
        let resolved = super::ResolvedNoteReferences {
            render_inputs: RenderInputs::default(),
            presentations: Default::default(),
        };

        assert!(render_note_html(&analysis, &resolved).is_err());
    }
}
