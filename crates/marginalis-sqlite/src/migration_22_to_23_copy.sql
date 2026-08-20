-- schema 22に現れるidentityを一人ずつ独立したprincipalへ移す。同じ人物かどうかは
-- issuerとsubjectの完全一致だけで判断し、推測による統合は行わない。
CREATE TEMP TABLE migration22_identity_map (
    principal_id INTEGER PRIMARY KEY NOT NULL,
    identity_id INTEGER NOT NULL UNIQUE,
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    UNIQUE (issuer, subject)
) STRICT;

INSERT INTO migration22_identity_map (principal_id, identity_id, issuer, subject)
SELECT position, position, issuer, subject
FROM (
    SELECT row_number() OVER (ORDER BY issuer, subject) AS position, issuer, subject
    FROM (
        SELECT creator_issuer AS issuer, creator_subject AS subject FROM migration22_notes
        UNION SELECT reviewer_issuer, reviewer_subject FROM migration22_notes
              WHERE reviewer_issuer IS NOT NULL AND reviewer_subject IS NOT NULL
        UNION SELECT owner_issuer, owner_subject FROM migration22_bibliography_items
        UNION SELECT owner_issuer, owner_subject FROM migration22_bibliography_import_sources
        UNION SELECT owner_issuer, owner_subject FROM migration22_bibliography_import_links
        UNION SELECT owner_issuer, owner_subject FROM migration22_math_macro_settings
        UNION SELECT issuer, subject FROM migration22_note_acl
        UNION SELECT issuer, subject FROM migration22_note_sync_changes
        UNION SELECT issuer, subject FROM migration22_note_sync_cursors
        UNION SELECT issuer, subject FROM migration22_web_sessions
        UNION SELECT issuer, subject FROM migration22_mcp_principal_scope_ceilings
        UNION SELECT issuer, subject FROM migration22_mcp_client_scope_ceilings
        UNION SELECT issuer, subject FROM migration22_mcp_client_authorizations
        UNION SELECT issuer, subject FROM migration22_mcp_authorization_codes
        UNION SELECT issuer, subject FROM migration22_mcp_access_tokens
        UNION SELECT issuer, subject FROM migration22_mcp_refresh_tokens
        UNION SELECT owner_issuer, owner_subject FROM migration22_webhook_subscriptions
        UNION SELECT owner_issuer, owner_subject FROM migration22_webhook_outbox_events
    ) identities
);

INSERT INTO principals (principal_id)
SELECT principal_id FROM migration22_identity_map ORDER BY principal_id;
INSERT INTO principal_identities (
    identity_id, principal_id, issuer, subject, is_primary
)
SELECT identity_id, principal_id, issuer, subject, 1
FROM migration22_identity_map ORDER BY principal_id;

INSERT INTO oidc_login_attempts
SELECT * FROM migration22_oidc_login_attempts;
INSERT INTO mcp_clients
SELECT * FROM migration22_mcp_clients;
UPDATE note_sync_state
SET next_sequence = (SELECT next_sequence FROM migration22_note_sync_state WHERE singleton = 1)
WHERE singleton = 1;

INSERT INTO notes (
    note_id, creator_principal_id, title, source, tags_json,
    created_at_ms, updated_at_ms, revision, deleted_at_ms, created_via,
    review_tracking_known, reviewed_revision, reviewed_at_ms, reviewer_principal_id
)
SELECT n.note_id, creator.principal_id, n.title, n.source, n.tags_json,
       n.created_at_ms, n.updated_at_ms, n.revision, n.deleted_at_ms, n.created_via,
       n.review_tracking_known, n.reviewed_revision, n.reviewed_at_ms,
       reviewer.principal_id
FROM migration22_notes n
JOIN migration22_identity_map creator
  ON creator.issuer = n.creator_issuer AND creator.subject = n.creator_subject
LEFT JOIN migration22_identity_map reviewer
  ON reviewer.issuer = n.reviewer_issuer AND reviewer.subject = n.reviewer_subject;

INSERT INTO note_references SELECT * FROM migration22_note_references;
INSERT INTO note_citations SELECT * FROM migration22_note_citations;

INSERT INTO bibliography_items (
    item_id, owner_principal_id, citation_key, csl_json,
    created_at_ms, updated_at_ms, revision
)
SELECT item.item_id, owner.principal_id, item.citation_key, item.csl_json,
       item.created_at_ms, item.updated_at_ms, item.revision
FROM migration22_bibliography_items item
JOIN migration22_identity_map owner
  ON owner.issuer = item.owner_issuer AND owner.subject = item.owner_subject;

