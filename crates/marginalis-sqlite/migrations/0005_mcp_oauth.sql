CREATE TABLE mcp_oauth_clients (
    client_id TEXT PRIMARY KEY NOT NULL,
    display_name TEXT NOT NULL,
    redirect_uris_json TEXT NOT NULL
) STRICT;

CREATE TABLE mcp_authorization_codes (
    code_hash BLOB PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(user_id),
    client_id TEXT NOT NULL REFERENCES mcp_oauth_clients(client_id),
    redirect_uri TEXT NOT NULL,
    resource_uri TEXT NOT NULL,
    scopes TEXT NOT NULL,
    code_challenge TEXT NOT NULL,
    expires_at_ms INTEGER NOT NULL,
    consumed_at_ms INTEGER
) STRICT;

CREATE INDEX mcp_authorization_codes_expiry_idx ON mcp_authorization_codes(expires_at_ms);

ALTER TABLE mcp_access_tokens ADD COLUMN client_id TEXT NOT NULL DEFAULT '';

CREATE TABLE mcp_refresh_tokens (
    token_hash BLOB PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(user_id),
    client_id TEXT NOT NULL REFERENCES mcp_oauth_clients(client_id),
    resource_uri TEXT NOT NULL,
    scopes TEXT NOT NULL,
    expires_at_ms INTEGER NOT NULL,
    rotated_at_ms INTEGER,
    revoked_at_ms INTEGER
) STRICT;

CREATE INDEX mcp_refresh_tokens_user_client_idx ON mcp_refresh_tokens(user_id, client_id);
