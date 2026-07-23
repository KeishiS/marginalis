ALTER TABLE mcp_access_tokens ADD COLUMN issued_at_ms INTEGER NOT NULL DEFAULT 0;
ALTER TABLE mcp_access_tokens ADD COLUMN last_used_at_ms INTEGER;
ALTER TABLE mcp_refresh_tokens ADD COLUMN issued_at_ms INTEGER NOT NULL DEFAULT 0;
