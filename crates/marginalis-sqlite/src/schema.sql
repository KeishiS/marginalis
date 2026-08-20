CREATE TABLE principals (
    principal_id INTEGER PRIMARY KEY NOT NULL CHECK (principal_id > 0)
) STRICT;

CREATE TABLE principal_identities (
    identity_id INTEGER PRIMARY KEY NOT NULL CHECK (identity_id > 0),
    principal_id INTEGER NOT NULL REFERENCES principals(principal_id),
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    is_primary INTEGER NOT NULL CHECK (is_primary IN (0, 1)),
    UNIQUE (issuer, subject),
    UNIQUE (identity_id, principal_id)
) STRICT;
CREATE UNIQUE INDEX principal_identities_one_primary_idx
ON principal_identities (principal_id) WHERE is_primary = 1;
CREATE INDEX principal_identities_principal_idx
ON principal_identities (principal_id, identity_id);

CREATE TABLE notes (
    note_id TEXT PRIMARY KEY NOT NULL,
    creator_principal_id INTEGER NOT NULL REFERENCES principals(principal_id),
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
    reviewer_principal_id INTEGER REFERENCES principals(principal_id),
    CHECK (
        (reviewed_revision IS NULL AND reviewed_at_ms IS NULL
            AND reviewer_principal_id IS NULL)
        OR
        (review_tracking_known = 1 AND reviewed_revision IS NOT NULL
            AND reviewed_revision <= revision
            AND reviewed_at_ms BETWEEN created_at_ms AND updated_at_ms
            AND reviewer_principal_id = creator_principal_id)
    ),
    UNIQUE (note_id, creator_principal_id)
) STRICT;
CREATE INDEX notes_owner_listing_idx
ON notes (creator_principal_id, updated_at_ms DESC, note_id)
WHERE deleted_at_ms IS NULL;
CREATE INDEX notes_visible_provenance_idx
ON notes (created_via, review_tracking_known, reviewed_revision, revision, updated_at_ms DESC, note_id)
WHERE deleted_at_ms IS NULL;

CREATE VIEW note_details AS
SELECT notes.*,
       owner_identity.issuer AS creator_issuer,
       owner_identity.subject AS creator_subject,
       reviewer_identity.issuer AS reviewer_issuer,
       reviewer_identity.subject AS reviewer_subject
FROM notes
JOIN principal_identities owner_identity
  ON owner_identity.principal_id = notes.creator_principal_id
 AND owner_identity.is_primary = 1
LEFT JOIN principal_identities reviewer_identity
  ON reviewer_identity.principal_id = notes.reviewer_principal_id
 AND reviewer_identity.is_primary = 1;

-- revisionが確定した直後の完全なノート状態。差分は要求時にsource同士から生成する。
-- 現在のACLだけを履歴の認可へ使い、過去の共有先identityは保存しない。
CREATE TABLE note_revisions (
    note_id TEXT NOT NULL REFERENCES notes(note_id) ON DELETE CASCADE,
    revision INTEGER NOT NULL CHECK (revision > 0),
    changed_at_ms INTEGER NOT NULL,
    changed_by_principal_id INTEGER NOT NULL REFERENCES principals(principal_id),
    change_kind TEXT NOT NULL CHECK (change_kind IN (
        'created', 'content_updated', 'acl_updated', 'reviewed', 'deleted',
        'restored', 'history_restored', 'imported'
    )),
    title TEXT NOT NULL,
    source TEXT NOT NULL,
    tags_json TEXT NOT NULL CHECK (json_valid(tags_json)),
    deleted_at_ms INTEGER,
    review_tracking_known INTEGER NOT NULL CHECK (review_tracking_known IN (0, 1)),
    reviewed_revision INTEGER CHECK (reviewed_revision > 0),
    reviewed_at_ms INTEGER,
    reviewer_principal_id INTEGER REFERENCES principals(principal_id),
    CHECK (
        (reviewed_revision IS NULL AND reviewed_at_ms IS NULL
            AND reviewer_principal_id IS NULL)
        OR
        (review_tracking_known = 1 AND reviewed_revision IS NOT NULL
            AND reviewed_revision <= revision
            AND reviewed_at_ms <= changed_at_ms)
    ),
    PRIMARY KEY (note_id, revision)
) STRICT, WITHOUT ROWID;
CREATE INDEX note_revisions_changed_by_idx
ON note_revisions (changed_by_principal_id, changed_at_ms DESC, note_id, revision);

