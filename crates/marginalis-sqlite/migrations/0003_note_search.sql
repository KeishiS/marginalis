CREATE VIRTUAL TABLE note_search USING fts5(
    note_id UNINDEXED,
    title,
    content,
    tokenize = 'unicode61 remove_diacritics 2'
);
