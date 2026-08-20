-- schema 22の外部identity列を、schema 23の内部principal参照へ置き換える準備。
-- 外部キー検査はmigration runnerがtransaction外で無効化し、copy後に全件検査する。

CREATE TEMP TABLE migration22_sequences AS
SELECT name, seq FROM sqlite_sequence
WHERE name = 'webhook_outbox_events';

DROP VIEW note_access;

DROP TRIGGER note_acl_reject_owner;
DROP TRIGGER note_sync_after_note_insert;
DROP TRIGGER note_sync_after_note_update;
DROP TRIGGER note_sync_after_acl_insert;
DROP TRIGGER note_sync_after_acl_delete;
DROP TRIGGER webhook_fan_out_after_event_insert;
DROP TRIGGER webhook_after_note_insert;
DROP TRIGGER webhook_after_note_update;
DROP TRIGGER webhook_after_note_delete;
DROP TRIGGER webhook_after_note_restore;
DROP TRIGGER webhook_after_bibliography_insert;
DROP TRIGGER webhook_after_bibliography_update;
DROP TRIGGER webhook_after_bibliography_delete;

DROP INDEX notes_owner_listing_idx;
DROP INDEX notes_visible_provenance_idx;
DROP INDEX note_references_target_idx;
DROP INDEX note_citations_key_idx;
DROP INDEX bibliography_items_owner_listing_idx;
DROP INDEX bibliography_import_sources_owner_idx;
DROP INDEX bibliography_import_links_item_idx;
DROP INDEX note_acl_identity_idx;
DROP INDEX note_sync_changes_principal_sequence_idx;
DROP INDEX note_sync_cursors_expiry_idx;
DROP INDEX web_sessions_subject_idx;
DROP INDEX mcp_client_scope_ceilings_client_id_idx;
DROP INDEX mcp_client_authorizations_client_id_idx;
DROP INDEX mcp_access_subject_idx;
DROP INDEX mcp_refresh_family_idx;
DROP INDEX webhook_subscriptions_owner_idx;
DROP INDEX webhook_outbox_events_owner_idx;
DROP INDEX webhook_deliveries_due_idx;

ALTER TABLE notes RENAME TO migration22_notes;
ALTER TABLE note_references RENAME TO migration22_note_references;
ALTER TABLE note_citations RENAME TO migration22_note_citations;
ALTER TABLE bibliography_items RENAME TO migration22_bibliography_items;
ALTER TABLE bibliography_import_sources RENAME TO migration22_bibliography_import_sources;
ALTER TABLE bibliography_import_links RENAME TO migration22_bibliography_import_links;
ALTER TABLE math_macro_settings RENAME TO migration22_math_macro_settings;
ALTER TABLE note_acl RENAME TO migration22_note_acl;
ALTER TABLE note_sync_state RENAME TO migration22_note_sync_state;
ALTER TABLE note_sync_changes RENAME TO migration22_note_sync_changes;
ALTER TABLE note_sync_cursors RENAME TO migration22_note_sync_cursors;
ALTER TABLE web_sessions RENAME TO migration22_web_sessions;
ALTER TABLE oidc_login_attempts RENAME TO migration22_oidc_login_attempts;
ALTER TABLE mcp_clients RENAME TO migration22_mcp_clients;
ALTER TABLE mcp_principal_scope_ceilings RENAME TO migration22_mcp_principal_scope_ceilings;
ALTER TABLE mcp_client_scope_ceilings RENAME TO migration22_mcp_client_scope_ceilings;
ALTER TABLE mcp_client_authorizations RENAME TO migration22_mcp_client_authorizations;
ALTER TABLE mcp_authorization_codes RENAME TO migration22_mcp_authorization_codes;
ALTER TABLE mcp_access_tokens RENAME TO migration22_mcp_access_tokens;
ALTER TABLE mcp_refresh_tokens RENAME TO migration22_mcp_refresh_tokens;
ALTER TABLE webhook_subscriptions RENAME TO migration22_webhook_subscriptions;
ALTER TABLE webhook_outbox_events RENAME TO migration22_webhook_outbox_events;
ALTER TABLE webhook_deliveries RENAME TO migration22_webhook_deliveries;
