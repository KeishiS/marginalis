-- 期限切れ削除の対象（stale）と保持対象（recent）のノートを投入する。
INSERT INTO principals (principal_id) VALUES (1001), (1002);
INSERT INTO principal_identities
  (identity_id, principal_id, issuer, subject, is_primary)
VALUES
  (1001, 1001, 'https://id.example.test', 'stale', 1),
  (1002, 1002, 'https://id.example.test', 'recent', 1);
INSERT INTO notes
  (note_id, creator_principal_id, title, source, tags_json, created_via,
   review_tracking_known, created_at_ms, updated_at_ms, revision, deleted_at_ms)
VALUES
  ('019f0000-0000-7000-8000-000000000001', 1001,
   'stale', '= stale', '[]', 'unknown', 0, 0, 0, 1, 0),
  ('019f0000-0000-7000-8000-000000000002', 1002,
   'recent', '= recent

image::attachment:019f0000-0000-7000-8000-000000000012[]',
   '[]', 'unknown', 0, 0, 4102444800000, 1, 4102444800000);

-- 公開書庫から取り込んだ状態と同様に、現在版の完全な履歴も投入する。
-- stale側は、ノートの物理削除に伴って履歴も削除されることを確認するために使う。
INSERT INTO note_revisions
  (note_id, revision, changed_at_ms, changed_by_principal_id, change_kind,
   title, source, tags_json, deleted_at_ms, review_tracking_known,
   reviewed_revision, reviewed_at_ms, reviewer_principal_id)
VALUES
  ('019f0000-0000-7000-8000-000000000001', 1, 0, 1001, 'imported',
   'stale', '= stale', '[]', 0, 0, NULL, NULL, NULL),
  ('019f0000-0000-7000-8000-000000000002', 1, 4102444800000, 1002, 'imported',
   'recent', '= recent

image::attachment:019f0000-0000-7000-8000-000000000012[]',
   '[]', 4102444800000, 0, NULL, NULL, NULL);

-- 中断したuploadだけを期限後に削除し、版が参照する画像と新しいuploadを保持する。
INSERT INTO note_attachments
  (attachment_id, note_id, file_name, media_type, byte_length, sha256, content,
   created_at_ms, created_by_principal_id)
VALUES
  ('019f0000-0000-7000-8000-000000000011',
   '019f0000-0000-7000-8000-000000000002', 'expired.png', 'image/png', 68,
   X'431ced6916a2a21a156e38701afe55bbd7f88969fbbfc56d7fe099d47f265460',
   X'89504e470d0a1a0a0000000d4948445200000001000000010804000000b51c0c020000000b4944415478da6364f80f00010501012718e3660000000049454e44ae426082',
   0, 1002),
  ('019f0000-0000-7000-8000-000000000012',
   '019f0000-0000-7000-8000-000000000002', 'referenced.png', 'image/png', 68,
   X'431ced6916a2a21a156e38701afe55bbd7f88969fbbfc56d7fe099d47f265460',
   X'89504e470d0a1a0a0000000d4948445200000001000000010804000000b51c0c020000000b4944415478da6364f80f00010501012718e3660000000049454e44ae426082',
   0, 1002),
  ('019f0000-0000-7000-8000-000000000013',
   '019f0000-0000-7000-8000-000000000002', 'recent.png', 'image/png', 68,
   X'431ced6916a2a21a156e38701afe55bbd7f88969fbbfc56d7fe099d47f265460',
   X'89504e470d0a1a0a0000000d4948445200000001000000010804000000b51c0c020000000b4944415478da6364f80f00010501012718e3660000000049454e44ae426082',
   4102444800000, 1002);
INSERT INTO note_revision_attachments (note_id, revision, attachment_id)
VALUES
  ('019f0000-0000-7000-8000-000000000002', 1,
   '019f0000-0000-7000-8000-000000000012');