-- 本文へ組み込む前のuploadも保持する、ノートに所属した変更不可の画像。
CREATE TABLE note_attachments (
    attachment_id TEXT PRIMARY KEY NOT NULL,
    note_id TEXT NOT NULL REFERENCES notes(note_id) ON DELETE CASCADE,
    file_name TEXT NOT NULL CHECK (
        length(file_name) BETWEEN 1 AND 200
        AND instr(file_name, '/') = 0 AND instr(file_name, char(92)) = 0
    ),
    media_type TEXT NOT NULL CHECK (media_type IN (
        'image/png', 'image/jpeg', 'image/webp'
    )),
    byte_length INTEGER NOT NULL CHECK (byte_length BETWEEN 1 AND 8388608),
    sha256 BLOB NOT NULL CHECK (length(sha256) = 32),
    content BLOB NOT NULL CHECK (length(content) = byte_length),
    created_at_ms INTEGER NOT NULL,
    created_by_principal_id INTEGER NOT NULL REFERENCES principals(principal_id),
    UNIQUE (note_id, attachment_id)
) STRICT;
CREATE INDEX note_attachments_note_idx
ON note_attachments (note_id, created_at_ms, attachment_id);
CREATE INDEX note_attachments_creator_idx
ON note_attachments (created_by_principal_id, created_at_ms, attachment_id);

-- revisionが表示する添付集合。現在版から外れても、履歴が残る間はobjectを保持する。
CREATE TABLE note_revision_attachments (
    note_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    attachment_id TEXT NOT NULL,
    PRIMARY KEY (note_id, revision, attachment_id),
    FOREIGN KEY (note_id, revision)
        REFERENCES note_revisions(note_id, revision) ON DELETE CASCADE,
    FOREIGN KEY (note_id, attachment_id)
        REFERENCES note_attachments(note_id, attachment_id) ON DELETE RESTRICT
) STRICT, WITHOUT ROWID;
CREATE INDEX note_revision_attachments_attachment_idx
ON note_revision_attachments (note_id, attachment_id, revision);

CREATE TABLE note_references (
    source_note_id TEXT NOT NULL REFERENCES notes(note_id) ON DELETE CASCADE,
    target_note_id TEXT NOT NULL,
    PRIMARY KEY (source_note_id, target_note_id)
) STRICT, WITHOUT ROWID;
CREATE INDEX note_references_target_idx
ON note_references (target_note_id, source_note_id);

-- 本文が`cite:`で名指したcitation key。グラフビューで、ノートと文献を結ぶ線に使う。
-- 参照先の文献項目が実在するかどうかは保存時に問わない。ライブラリは後から変わるためである。
CREATE TABLE note_citations (
    source_note_id TEXT NOT NULL REFERENCES notes(note_id) ON DELETE CASCADE,
    citation_key TEXT NOT NULL,
    PRIMARY KEY (source_note_id, citation_key)
) STRICT, WITHOUT ROWID;
CREATE INDEX note_citations_key_idx
ON note_citations (citation_key, source_note_id);

CREATE TABLE bibliography_items (
    item_id TEXT PRIMARY KEY NOT NULL,
    owner_principal_id INTEGER NOT NULL REFERENCES principals(principal_id),
    citation_key TEXT NOT NULL,
    csl_json TEXT NOT NULL CHECK (json_valid(csl_json)),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    UNIQUE (owner_principal_id, citation_key),
    UNIQUE (item_id, owner_principal_id)
) STRICT;
CREATE INDEX bibliography_items_owner_listing_idx
ON bibliography_items (owner_principal_id, updated_at_ms DESC, item_id);

CREATE VIEW bibliography_item_details AS
SELECT item.*,
       owner_identity.issuer AS owner_issuer,
       owner_identity.subject AS owner_subject
FROM bibliography_items item
JOIN principal_identities owner_identity
  ON owner_identity.principal_id = item.owner_principal_id
 AND owner_identity.is_primary = 1;

