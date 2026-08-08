-- schema 9（v0.9.0）のdatabaseへ、移行検査が期待するノート、参照、ACLを投入する。
INSERT INTO notes
  (note_id, creator_issuer, creator_subject, title, source, tags_json,
   created_at_ms, updated_at_ms, revision, deleted_at_ms)
VALUES
  ('019f0000-0000-7000-8000-000000000091',
   'https://id.example.test', 'migration-owner', '移行元',
   '= 移行元
:tags: 移行, 検証

xref:note:019f0000-0000-7000-8000-000000000092[移行先]',
   '["検証","移行"]', 1000, 4000, 4, NULL),
  ('019f0000-0000-7000-8000-000000000092',
   'https://id.example.test', 'migration-owner', '移行先',
   '= 移行先

削除済みの本文', '[]', 2000, 6000, 2, 6000);
INSERT INTO note_references (source_note_id, target_note_id)
VALUES ('019f0000-0000-7000-8000-000000000091',
        '019f0000-0000-7000-8000-000000000092');
INSERT INTO note_acl (note_id, issuer, subject, permission)
VALUES
  ('019f0000-0000-7000-8000-000000000091',
   'https://id.example.test', 'migration-reader', 'read'),
  ('019f0000-0000-7000-8000-000000000091',
   'https://id.example.test', 'migration-editor', 'edit');
