CREATE TABLE registration_policy (
    singleton INTEGER PRIMARY KEY NOT NULL CHECK (singleton = 1),
    policy TEXT NOT NULL CHECK (policy IN ('open', 'approval', 'invite-only'))
) STRICT;
INSERT INTO registration_policy (singleton, policy) VALUES (1, 'approval');