-- 外部サービスへ接続せず、利用者が選んだCSL-JSONファイルの取込元だけを記録する。
CREATE TABLE bibliography_import_sources (
    source_id TEXT PRIMARY KEY NOT NULL,
    owner_principal_id INTEGER NOT NULL REFERENCES principals(principal_id),
    method TEXT NOT NULL CHECK (method = 'csl_json_file'),
    display_name TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    created_at_ms INTEGER NOT NULL,
    last_imported_at_ms INTEGER NOT NULL CHECK (last_imported_at_ms >= created_at_ms),
    UNIQUE (source_id, owner_principal_id)
) STRICT;
CREATE INDEX bibliography_import_sources_owner_idx
ON bibliography_import_sources (owner_principal_id, last_imported_at_ms DESC, source_id);

CREATE VIEW bibliography_import_source_details AS
SELECT source.*,
       owner_identity.issuer AS owner_issuer,
       owner_identity.subject AS owner_subject
FROM bibliography_import_sources source
JOIN principal_identities owner_identity
  ON owner_identity.principal_id = source.owner_principal_id
 AND owner_identity.is_primary = 1;

-- 取込元内の外部IDと文献項目を対応させる。owner列を両方の外部キーに含め、異なる
-- 利用者の取込元と文献項目を結び付けられないようにする。
CREATE TABLE bibliography_import_links (
    source_id TEXT NOT NULL,
    external_item_id TEXT NOT NULL,
    item_id TEXT NOT NULL,
    owner_principal_id INTEGER NOT NULL,
    imported_digest BLOB NOT NULL CHECK (length(imported_digest) = 32),
    imported_item_revision INTEGER NOT NULL CHECK (imported_item_revision > 0),
    PRIMARY KEY (source_id, external_item_id),
    FOREIGN KEY (source_id, owner_principal_id)
        REFERENCES bibliography_import_sources(source_id, owner_principal_id)
        ON DELETE CASCADE,
    FOREIGN KEY (item_id, owner_principal_id)
        REFERENCES bibliography_items(item_id, owner_principal_id)
        ON DELETE CASCADE
) STRICT, WITHOUT ROWID;
CREATE INDEX bibliography_import_links_item_idx
ON bibliography_import_links (item_id, source_id, external_item_id);

-- 数式マクロはノート所有者ごとにまとめて更新し、共有ノートの閲覧者によって表示が変わらない
-- よう、描画時にはノート所有者の設定を読み取る。
CREATE TABLE math_macro_settings (
    owner_principal_id INTEGER PRIMARY KEY NOT NULL REFERENCES principals(principal_id),
    macros_json TEXT NOT NULL CHECK (json_valid(macros_json)),
    revision INTEGER NOT NULL CHECK (revision > 0)
) STRICT;

