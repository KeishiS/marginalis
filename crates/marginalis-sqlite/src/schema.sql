CREATE TABLE notes (
    note_id TEXT PRIMARY KEY NOT NULL,
    creator_issuer TEXT NOT NULL,
    creator_subject TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    tags_json TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    deleted_at_ms INTEGER
) STRICT;

CREATE TABLE note_acl (
    note_id TEXT NOT NULL REFERENCES notes(note_id) ON DELETE CASCADE,
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    permission INTEGER NOT NULL CHECK (permission BETWEEN 1 AND 3),
    PRIMARY KEY (note_id, issuer, subject)
) STRICT;

CREATE TABLE web_sessions (
    session_id_hash BLOB PRIMARY KEY NOT NULL,
    csrf_token_hash BLOB NOT NULL,
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    is_administrator INTEGER NOT NULL CHECK (is_administrator IN (0, 1)),
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
    registered_at_ms INTEGER NOT NULL
) STRICT;

CREATE TABLE mcp_authorization_codes (
    code_hash BLOB PRIMARY KEY NOT NULL,
    client_id TEXT NOT NULL REFERENCES mcp_clients(client_id),
    redirect_uri TEXT NOT NULL,
    resource_uri TEXT NOT NULL,
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    is_administrator INTEGER NOT NULL CHECK (is_administrator IN (0, 1)),
    scopes TEXT NOT NULL,
    code_challenge TEXT NOT NULL,
    expires_at_ms INTEGER NOT NULL,
    consumed_at_ms INTEGER
) STRICT;

CREATE TABLE mcp_access_tokens (
    token_hash BLOB PRIMARY KEY NOT NULL,
    client_id TEXT NOT NULL REFERENCES mcp_clients(client_id),
    resource_uri TEXT NOT NULL,
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    is_administrator INTEGER NOT NULL CHECK (is_administrator IN (0, 1)),
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
    is_administrator INTEGER NOT NULL CHECK (is_administrator IN (0, 1)),
    token_family_id BLOB NOT NULL CHECK (length(token_family_id) = 32)
) STRICT;
CREATE INDEX mcp_refresh_family_idx ON mcp_refresh_tokens (token_family_id);
