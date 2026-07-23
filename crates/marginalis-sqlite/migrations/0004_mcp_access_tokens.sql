CREATE TABLE mcp_access_tokens (
    token_hash BLOB PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(user_id),
    resource_uri TEXT NOT NULL,
    scopes TEXT NOT NULL,
    expires_at_ms INTEGER NOT NULL,
    revoked_at_ms INTEGER
) STRICT;

CREATE INDEX mcp_access_tokens_user_idx ON mcp_access_tokens(user_id);
