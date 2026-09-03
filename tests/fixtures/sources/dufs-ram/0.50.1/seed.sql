PRAGMA foreign_keys = ON;
INSERT INTO product_metadata VALUES (1, 'dufs-ram', '0.50.1', 1, '3659ff0c703515f555af95f0f1c08c35fa0555a8978f5f0e5a658fd93d225423');
INSERT INTO store_meta (key, value) VALUES ('fixture-unicode', CAST('文件-é-📁' AS BLOB));
INSERT INTO operations (owner_digest, operation_id, fingerprint, lease_token, state, terminal_state, http_status, created_at_ms, updated_at_ms, expires_at_ms)
VALUES (zeroblob(32), zeroblob(16), zeroblob(32), x'01010101010101010101010101010101', 2, 0, 200, 1767225600000, 1767225660000, 1893456000000);
