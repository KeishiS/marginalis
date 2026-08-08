-- v0.5.0のschema適用前に、schema_migrations表だけを持つ状態を再現する。
CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY NOT NULL);
INSERT INTO schema_migrations (version) VALUES (4);
