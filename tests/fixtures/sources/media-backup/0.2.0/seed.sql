PRAGMA foreign_keys = ON;
INSERT INTO product_metadata VALUES (1, 'media-backup', '0.2.0', 1, '2563e6afc3fff272d02b7a5615272cc773862243bfd15aec51655abf1d9c6b1c');
INSERT INTO accounts (id, created_at, display_name, storage_path, username, password_hash)
VALUES ('fixture-account', '2026-01-01T00:00:00Z', '图库用户-é-📱', 'fixture-account', 'fixture-owner', NULL);
INSERT INTO devices (id, account_id, name, platform, token_hash, created_at, last_seen_at)
VALUES ('fixture-device', 'fixture-account', '边界设备', 'android', zeroblob(32), '2026-01-01T00:00:00Z', '2026-01-01T00:01:00Z');
INSERT INTO assets (id, account_id, device_id, source_asset_id, media_kind, source_created_at_ms, created_at, updated_at)
VALUES ('fixture-asset', 'fixture-account', 'fixture-device', 'unicode-边界', 'image', 1767225600000, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
INSERT INTO auth_users (id, username, password_hash, active, session_version, created_at, updated_at)
VALUES ('fixture-administrator', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', '$argon2id$v=19$m=19456,t=2,p=1$aOVS660TGXMspMgoSOcv6A$1eMydp1lX0/SzNdUYR28nln2fa2gMPGF626+W6gPKK8', 1, 1, 1767225600, 1767225600);
INSERT INTO auth_sessions VALUES ('fixture-session', 'fixture-administrator', zeroblob(32), 1, 1767225600, 1767225660, 1893456000, 1893499200, NULL, 'fixture-agent', '127.0.0.1');
INSERT INTO admin_session_csrf_tokens (session_id, token_hash, created_at)
VALUES ('fixture-session', zeroblob(32), 1767225660);
INSERT INTO audit_events (account_id, actor_kind, actor_id, action, entity_kind, entity_id, occurred_at)
VALUES ('fixture-account', 'administrator', 'fixture-administrator', 'fixture.created', 'asset', 'fixture-asset', '2026-01-01T00:01:00Z');
