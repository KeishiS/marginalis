-- v0.5.0（schema 5）のdatabaseへ、更新をまたいで保持されるノートを投入する。
INSERT INTO notes
  (note_id, creator_issuer, creator_subject, title, body, tags_json,
   created_at_ms, updated_at_ms, revision, deleted_at_ms)
VALUES
  ('019f0000-0000-7000-8000-000000000050', 'https://id.example.test', 'v0.5-user',
   'v0.5 note', 'kept across update', '["upgrade"]', 1, 2, 3, NULL);
