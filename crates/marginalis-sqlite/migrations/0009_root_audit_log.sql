CREATE TABLE root_audit_log (
    audit_id INTEGER PRIMARY KEY,
    action TEXT NOT NULL CHECK (action IN (
        'login-succeeded',
        'login-failed',
        'logout',
        'oidc-user-activated',
        'oidc-user-disabled',
        'registration-policy-changed',
        'mcp-client-registered',
        'mcp-client-authorization-revoked'
    )),
    actor_user_id TEXT,
    target_user_id TEXT,
    target TEXT,
    occurred_at_ms INTEGER NOT NULL
) STRICT;

CREATE INDEX root_audit_log_occurred_at
    ON root_audit_log (occurred_at_ms);
