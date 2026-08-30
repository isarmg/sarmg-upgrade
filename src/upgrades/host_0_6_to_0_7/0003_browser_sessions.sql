ALTER TABLE auth_users
    ADD COLUMN session_version INTEGER NOT NULL DEFAULT 1 CHECK (session_version > 0);

CREATE TABLE auth_sessions (
    session_id            TEXT PRIMARY KEY,
    user_id               TEXT NOT NULL REFERENCES auth_users(user_id) ON DELETE CASCADE,
    token_hash            TEXT NOT NULL UNIQUE,
    user_session_version  INTEGER NOT NULL,
    created_at            TEXT NOT NULL,
    last_seen_at          TEXT NOT NULL,
    idle_expires_at       TEXT NOT NULL,
    absolute_expires_at   TEXT NOT NULL,
    revoked_at            TEXT,
    CHECK (idle_expires_at <= absolute_expires_at)
);

CREATE INDEX auth_sessions_user ON auth_sessions(user_id, created_at DESC);
CREATE INDEX auth_sessions_expiry ON auth_sessions(idle_expires_at, absolute_expires_at)
    WHERE revoked_at IS NULL;

CREATE TABLE auth_session_csrf_tokens (
    csrf_id       INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id    TEXT NOT NULL REFERENCES auth_sessions(session_id) ON DELETE CASCADE,
    token_hash    TEXT NOT NULL UNIQUE,
    created_at    TEXT NOT NULL
);

CREATE INDEX auth_session_csrf_tokens_session
    ON auth_session_csrf_tokens(session_id, csrf_id DESC);

CREATE TRIGGER auth_users_invalidate_sessions_after_security_change
AFTER UPDATE OF password_hash, active ON auth_users
FOR EACH ROW
WHEN OLD.password_hash IS NOT NEW.password_hash OR OLD.active IS NOT NEW.active
BEGIN
    UPDATE auth_users
       SET session_version = OLD.session_version + 1
     WHERE user_id = NEW.user_id;

    UPDATE auth_sessions
       SET revoked_at = COALESCE(revoked_at, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
     WHERE user_id = NEW.user_id
       AND revoked_at IS NULL;

    DELETE FROM auth_session_csrf_tokens
     WHERE session_id IN (
         SELECT session_id FROM auth_sessions WHERE user_id = NEW.user_id
     );
END;
