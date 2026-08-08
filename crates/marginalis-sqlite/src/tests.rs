use std::str::FromStr;

use marginalis_application::{
    BibliographyImportRepository, BibliographyRepository, MathMacro, MathMacroRepository,
    McpAuthorizationCodeExchange, McpAuthorizationGrant, McpClientRegistrationMethod,
    McpOAuthClient, McpRefreshTokenRotation, McpRefreshTokenRotationOutcome,
    McpResolvedRedirectUri, McpTimestamp, OidcLoginAttempt, OidcLoginAttemptStore, RestorePlan,
    StorageError,
};
use marginalis_domain::{
    Actor, BibliographyItem, BibliographyItemId, EntityId, Identity, Note, NoteAccess,
    NoteAclEntry, NoteCreationSource, NoteDraft, NoteId, NotePermission, NoteRestore,
    NoteReviewTracking, Revision, SOFT_DELETE_RETENTION_MS, UnixMillis, ValidatedCslJson,
    WebSession,
};

use super::*;

/// 試験全体で使う既定のissuer。issuerを変えたい試験だけが`actor`へ別の値を渡す。
const ISSUER: &str = "https://id.example.test";

/// schema初期化済みのin-memory databaseへ接続する。
async fn database() -> SqliteDatabase {
    SqliteDatabase::connect("sqlite::memory:")
        .await
        .expect("in-memory database with initialized schema")
}

fn actor(issuer: &str, subject: &str) -> Actor {
    Actor::try_new(issuer.into(), subject.into()).expect("valid test actor")
}

/// 既定issuerの利用者。
fn user(subject: &str) -> Actor {
    actor(ISSUER, subject)
}

/// 既定issuerのIdentity。所有者やACLの指定に使う。
fn identity(subject: &str) -> Identity {
    Identity::new(ISSUER.into(), subject.into()).expect("valid test identity")
}

fn principal(issuer: &str, subject: &str) -> marginalis_application::McpPrincipal {
    let actor = actor(issuer, subject);
    marginalis_application::McpPrincipal::new(actor.issuer().into(), actor.subject().into())
}

fn revision(value: i64) -> Revision {
    Revision::new(value).expect("positive test revision")
}

fn note_id(hex: &str) -> NoteId {
    NoteId::new(EntityId::from_str(hex).expect("v7 note ID"))
}

fn draft(title: &str, source: &str, tags: &[&str]) -> NoteDraft {
    NoteDraft {
        title: title.into(),
        source: source.into(),
        tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
    }
}

/// 既定issuerの対象者に対するACL entry。
fn acl_entry(subject: &str, permission: NotePermission) -> NoteAclEntry {
    NoteAclEntry::new(identity(subject), permission)
}

fn bibliography_item(id_hex: &str, owner: &Actor, key: &str, csl_json: &str) -> BibliographyItem {
    BibliographyItem::create(
        BibliographyItemId::new(EntityId::from_str(id_hex).expect("v7 bibliography item ID")),
        owner.identity(),
        validated_csl_json(key, csl_json),
        UnixMillis::new(100),
    )
}

fn validated_csl_json(key: &str, csl_json: &str) -> ValidatedCslJson {
    ValidatedCslJson::from_encoded(key, csl_json).expect("valid CSL-JSON")
}

/// `Note::restore`の定型を省く試験用のseed。
///
/// 既定は時刻100、初期revision、未削除、作成経路と人手確認記録は不明、本文は
/// `= <title>`とする。異なる値が必要な試験だけがbuilder風のmethodで上書きする。
struct NoteSeed {
    note_id: NoteId,
    owner: Identity,
    title: String,
    source: String,
    tags: Vec<String>,
}

fn note_seed(id_hex: &str, owner_subject: &str, title: &str) -> NoteSeed {
    NoteSeed {
        note_id: note_id(id_hex),
        owner: identity(owner_subject),
        title: title.into(),
        source: format!("= {title}\n\n本文"),
        tags: Vec::new(),
    }
}

impl NoteSeed {
    fn source(mut self, source: impl Into<String>) -> Self {
        self.source = source.into();
        self
    }

    fn tags(mut self, tags: &[&str]) -> Self {
        self.tags = tags.iter().map(|tag| (*tag).to_owned()).collect();
        self
    }

    fn build(self) -> Note {
        Note::restore(NoteRestore {
            note_id: self.note_id,
            owner: self.owner,
            draft: NoteDraft {
                title: self.title,
                source: self.source,
                tags: self.tags,
            },
            created_at: UnixMillis::new(100),
            updated_at: UnixMillis::new(100),
            revision: Revision::INITIAL,
            deleted_at: None,
            created_via: NoteCreationSource::Unknown,
            review: NoteReviewTracking::Unknown,
        })
        .expect("consistent note")
    }
}

/// 検証専用の読み出し・登録操作。
///
/// 本番codeへ`#[cfg(test)]`を混ぜないため、試験moduleのinherent実装として追加する。
impl SqliteDatabase {
    /// 認可を通さずにノート正本の行を読み出す。
    async fn note(
        &self,
        note_id: NoteId,
        include_deleted: bool,
    ) -> Result<Option<Note>, SqliteStoreError> {
        let row = if include_deleted {
            sqlx::query("SELECT * FROM notes WHERE note_id = ?")
                .bind(note_id.to_string())
                .fetch_optional(&self.pool)
                .await
        } else {
            sqlx::query("SELECT * FROM notes WHERE note_id = ? AND deleted_at_ms IS NULL")
                .bind(note_id.to_string())
                .fetch_optional(&self.pool)
                .await
        }
        .map_err(crate::database_error)?;
        row.map(crate::notes::note_from_row).transpose()
    }

    /// clientを動的登録として直接登録する。
    async fn upsert_mcp_client(
        &self,
        client: &McpOAuthClient,
        registered_at: McpTimestamp,
    ) -> Result<(), SqliteStoreError> {
        crate::mcp::upsert_client(
            &self.pool,
            client,
            McpClientRegistrationMethod::Dynamic,
            registered_at,
        )
        .await
    }

    /// 登録済みclientを登録方法を落として読み出す。
    async fn mcp_client(
        &self,
        client_id: &str,
    ) -> Result<Option<McpOAuthClient>, SqliteStoreError> {
        Ok(self
            .registered_mcp_client(client_id)
            .await?
            .map(|registered| registered.client))
    }
}

mod schema;

mod notes;

mod bibliography;

mod bibliography_import;

mod math_macros;

mod sessions;

mod oauth;
