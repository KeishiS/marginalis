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
   'recent', '= recent', '[]', 'unknown', 0, 0, 4102444800000, 1, 4102444800000);

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
   'recent', '= recent', '[]', 4102444800000, 0, NULL, NULL, NULL);
