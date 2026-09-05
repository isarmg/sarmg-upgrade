PRAGMA foreign_keys = ON;
INSERT INTO product_metadata VALUES (1, 'host-monitoring', '0.8.0', 1, '12dd1e61426b6b99df3d429b8c36ee3a5b22d1da776d98fc960b45b4f58c8e05');
INSERT INTO auth_users (user_id, username, password_hash, active, created_at, session_version)
VALUES ('fixture-administrator', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', '$argon2id$v=19$m=19456,t=2,p=1$aOVS660TGXMspMgoSOcv6A$1eMydp1lX0/SzNdUYR28nln2fa2gMPGF626+W6gPKK8', 1, '2026-01-01T00:00:00Z', 1);
INSERT INTO auth_sessions VALUES ('fixture-session', 'fixture-administrator', 'fixture-session-token-digest', 1, '2026-01-01T00:00:00Z', '2026-01-01T00:01:00Z', '2030-01-01T00:00:00Z', '2030-01-01T12:00:00Z', NULL);
INSERT INTO auth_session_csrf_tokens (session_id, token_hash, created_at)
VALUES ('fixture-session', 'fixture-csrf-token-digest', '2026-01-01T00:01:00Z');
INSERT INTO monitored_hosts (host_id, name, os, arch, agent_version, registered_at, last_seen_at)
VALUES ('fixture-host', '边界主机-é-📈', 'linux', 'x86_64', '0.8.0', '2026-01-01T00:00:00Z', '2026-01-01T00:01:00Z');
INSERT INTO audit_events (action, target, detail, actor, created_at)
VALUES ('fixture.created', 'fixture-host', '{"unicode":"边界"}', 'fixture-administrator', '2026-01-01T00:01:00Z');
