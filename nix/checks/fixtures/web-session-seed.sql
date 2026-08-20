-- ブラウザー試験用に、権限の異なる3利用者のWebセッションを投入する。
-- hashはtests/browser側が持つ試験専用Cookieとcsrf tokenに対応する。
INSERT INTO principals (principal_id) VALUES (2001), (2002), (2003);
INSERT INTO principal_identities
  (identity_id, principal_id, issuer, subject, is_primary)
VALUES
  (2001, 2001, 'https://id.example.test:8443/oauth2/openid/marginalis', 'reader-subject', 1),
  (2002, 2002, 'https://id.example.test:8443/oauth2/openid/marginalis', 'editor-subject', 1),
  (2003, 2003, 'https://id.example.test:8443/oauth2/openid/marginalis', 'outsider-subject', 1);
INSERT INTO web_sessions
  (session_id_hash, csrf_token_hash, principal_id, authenticated_identity_id, issued_at_ms,
   last_seen_at_ms, idle_expires_at_ms, absolute_expires_at_ms)
VALUES
  (X'9257575af58c9bed123fb881f8ed8ddac43449f996542b47d3f8ebd74affc997',
   X'06f4c546d56505fc3365ad0af9315b19674c857a5aa0eb07a4b520a373d5bb80',
   2001, 2001,
   1000000000000, 1000000000000, 4000000000000, 4000000000000),
  (X'2af479431a32c17ea66d6eec48a390ca8051630ffea1b2ae75f7deff228286c7',
   X'2ac2f8b7dbd2b4547e48f2c6d78535c68977644aa94ebacf1334bf1d4069c5cb',
   2002, 2002,
   1000000000000, 1000000000000, 4000000000000, 4000000000000),
  (X'621388f1a111b5f664f87a25c012d3f1776eb8e53bd0bfe95b7524b536e27d64',
   X'c635ac885a14aa3b00a3d3fcfc7c158a0975139fdc42defcebb751da43c436a8',
   2003, 2003,
   1000000000000, 1000000000000, 4000000000000, 4000000000000);
