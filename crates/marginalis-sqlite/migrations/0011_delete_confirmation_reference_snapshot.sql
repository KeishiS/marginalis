ALTER TABLE delete_confirmations
ADD COLUMN incoming_reference_state_hash BLOB NOT NULL DEFAULT X'';