CREATE TABLE note_acl (
    note_id TEXT NOT NULL,
    principal_id INTEGER NOT NULL REFERENCES principals(principal_id),
    permission TEXT NOT NULL CHECK (permission IN ('read', 'edit')),
    PRIMARY KEY (note_id, principal_id),
    FOREIGN KEY (note_id) REFERENCES notes(note_id) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;
CREATE INDEX note_acl_principal_idx ON note_acl (principal_id, note_id);
CREATE TRIGGER note_acl_reject_owner
BEFORE INSERT ON note_acl
WHEN EXISTS (
    SELECT 1 FROM notes
    WHERE notes.note_id = NEW.note_id
      AND notes.creator_principal_id = NEW.principal_id
)
BEGIN
    SELECT RAISE(ABORT, 'note owner cannot be stored in note_acl');
END;

CREATE VIEW note_access AS
SELECT note_id, creator_principal_id AS principal_id, 3 AS access_level
FROM notes
UNION ALL
SELECT note_id, principal_id,
       CASE permission WHEN 'read' THEN 1 WHEN 'edit' THEN 2 END AS access_level
FROM note_acl;

-- 業務データの確定した変更。同期とWebhook配送はこの一つの記録から派生する。
-- 本文・CSL-JSON・identityは保持せず、event_idはWebhookの再送でも変わらない。
CREATE TABLE domain_changes (
    change_sequence INTEGER PRIMARY KEY AUTOINCREMENT CHECK (change_sequence > 0),
    event_id TEXT NOT NULL UNIQUE,
    owner_principal_id INTEGER NOT NULL REFERENCES principals(principal_id),
    affected_principal_id INTEGER REFERENCES principals(principal_id),
    change_kind TEXT NOT NULL CHECK (change_kind IN (
        'note.created', 'note.updated', 'note.deleted', 'note.restored',
        'note.state_changed', 'note.access_granted', 'note.access_revoked',
        'bibliography_item.created', 'bibliography_item.updated',
        'bibliography_item.deleted'
    )),
    target_id TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    occurred_at_ms INTEGER NOT NULL,
    CHECK (
        (change_kind IN ('note.access_granted', 'note.access_revoked')
            AND affected_principal_id IS NOT NULL)
        OR (change_kind NOT IN ('note.access_granted', 'note.access_revoked')
            AND affected_principal_id IS NULL)
    )
) STRICT;
CREATE INDEX domain_changes_owner_sequence_idx
ON domain_changes (owner_principal_id, change_sequence);

-- 検索用投影へ渡す、利用者・ノートごとの最新状態。本文は保持せず、読取時に
-- domain_changesと現在の可視ノートへ結合する。
CREATE TABLE note_sync_state (
    singleton INTEGER PRIMARY KEY NOT NULL CHECK (singleton = 1),
    latest_note_change_sequence INTEGER NOT NULL CHECK (latest_note_change_sequence >= 0)
) STRICT;
INSERT INTO note_sync_state (singleton, latest_note_change_sequence) VALUES (1, 0);

CREATE TABLE note_sync_projection (
    change_sequence INTEGER NOT NULL
        REFERENCES domain_changes(change_sequence) ON DELETE CASCADE,
    principal_id INTEGER NOT NULL REFERENCES principals(principal_id),
    note_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('upsert', 'remove')),
    reason TEXT CHECK (
        (kind = 'upsert' AND reason IS NULL)
        OR (kind = 'remove' AND reason IN ('deleted', 'access_revoked'))
    ),
    PRIMARY KEY (principal_id, note_id)
) STRICT;
CREATE INDEX note_sync_projection_principal_sequence_idx
ON note_sync_projection (principal_id, change_sequence);

CREATE TABLE note_sync_cursors (
    cursor_hash BLOB PRIMARY KEY NOT NULL CHECK (length(cursor_hash) = 32),
    principal_id INTEGER NOT NULL REFERENCES principals(principal_id),
    phase TEXT NOT NULL CHECK (phase IN ('snapshot', 'changes')),
    after_note_id TEXT,
    after_sequence INTEGER NOT NULL CHECK (after_sequence >= 0),
    high_watermark INTEGER NOT NULL CHECK (high_watermark >= 0),
    expires_at_ms INTEGER NOT NULL,
    CHECK ((phase = 'snapshot') OR after_note_id IS NULL)
) STRICT;
CREATE INDEX note_sync_cursors_expiry_idx ON note_sync_cursors (expires_at_ms);

-- 業務表の変更は、用途別の表へ重複して書かずdomain_changesへ一度だけ記録する。
CREATE TRIGGER domain_change_after_note_insert
AFTER INSERT ON notes
BEGIN
    INSERT INTO domain_changes (
        event_id, owner_principal_id, affected_principal_id, change_kind,
        target_id, revision, occurred_at_ms
    ) VALUES (lower(hex(randomblob(16))), NEW.creator_principal_id, NULL,
              'note.created', NEW.note_id, NEW.revision, NEW.updated_at_ms);
END;

CREATE TRIGGER domain_change_after_note_update
AFTER UPDATE ON notes
BEGIN
    INSERT INTO domain_changes (
        event_id, owner_principal_id, affected_principal_id, change_kind,
        target_id, revision, occurred_at_ms
    ) VALUES (
        lower(hex(randomblob(16))), NEW.creator_principal_id, NULL,
        CASE
            WHEN OLD.deleted_at_ms IS NULL AND NEW.deleted_at_ms IS NOT NULL
                THEN 'note.deleted'
            WHEN OLD.deleted_at_ms IS NOT NULL AND NEW.deleted_at_ms IS NULL
                THEN 'note.restored'
            WHEN NEW.source IS NOT OLD.source THEN 'note.updated'
            ELSE 'note.state_changed'
        END,
        NEW.note_id, NEW.revision, NEW.updated_at_ms
    );
END;

