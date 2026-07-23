ALTER TABLE notes ADD COLUMN creator_id TEXT NOT NULL DEFAULT '';
ALTER TABLE notes ADD COLUMN created_at TEXT NOT NULL DEFAULT '';
ALTER TABLE notes ADD COLUMN updated_at TEXT NOT NULL DEFAULT '';
CREATE INDEX notes_creator_created_idx ON notes(creator_id, created_at, note_id);
CREATE INDEX notes_updated_idx ON notes(updated_at, note_id);
CREATE TABLE note_tags (note_id TEXT NOT NULL REFERENCES notes(note_id) ON DELETE CASCADE, tag_key TEXT NOT NULL, PRIMARY KEY (note_id, tag_key)) STRICT;
CREATE INDEX note_tags_key_note_idx ON note_tags(tag_key, note_id);
