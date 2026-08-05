CREATE TABLE notes (
    note_id TEXT PRIMARY KEY NOT NULL,
    creator_issuer TEXT NOT NULL,
    creator_subject TEXT NOT NULL,
    title TEXT NOT NULL,
    source TEXT NOT NULL,
    tags_json TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    deleted_at_ms INTEGER,
    created_via TEXT NOT NULL CHECK (created_via IN ('web', 'rest', 'mcp', 'unknown')),
    review_tracking_known INTEGER NOT NULL CHECK (review_tracking_known IN (0, 1)),
    reviewed_revision INTEGER CHECK (reviewed_revision > 0),
    reviewed_at_ms INTEGER,
    reviewer_issuer TEXT,
    reviewer_subject TEXT,
    CHECK (
        (reviewed_revision IS NULL AND reviewed_at_ms IS NULL
            AND reviewer_issuer IS NULL AND reviewer_subject IS NULL)
        OR
        (review_tracking_known = 1 AND reviewed_revision IS NOT NULL
            AND reviewed_revision <= revision
            AND reviewed_at_ms BETWEEN created_at_ms AND updated_at_ms
            AND reviewer_issuer = creator_issuer
            AND reviewer_subject = creator_subject)
    ),
    UNIQUE (note_id, creator_issuer)
) STRICT;
CREATE INDEX notes_owner_listing_idx
ON notes (creator_issuer, creator_subject, updated_at_ms DESC, note_id)
WHERE deleted_at_ms IS NULL;
CREATE INDEX notes_visible_provenance_idx
ON notes (created_via, review_tracking_known, reviewed_revision, revision, updated_at_ms DESC, note_id)
WHERE deleted_at_ms IS NULL;

CREATE TABLE note_references (
    source_note_id TEXT NOT NULL REFERENCES notes(note_id) ON DELETE CASCADE,
    target_note_id TEXT NOT NULL,
    PRIMARY KEY (source_note_id, target_note_id)
) STRICT, WITHOUT ROWID;
CREATE INDEX note_references_target_idx
ON note_references (target_note_id, source_note_id);

-- 本文が`cite:`で名指したcitation key。関係の図で、ノートと文献を結ぶ線に使う。
-- 参照先の書誌項目が実在するかどうかは保存時に問わない。ライブラリーは後から変わるためである。
CREATE TABLE note_citations (
    source_note_id TEXT NOT NULL REFERENCES notes(note_id) ON DELETE CASCADE,
    citation_key TEXT NOT NULL,
    PRIMARY KEY (source_note_id, citation_key)
) STRICT, WITHOUT ROWID;
CREATE INDEX note_citations_key_idx
ON note_citations (citation_key, source_note_id);

CREATE TABLE bibliography_items (
    item_id TEXT PRIMARY KEY NOT NULL,
    owner_issuer TEXT NOT NULL,
    owner_subject TEXT NOT NULL,
    citation_key TEXT NOT NULL,
    csl_json TEXT NOT NULL CHECK (json_valid(csl_json)),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    UNIQUE (owner_issuer, owner_subject, citation_key),
    UNIQUE (item_id, owner_issuer, owner_subject)
) STRICT;
CREATE INDEX bibliography_items_owner_listing_idx
ON bibliography_items (owner_issuer, owner_subject, updated_at_ms DESC, item_id);

-- 外部サービスへ接続せず、利用者が選んだCSL-JSONファイルの取込元だけを記録する。
CREATE TABLE bibliography_import_sources (
    source_id TEXT PRIMARY KEY NOT NULL,
    owner_issuer TEXT NOT NULL,
    owner_subject TEXT NOT NULL,
    method TEXT NOT NULL CHECK (method = 'csl_json_file'),
    display_name TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    created_at_ms INTEGER NOT NULL,
    last_imported_at_ms INTEGER NOT NULL CHECK (last_imported_at_ms >= created_at_ms),
    UNIQUE (source_id, owner_issuer, owner_subject)
) STRICT;
CREATE INDEX bibliography_import_sources_owner_idx
ON bibliography_import_sources (owner_issuer, owner_subject, last_imported_at_ms DESC, source_id);

-- 取込元内の外部IDと書誌項目を対応させる。owner列を両方の外部キーに含め、異なる
-- 利用者の取込元と書誌項目を結び付けられないようにする。
CREATE TABLE bibliography_import_links (
    source_id TEXT NOT NULL,
    external_item_id TEXT NOT NULL,
    item_id TEXT NOT NULL,
    owner_issuer TEXT NOT NULL,
    owner_subject TEXT NOT NULL,
    imported_digest BLOB NOT NULL CHECK (length(imported_digest) = 32),
    imported_item_revision INTEGER NOT NULL CHECK (imported_item_revision > 0),
    PRIMARY KEY (source_id, external_item_id),
    FOREIGN KEY (source_id, owner_issuer, owner_subject)
        REFERENCES bibliography_import_sources(source_id, owner_issuer, owner_subject)
        ON DELETE CASCADE,
    FOREIGN KEY (item_id, owner_issuer, owner_subject)
        REFERENCES bibliography_items(item_id, owner_issuer, owner_subject)
        ON DELETE CASCADE
) STRICT, WITHOUT ROWID;
CREATE INDEX bibliography_import_links_item_idx
ON bibliography_import_links (item_id, source_id, external_item_id);

-- 数式マクロはノート所有者ごとにまとめて更新し、共有ノートの閲覧者によって表示が変わらない
-- よう、描画時にはノート所有者の設定を読み取る。
CREATE TABLE math_macro_settings (
    owner_issuer TEXT NOT NULL,
    owner_subject TEXT NOT NULL,
    macros_json TEXT NOT NULL CHECK (json_valid(macros_json)),
    revision INTEGER NOT NULL CHECK (revision > 0),
    PRIMARY KEY (owner_issuer, owner_subject)
) STRICT, WITHOUT ROWID;