CREATE TRIGGER domain_change_after_acl_insert
AFTER INSERT ON note_acl
BEGIN
    INSERT INTO domain_changes (
        event_id, owner_principal_id, affected_principal_id, change_kind,
        target_id, revision, occurred_at_ms
    ) SELECT lower(hex(randomblob(16))), notes.creator_principal_id,
             NEW.principal_id, 'note.access_granted', NEW.note_id,
             notes.revision, notes.updated_at_ms
        FROM notes WHERE notes.note_id = NEW.note_id;
END;

CREATE TRIGGER domain_change_after_acl_delete
AFTER DELETE ON note_acl
BEGIN
    INSERT INTO domain_changes (
        event_id, owner_principal_id, affected_principal_id, change_kind,
        target_id, revision, occurred_at_ms
    ) SELECT lower(hex(randomblob(16))), notes.creator_principal_id,
             OLD.principal_id, 'note.access_revoked', OLD.note_id,
             notes.revision, notes.updated_at_ms
        FROM notes WHERE notes.note_id = OLD.note_id;
END;

-- ノート自体の変更は、その時点で閲覧できる全利用者の最新同期状態へ反映する。
CREATE TRIGGER note_sync_after_note_change_insert
AFTER INSERT ON domain_changes
WHEN NEW.change_kind IN (
    'note.created', 'note.updated', 'note.deleted', 'note.restored', 'note.state_changed'
)
BEGIN
    INSERT INTO note_sync_projection (
        change_sequence, principal_id, note_id, kind, reason
    ) SELECT NEW.change_sequence, access.principal_id, NEW.target_id,
             CASE WHEN NEW.change_kind = 'note.deleted' THEN 'remove' ELSE 'upsert' END,
             CASE WHEN NEW.change_kind = 'note.deleted' THEN 'deleted' ELSE NULL END
        FROM note_access access WHERE access.note_id = NEW.target_id
    ON CONFLICT (principal_id, note_id) DO UPDATE SET
        change_sequence = excluded.change_sequence, kind = excluded.kind,
        reason = excluded.reason;
    UPDATE note_sync_state SET latest_note_change_sequence = NEW.change_sequence
    WHERE singleton = 1;
END;

-- ACL変更は対象利用者の最新同期状態だけを更新する。
CREATE TRIGGER note_sync_after_access_change_insert
AFTER INSERT ON domain_changes
WHEN NEW.change_kind IN ('note.access_granted', 'note.access_revoked')
BEGIN
    INSERT INTO note_sync_projection (
        change_sequence, principal_id, note_id, kind, reason
    ) VALUES (
        NEW.change_sequence, NEW.affected_principal_id, NEW.target_id,
        CASE WHEN NEW.change_kind = 'note.access_granted' THEN 'upsert' ELSE 'remove' END,
        CASE WHEN NEW.change_kind = 'note.access_revoked' THEN 'access_revoked' ELSE NULL END
    )
    ON CONFLICT (principal_id, note_id) DO UPDATE SET
        change_sequence = excluded.change_sequence, kind = excluded.kind,
        reason = excluded.reason;
    UPDATE note_sync_state SET latest_note_change_sequence = NEW.change_sequence
    WHERE singleton = 1;
END;

CREATE TABLE web_sessions (
    session_id_hash BLOB PRIMARY KEY NOT NULL,
    csrf_token_hash BLOB NOT NULL,
    principal_id INTEGER NOT NULL REFERENCES principals(principal_id),
    authenticated_identity_id INTEGER NOT NULL,
    issued_at_ms INTEGER NOT NULL,
    last_seen_at_ms INTEGER NOT NULL,
    idle_expires_at_ms INTEGER NOT NULL,
    absolute_expires_at_ms INTEGER NOT NULL,
    revoked_at_ms INTEGER,
    FOREIGN KEY (authenticated_identity_id, principal_id)
        REFERENCES principal_identities(identity_id, principal_id)
) STRICT;
CREATE INDEX web_sessions_subject_idx
ON web_sessions (principal_id)
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
    principal_id INTEGER PRIMARY KEY NOT NULL REFERENCES principals(principal_id),
    scopes TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    updated_at_ms INTEGER NOT NULL
) STRICT;

