-- 期限切れ削除の対象（stale）と保持対象（recent）のノートを投入する。
INSERT INTO notes
  (note_id, creator_issuer, creator_subject, title, source, tags_json, created_via,
   review_tracking_known, created_at_ms, updated_at_ms, revision, deleted_at_ms)
VALUES
  ('019f0000-0000-7000-8000-000000000001', 'https://id.example.test',
   'stale', 'stale', '= stale', '[]', 'unknown', 0, 0, 0, 1, 0),
  ('019f0000-0000-7000-8000-000000000002', 'https://id.example.test',
   'recent', 'recent', '= recent', '[]', 'unknown', 0, 0, 4102444800000, 1, 4102444800000);