INSERT INTO bibliography_import_sources (
    source_id, owner_principal_id, method, display_name, revision,
    created_at_ms, last_imported_at_ms
)
SELECT source.source_id, owner.principal_id, source.method, source.display_name,
       source.revision, source.created_at_ms, source.last_imported_at_ms
FROM migration22_bibliography_import_sources source
JOIN migration22_identity_map owner
  ON owner.issuer = source.owner_issuer AND owner.subject = source.owner_subject;

INSERT INTO bibliography_import_links (
    source_id, external_item_id, item_id, owner_principal_id,
    imported_digest, imported_item_revision
)
SELECT link.source_id, link.external_item_id, link.item_id, owner.principal_id,
       link.imported_digest, link.imported_item_revision
FROM migration22_bibliography_import_links link
JOIN migration22_identity_map owner
  ON owner.issuer = link.owner_issuer AND owner.subject = link.owner_subject;

INSERT INTO math_macro_settings (owner_principal_id, macros_json, revision)
SELECT owner.principal_id, settings.macros_json, settings.revision
FROM migration22_math_macro_settings settings
JOIN migration22_identity_map owner
  ON owner.issuer = settings.owner_issuer AND owner.subject = settings.owner_subject;

INSERT INTO note_acl (note_id, principal_id, permission)
SELECT acl.note_id, principal.principal_id, acl.permission
FROM migration22_note_acl acl
JOIN migration22_identity_map principal
  ON principal.issuer = acl.issuer AND principal.subject = acl.subject;

INSERT INTO web_sessions (
    session_id_hash, csrf_token_hash, principal_id, authenticated_identity_id,
    issued_at_ms, last_seen_at_ms, idle_expires_at_ms, absolute_expires_at_ms, revoked_at_ms
)
SELECT session.session_id_hash, session.csrf_token_hash,
       principal.principal_id, principal.identity_id,
       session.issued_at_ms, session.last_seen_at_ms, session.idle_expires_at_ms,
       session.absolute_expires_at_ms, session.revoked_at_ms
FROM migration22_web_sessions session
JOIN migration22_identity_map principal
  ON principal.issuer = session.issuer AND principal.subject = session.subject;

INSERT INTO mcp_principal_scope_ceilings (
    principal_id, scopes, revision, updated_at_ms
)
SELECT principal.principal_id, ceiling.scopes, ceiling.revision, ceiling.updated_at_ms
FROM migration22_mcp_principal_scope_ceilings ceiling
JOIN migration22_identity_map principal
  ON principal.issuer = ceiling.issuer AND principal.subject = ceiling.subject;

INSERT INTO mcp_client_scope_ceilings (
    principal_id, client_id, scopes, revision, updated_at_ms
)
SELECT principal.principal_id, ceiling.client_id, ceiling.scopes,
       ceiling.revision, ceiling.updated_at_ms
FROM migration22_mcp_client_scope_ceilings ceiling
JOIN migration22_identity_map principal
  ON principal.issuer = ceiling.issuer AND principal.subject = ceiling.subject;

INSERT INTO mcp_client_authorizations (
    principal_id, client_id, granted_scopes, authorized_at_ms, last_used_at_ms, revoked_at_ms
)
SELECT principal.principal_id, authorization.client_id, authorization.granted_scopes,
       authorization.authorized_at_ms, authorization.last_used_at_ms,
       authorization.revoked_at_ms
FROM migration22_mcp_client_authorizations authorization
JOIN migration22_identity_map principal
  ON principal.issuer = authorization.issuer
 AND principal.subject = authorization.subject;

INSERT INTO mcp_authorization_codes (
    code_hash, client_id, redirect_uri, redirect_uri_was_supplied, resource_uri,
    principal_id, authenticated_identity_id, scopes, code_challenge,
    expires_at_ms, consumed_at_ms, token_family_id
)
SELECT code.code_hash, code.client_id, code.redirect_uri, code.redirect_uri_was_supplied,
       code.resource_uri, principal.principal_id, principal.identity_id, code.scopes,
       code.code_challenge, code.expires_at_ms, code.consumed_at_ms, code.token_family_id
FROM migration22_mcp_authorization_codes code
JOIN migration22_identity_map principal
  ON principal.issuer = code.issuer AND principal.subject = code.subject;

INSERT INTO mcp_access_tokens (
    token_hash, client_id, resource_uri, principal_id, authenticated_identity_id,
    scopes, expires_at_ms, revoked_at_ms, last_used_at_ms, token_family_id
)
SELECT token.token_hash, token.client_id, token.resource_uri,
       principal.principal_id, principal.identity_id, token.scopes,
       token.expires_at_ms, token.revoked_at_ms, token.last_used_at_ms,
       token.token_family_id