CREATE TABLE mcp_client_scope_ceilings (
    principal_id INTEGER NOT NULL REFERENCES principals(principal_id),
    client_id TEXT NOT NULL REFERENCES mcp_clients(client_id),
    scopes TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    updated_at_ms INTEGER NOT NULL,
    PRIMARY KEY (principal_id, client_id)
) STRICT;

CREATE INDEX mcp_client_scope_ceilings_client_id_idx
ON mcp_client_scope_ceilings (client_id);

CREATE TABLE mcp_client_authorizations (
    principal_id INTEGER NOT NULL REFERENCES principals(principal_id),
    client_id TEXT NOT NULL REFERENCES mcp_clients(client_id),
    granted_scopes TEXT NOT NULL,
    authorized_at_ms INTEGER NOT NULL,
    last_used_at_ms INTEGER,
    revoked_at_ms INTEGER,
    PRIMARY KEY (principal_id, client_id)
) STRICT;

CREATE INDEX mcp_client_authorizations_client_id_idx
ON mcp_client_authorizations (client_id);

CREATE TABLE mcp_authorization_codes (
    code_hash BLOB PRIMARY KEY NOT NULL,
    client_id TEXT NOT NULL REFERENCES mcp_clients(client_id),
    redirect_uri TEXT NOT NULL,
    redirect_uri_was_supplied INTEGER NOT NULL CHECK (redirect_uri_was_supplied IN (0, 1)),
    resource_uri TEXT NOT NULL,
    principal_id INTEGER NOT NULL REFERENCES principals(principal_id),
    authenticated_identity_id INTEGER NOT NULL,
    scopes TEXT NOT NULL,
    code_challenge TEXT NOT NULL,
    expires_at_ms INTEGER NOT NULL,
    consumed_at_ms INTEGER,
    token_family_id BLOB CHECK (token_family_id IS NULL OR length(token_family_id) = 32),
    FOREIGN KEY (authenticated_identity_id, principal_id)
        REFERENCES principal_identities(identity_id, principal_id)
) STRICT;

CREATE TABLE mcp_access_tokens (
    token_hash BLOB PRIMARY KEY NOT NULL,
    client_id TEXT NOT NULL REFERENCES mcp_clients(client_id),
    resource_uri TEXT NOT NULL,
    principal_id INTEGER NOT NULL REFERENCES principals(principal_id),
    authenticated_identity_id INTEGER NOT NULL,
    scopes TEXT NOT NULL,
    expires_at_ms INTEGER NOT NULL,
    revoked_at_ms INTEGER,
    last_used_at_ms INTEGER,
    token_family_id BLOB NOT NULL CHECK (length(token_family_id) = 32),
    FOREIGN KEY (authenticated_identity_id, principal_id)
        REFERENCES principal_identities(identity_id, principal_id)
) STRICT;
CREATE INDEX mcp_access_subject_idx
ON mcp_access_tokens (principal_id)
WHERE revoked_at_ms IS NULL;

CREATE TABLE mcp_refresh_tokens (
    token_hash BLOB PRIMARY KEY NOT NULL,
    client_id TEXT NOT NULL REFERENCES mcp_clients(client_id),
    resource_uri TEXT NOT NULL,
    principal_id INTEGER NOT NULL REFERENCES principals(principal_id),
    authenticated_identity_id INTEGER NOT NULL,
    scopes TEXT NOT NULL,
    expires_at_ms INTEGER NOT NULL,
    rotated_at_ms INTEGER,
    revoked_at_ms INTEGER,
    token_family_id BLOB NOT NULL CHECK (length(token_family_id) = 32),
    FOREIGN KEY (authenticated_identity_id, principal_id)
        REFERENCES principal_identities(identity_id, principal_id)
) STRICT;
CREATE INDEX mcp_refresh_family_idx ON mcp_refresh_tokens (token_family_id);

-- Webhookの送信先。secretはHMAC署名に平文が必要なため平文で保存する(ADR 0014)。
-- archiveへは含めない。challenge応答を確認するまでstateはpending_challengeのまま。
CREATE TABLE webhook_subscriptions (
    subscription_id TEXT PRIMARY KEY NOT NULL,
    owner_principal_id INTEGER NOT NULL REFERENCES principals(principal_id),
    url TEXT NOT NULL,
    secret TEXT NOT NULL,
    event_kinds_json TEXT NOT NULL CHECK (json_valid(event_kinds_json)),
    state TEXT NOT NULL CHECK (state IN ('pending_challenge', 'active', 'disabled')),
    disabled_reason TEXT CHECK (
        (state = 'disabled' AND disabled_reason IN
            ('delivery_exhausted', 'destination_rejected', 'owner_disabled'))
        OR (state != 'disabled' AND disabled_reason IS NULL)
    ),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0)
) STRICT;
CREATE INDEX webhook_subscriptions_owner_idx
ON webhook_subscriptions (owner_principal_id);

