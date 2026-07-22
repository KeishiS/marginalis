CREATE TABLE users (
    user_id TEXT PRIMARY KEY NOT NULL,
    authentication_kind TEXT NOT NULL CHECK (authentication_kind IN ('oidc', 'root')),
    status TEXT NOT NULL CHECK (status IN ('pending', 'active', 'disabled')),
    display_name TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
) STRICT;

CREATE TABLE oidc_identities (
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(user_id),
    PRIMARY KEY (issuer, subject),
    UNIQUE (user_id)
) STRICT;

CREATE TABLE oidc_login_attempts (
    state_hash BLOB PRIMARY KEY NOT NULL,
    nonce TEXT NOT NULL,
    pkce_verifier TEXT NOT NULL,
    expires_at_ms INTEGER NOT NULL
) STRICT;

CREATE TABLE web_sessions (
    session_id_hash BLOB PRIMARY KEY NOT NULL,
    csrf_token_hash BLOB NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(user_id),
    idle_timeout_ms INTEGER NOT NULL CHECK (idle_timeout_ms > 0),
    issued_at_ms INTEGER NOT NULL,
    last_seen_at_ms INTEGER NOT NULL,
    idle_expires_at_ms INTEGER NOT NULL,
    absolute_expires_at_ms INTEGER NOT NULL,
    revoked_at_ms INTEGER
) STRICT;

CREATE INDEX web_sessions_user_idx ON web_sessions(user_id);

CREATE TABLE root_credentials (
    user_id TEXT PRIMARY KEY NOT NULL REFERENCES users(user_id),
    password_hash TEXT NOT NULL
) STRICT;

CREATE TABLE notes (
    note_id TEXT PRIMARY KEY NOT NULL,
    relative_path TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL,
    source_revision BLOB NOT NULL,
    deleted_at_ms INTEGER
) STRICT;

CREATE TABLE note_acl (
    note_id TEXT NOT NULL REFERENCES notes(note_id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(user_id),
    permission INTEGER NOT NULL CHECK (permission BETWEEN 1 AND 3),
    PRIMARY KEY (note_id, user_id)
) STRICT;

CREATE TABLE note_anchors (
    note_id TEXT NOT NULL REFERENCES notes(note_id) ON DELETE CASCADE,
    anchor_id TEXT NOT NULL,
    PRIMARY KEY (note_id, anchor_id)
) STRICT;

CREATE TABLE note_references (
    source_note_id TEXT NOT NULL REFERENCES notes(note_id) ON DELETE CASCADE,
    source_start INTEGER NOT NULL CHECK (source_start >= 0),
    source_end INTEGER NOT NULL CHECK (source_end > source_start),
    target_note_id TEXT NOT NULL,
    target_anchor TEXT,
    PRIMARY KEY (source_note_id, source_start, source_end)
) STRICT;

CREATE INDEX note_references_target_idx ON note_references(target_note_id, target_anchor);

CREATE TABLE operation_journal (
    operation_id TEXT PRIMARY KEY NOT NULL,
    kind TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('prepared', 'source_applied', 'completed')),
    note_id TEXT NOT NULL,
    source_revision BLOB,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
) STRICT;

CREATE INDEX operation_journal_incomplete_idx ON operation_journal(state)
WHERE state <> 'completed';