FROM migration22_mcp_access_tokens token
JOIN migration22_identity_map principal
  ON principal.issuer = token.issuer AND principal.subject = token.subject;

INSERT INTO mcp_refresh_tokens (
    token_hash, client_id, resource_uri, principal_id, authenticated_identity_id,
    scopes, expires_at_ms, rotated_at_ms, revoked_at_ms, token_family_id
)
SELECT token.token_hash, token.client_id, token.resource_uri,
       principal.principal_id, principal.identity_id, token.scopes,
       token.expires_at_ms, token.rotated_at_ms, token.revoked_at_ms,
       token.token_family_id
FROM migration22_mcp_refresh_tokens token
JOIN migration22_identity_map principal
  ON principal.issuer = token.issuer AND principal.subject = token.subject;

INSERT INTO webhook_subscriptions (
    subscription_id, owner_principal_id, url, secret, event_kinds_json,
    state, disabled_reason, created_at_ms, updated_at_ms, revision
)
SELECT subscription.subscription_id, owner.principal_id, subscription.url,
       subscription.secret, subscription.event_kinds_json, subscription.state,
       subscription.disabled_reason, subscription.created_at_ms,
       subscription.updated_at_ms, subscription.revision
FROM migration22_webhook_subscriptions subscription
JOIN migration22_identity_map owner
  ON owner.issuer = subscription.owner_issuer
 AND owner.subject = subscription.owner_subject;

-- notes、ACL、文献のcopyで現行triggerが作った派生行を捨て、schema 22の状態をそのまま戻す。
DELETE FROM note_sync_changes;
DELETE FROM webhook_deliveries;
DELETE FROM webhook_outbox_events;

INSERT INTO note_sync_changes (
    change_sequence, principal_id, note_id, kind, reason, changed_at_ms
)
SELECT change.change_sequence, principal.principal_id, change.note_id,
       change.kind, change.reason, change.changed_at_ms
FROM migration22_note_sync_changes change
JOIN migration22_identity_map principal
  ON principal.issuer = change.issuer AND principal.subject = change.subject;

INSERT INTO note_sync_cursors (
    cursor_hash, principal_id, phase, after_note_id, after_sequence,
    high_watermark, expires_at_ms
)
SELECT cursor.cursor_hash, principal.principal_id, cursor.phase, cursor.after_note_id,
       cursor.after_sequence, cursor.high_watermark, cursor.expires_at_ms
FROM migration22_note_sync_cursors cursor
JOIN migration22_identity_map principal
  ON principal.issuer = cursor.issuer AND principal.subject = cursor.subject;

INSERT INTO webhook_outbox_events (
    event_sequence, event_id, owner_principal_id, event_kind,
    target_id, revision, occurred_at_ms
)
SELECT event.event_sequence, event.event_id, owner.principal_id, event.event_kind,
       event.target_id, event.revision, event.occurred_at_ms
FROM migration22_webhook_outbox_events event
JOIN migration22_identity_map owner
  ON owner.issuer = event.owner_issuer AND owner.subject = event.owner_subject;

-- outboxのcopyでもfan-out triggerが動くため、元の配送状態だけを残す。
DELETE FROM webhook_deliveries;
INSERT INTO webhook_deliveries SELECT * FROM migration22_webhook_deliveries;

DELETE FROM sqlite_sequence
WHERE name IN ('note_sync_changes', 'webhook_outbox_events');
INSERT INTO sqlite_sequence (name, seq)
SELECT name, seq FROM migration22_sequences;

DROP TABLE migration22_webhook_deliveries;
DROP TABLE migration22_webhook_outbox_events;
DROP TABLE migration22_webhook_subscriptions;
DROP TABLE migration22_mcp_refresh_tokens;
DROP TABLE migration22_mcp_access_tokens;
DROP TABLE migration22_mcp_authorization_codes;
DROP TABLE migration22_mcp_client_authorizations;
DROP TABLE migration22_mcp_client_scope_ceilings;
DROP TABLE migration22_mcp_principal_scope_ceilings;
DROP TABLE migration22_mcp_clients;
DROP TABLE migration22_oidc_login_attempts;
DROP TABLE migration22_web_sessions;
DROP TABLE migration22_note_sync_cursors;
DROP TABLE migration22_note_sync_changes;
DROP TABLE migration22_note_sync_state;
DROP TABLE migration22_note_acl;
DROP TABLE migration22_math_macro_settings;
DROP TABLE migration22_bibliography_import_links;
DROP TABLE migration22_bibliography_import_sources;
DROP TABLE migration22_bibliography_items;
DROP TABLE migration22_note_citations;
DROP TABLE migration22_note_references;
DROP TABLE migration22_notes;

DROP TABLE migration22_identity_map;
DROP TABLE migration22_sequences;