-- subscriptionごとの配送状態。eventの発生時に有効な送信先だけへ展開する。
-- 同じ送信先へはevent_sequence順に配送し、失敗中は後続を保留する。
CREATE TABLE webhook_deliveries (
    subscription_id TEXT NOT NULL
        REFERENCES webhook_subscriptions (subscription_id) ON DELETE CASCADE,
    event_sequence INTEGER NOT NULL
        REFERENCES domain_changes (change_sequence) ON DELETE CASCADE,
    state TEXT NOT NULL CHECK (state IN ('pending', 'delivered', 'discarded')),
    attempt_count INTEGER NOT NULL CHECK (attempt_count >= 0),
    next_attempt_at_ms INTEGER NOT NULL,
    lease_expires_at_ms INTEGER,
    last_failure TEXT CHECK (last_failure IN
        ('non_success_status', 'connect_failed', 'timed_out',
         'destination_rejected')),
    last_attempted_at_ms INTEGER,
    PRIMARY KEY (subscription_id, event_sequence)
) STRICT;
CREATE INDEX webhook_deliveries_due_idx
ON webhook_deliveries (state, next_attempt_at_ms);

-- 有効な送信先のうちevent種別を購読しているものへ、配送行を同じtransactionで展開する。
CREATE TRIGGER webhook_fan_out_after_change_insert
AFTER INSERT ON domain_changes
WHEN NEW.change_kind IN (
    'note.created', 'note.updated', 'note.deleted', 'note.restored',
    'bibliography_item.created', 'bibliography_item.updated',
    'bibliography_item.deleted'
)
BEGIN
    INSERT INTO webhook_deliveries (
        subscription_id, event_sequence, state, attempt_count,
        next_attempt_at_ms, lease_expires_at_ms, last_failure, last_attempted_at_ms
    )
    SELECT s.subscription_id, NEW.change_sequence, 'pending', 0,
           NEW.occurred_at_ms, NULL, NULL, NULL
    FROM webhook_subscriptions s
    WHERE s.owner_principal_id = NEW.owner_principal_id
      AND s.state = 'active'
      AND EXISTS (
          SELECT 1 FROM json_each(s.event_kinds_json)
          WHERE json_each.value = NEW.change_kind
      );
END;

CREATE TRIGGER domain_change_after_bibliography_insert
AFTER INSERT ON bibliography_items
BEGIN
    INSERT INTO domain_changes (
        event_id, owner_principal_id, affected_principal_id, change_kind,
        target_id, revision, occurred_at_ms
    ) VALUES (lower(hex(randomblob(16))), NEW.owner_principal_id, NULL,
              'bibliography_item.created', NEW.item_id, NEW.revision, NEW.updated_at_ms);
END;

-- 一括取込でも、実際に内容が変わりrevisionが進んだ項目だけを対象にする。
CREATE TRIGGER domain_change_after_bibliography_update
AFTER UPDATE ON bibliography_items
WHEN NEW.revision != OLD.revision
BEGIN
    INSERT INTO domain_changes (
        event_id, owner_principal_id, affected_principal_id, change_kind,
        target_id, revision, occurred_at_ms
    ) VALUES (lower(hex(randomblob(16))), NEW.owner_principal_id, NULL,
              'bibliography_item.updated', NEW.item_id, NEW.revision, NEW.updated_at_ms);
END;

CREATE TRIGGER domain_change_after_bibliography_delete
AFTER DELETE ON bibliography_items
BEGIN
    INSERT INTO domain_changes (
        event_id, owner_principal_id, affected_principal_id, change_kind,
        target_id, revision, occurred_at_ms
    ) VALUES (lower(hex(randomblob(16))), OLD.owner_principal_id, NULL,
              'bibliography_item.deleted', OLD.item_id, OLD.revision, OLD.updated_at_ms);
END;
