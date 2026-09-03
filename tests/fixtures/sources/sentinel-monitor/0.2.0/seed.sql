PRAGMA foreign_keys = ON;
INSERT INTO product_metadata VALUES (1, 'sentinel-monitor', '0.2.0', 1, 'f547ddc817d830d23b5305bb1f88b29898d6531568edd6eb194c2b629eb560c0');
INSERT INTO users (id, username, password_hash, active, created_at, updated_at, session_version)
VALUES ('fixture-administrator', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', '$argon2id$v=19$m=19456,t=2,p=1$QVY45eyzTvMwT00q1qHjow$t6njtuXI3oRWbaqjK8pyUNyFtckOF2HdosRzSxbZtpk', 1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1);
INSERT INTO browser_sessions VALUES ('fixture-session', 'fixture-administrator', zeroblob(32), zeroblob(32), 1, '2026-01-01T00:00:00Z', '2026-01-01T00:01:00Z', '2030-01-01T00:00:00Z', '2030-01-01T12:00:00Z', NULL);
INSERT INTO cameras (id, name, location, main_stream_url_enc, created_by, created_at, updated_at)
VALUES ('fixture-camera', '入口摄像机-é-📷', '边界位置', x'01020304', 'fixture-administrator', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
INSERT INTO audit_logs (id, user_id, action, entity_type, entity_id, details, created_at)
VALUES ('fixture-audit', 'fixture-administrator', 'fixture.created', 'camera', 'fixture-camera', '{"unicode":"边界"}', '2026-01-01T00:01:00Z');
