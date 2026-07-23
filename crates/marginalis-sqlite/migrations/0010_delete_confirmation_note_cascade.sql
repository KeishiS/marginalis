CREATE TABLE delete_confirmations_replacement (
    token_hash BLOB PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(user_id),
    note_id TEXT NOT NULL REFERENCES notes(note_id) ON DELETE CASCADE,
    source_revision TEXT NOT NULL,
    expires_at_ms INTEGER NOT NULL,
    consumed_at_ms INTEGER
) STRICT;

INSERT INTO delete_confirmations_replacement
    (token_hash, user_id, note_id, source_revision, expires_at_ms, consumed_at_ms)
SELECT token_hash, user_id, note_id, source_revision, expires_at_ms, consumed_at_ms
FROM delete_confirmations;

DROP TABLE delete_confirmations;
ALTER TABLE delete_confirmations_replacement RENAME TO delete_confirmations;
CREATE INDEX delete_confirmations_expiry_idx ON delete_confirmations(expires_at_ms);
