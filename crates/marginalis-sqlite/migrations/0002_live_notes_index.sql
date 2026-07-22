CREATE INDEX notes_live_title_idx ON notes(title)
WHERE deleted_at_ms IS NULL;
