CREATE TABLE delete_confirmations (
    token_hash BLOB PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(user_id),
    note_id TEXT NOT NULL REFERENCES notes(note_id),
    source_revision TEXT NOT NULL,
    expires_at_ms INTEGER NOT NULL,
    consumed_at_ms INTEGER
) STRICT;

CREATE INDEX delete_confirmations_expiry_idx ON delete_confirmations(expires_at_ms);