CREATE TABLE note_acl (
    note_id TEXT NOT NULL,
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    permission TEXT NOT NULL CHECK (permission IN ('read', 'edit')),
    PRIMARY KEY (note_id, issuer, subject),
    FOREIGN KEY (note_id, issuer) REFERENCES notes(note_id, creator_issuer) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;
CREATE INDEX note_acl_identity_idx ON note_acl (issuer, subject, note_id);
CREATE TRIGGER note_acl_reject_owner
BEFORE INSERT ON note_acl
WHEN EXISTS (
    SELECT 1 FROM notes
    WHERE notes.note_id = NEW.note_id
      AND notes.creator_issuer = NEW.issuer
      AND notes.creator_subject = NEW.subject
)
BEGIN
    SELECT RAISE(ABORT, 'note owner cannot be stored in note_acl');
END;

CREATE VIEW note_access AS
SELECT note_id, creator_issuer AS issuer, creator_subject AS subject, 3 AS access_level
FROM notes
UNION ALL
SELECT note_id, issuer, subject,
       CASE permission WHEN 'read' THEN 1 WHEN 'edit' THEN 2 END AS access_level
FROM note_acl;

CREATE TABLE web_sessions (
    session_id_hash BLOB PRIMARY KEY NOT NULL,
    csrf_token_hash BLOB NOT NULL,
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    issued_at_ms INTEGER NOT NULL,
    last_seen_at_ms INTEGER NOT NULL,
    idle_expires_at_ms INTEGER NOT NULL,
    absolute_expires_at_ms INTEGER NOT NULL,
    revoked_at_ms INTEGER
) STRICT;
CREATE INDEX web_sessions_subject_idx
ON web_sessions (issuer, subject)
WHERE revoked_at_ms IS NULL;

CREATE TABLE oidc_login_attempts (
    state_hash BLOB PRIMARY KEY NOT NULL,
    nonce TEXT NOT NULL,
    pkce_verifier TEXT NOT NULL,
    expires_at_ms INTEGER NOT NULL
) STRICT;

CREATE TABLE mcp_clients (
    client_id TEXT PRIMARY KEY NOT NULL,
    display_name TEXT NOT NULL,
    redirect_uris_json TEXT NOT NULL,
    registration_method TEXT NOT NULL CHECK (registration_method IN ('dynamic', 'metadata_document')),
    registered_at_ms INTEGER NOT NULL
) STRICT;

CREATE TABLE mcp_principal_scope_ceilings (
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    scopes TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    updated_at_ms INTEGER NOT NULL,
    PRIMARY KEY (issuer, subject)
) STRICT;

CREATE TABLE mcp_client_scope_ceilings (
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    client_id TEXT NOT NULL REFERENCES mcp_clients(client_id),
    scopes TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    updated_at_ms INTEGER NOT NULL,
    PRIMARY KEY (issuer, subject, client_id)
) STRICT;

CREATE INDEX mcp_client_scope_ceilings_client_id_idx
ON mcp_client_scope_ceilings (client_id);

CREATE TABLE mcp_client_authorizations (
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    client_id TEXT NOT NULL REFERENCES mcp_clients(client_id),
    granted_scopes TEXT NOT NULL,
    authorized_at_ms INTEGER NOT NULL,
    last_used_at_ms INTEGER,
    revoked_at_ms INTEGER,
    PRIMARY KEY (issuer, subject, client_id)
) STRICT;

CREATE INDEX mcp_client_authorizations_client_id_idx
ON mcp_client_authorizations (client_id);

CREATE TABLE mcp_authorization_codes (
    code_hash BLOB PRIMARY KEY NOT NULL,
    client_id TEXT NOT NULL REFERENCES mcp_clients(client_id),
    redirect_uri TEXT NOT NULL,
    redirect_uri_was_supplied INTEGER NOT NULL CHECK (redirect_uri_was_supplied IN (0, 1)),
    resource_uri TEXT NOT NULL,
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    scopes TEXT NOT NULL,
    code_challenge TEXT NOT NULL,
    expires_at_ms INTEGER NOT NULL,
    consumed_at_ms INTEGER,
    token_family_id BLOB CHECK (token_family_id IS NULL OR length(token_family_id) = 32)
) STRICT;

CREATE TABLE mcp_access_tokens (
    token_hash BLOB PRIMARY KEY NOT NULL,
    client_id TEXT NOT NULL REFERENCES mcp_clients(client_id),
    resource_uri TEXT NOT NULL,
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    scopes TEXT NOT NULL,
    expires_at_ms INTEGER NOT NULL,
    revoked_at_ms INTEGER,
    last_used_at_ms INTEGER,
    token_family_id BLOB NOT NULL CHECK (length(token_family_id) = 32)
) STRICT;
CREATE INDEX mcp_access_subject_idx
ON mcp_access_tokens (issuer, subject)
WHERE revoked_at_ms IS NULL;

CREATE TABLE mcp_refresh_tokens (
    token_hash BLOB PRIMARY KEY NOT NULL,
    client_id TEXT NOT NULL REFERENCES mcp_clients(client_id),
    resource_uri TEXT NOT NULL,
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    scopes TEXT NOT NULL,
    expires_at_ms INTEGER NOT NULL,
    rotated_at_ms INTEGER,
    revoked_at_ms INTEGER,
    token_family_id BLOB NOT NULL CHECK (length(token_family_id) = 32)
) STRICT;
CREATE INDEX mcp_refresh_family_idx ON mcp_refresh_tokens (token_family_id);
