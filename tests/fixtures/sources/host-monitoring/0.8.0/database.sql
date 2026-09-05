CREATE TABLE product_metadata (
    singleton INTEGER PRIMARY KEY NOT NULL CHECK (singleton = 1),
    application TEXT NOT NULL,
    application_version TEXT NOT NULL,
    schema_revision INTEGER NOT NULL,
    schema_sha256 TEXT NOT NULL
);

CREATE TABLE monitored_hosts (
    host_id                  TEXT PRIMARY KEY,
    name                     TEXT NOT NULL,
    os                       TEXT NOT NULL,
    os_version               TEXT,
    kernel_version           TEXT,
    arch                     TEXT NOT NULL,
    agent_version            TEXT NOT NULL,
    capabilities             TEXT NOT NULL DEFAULT '[]',
    registered_at            TEXT NOT NULL,
    last_seen_at             TEXT NOT NULL,
    latest_report_id         TEXT,
    latest_collected_at      TEXT,
    latest_interval_seconds  REAL,
    lifecycle_status         TEXT NOT NULL DEFAULT 'active',
    revoked_at               TEXT
);

CREATE INDEX monitored_hosts_registered ON monitored_hosts(registered_at, host_id);
CREATE INDEX monitored_hosts_last_seen ON monitored_hosts(last_seen_at DESC);
CREATE INDEX monitored_hosts_latest_report_retention
    ON monitored_hosts(latest_report_id)
    WHERE latest_report_id IS NOT NULL;

CREATE TABLE agent_metric_reports (
    report_id                             TEXT PRIMARY KEY,
    host_id                               TEXT NOT NULL REFERENCES monitored_hosts(host_id) ON DELETE CASCADE,
    schema_version                        INTEGER NOT NULL,
    collected_at                          TEXT NOT NULL,
    received_at                           TEXT NOT NULL,
    interval_seconds                      REAL NOT NULL,
    payload                               TEXT,
    cpu_usage_percent                     REAL,
    memory_usage_percent                  REAL,
    network_received_bytes_per_second     REAL,
    network_transmitted_bytes_per_second  REAL,
    disk_read_bytes_per_second            REAL,
    disk_written_bytes_per_second         REAL,
    max_temperature_celsius               REAL,
    gpu_utilization_percent               REAL,
    gpu_memory_usage_percent              REAL,
    aggregated_at                         TEXT
);

CREATE INDEX agent_metric_reports_host_collected
    ON agent_metric_reports(host_id, collected_at DESC, report_id DESC);
CREATE INDEX agent_metric_reports_received ON agent_metric_reports(received_at);
CREATE INDEX agent_metric_reports_retention_pending
    ON agent_metric_reports(collected_at, report_id)
    WHERE aggregated_at IS NULL;
CREATE INDEX agent_metric_reports_retention_delete
    ON agent_metric_reports(aggregated_at, report_id)
    WHERE aggregated_at IS NOT NULL;

CREATE TABLE agent_credentials (
    credential_id   TEXT PRIMARY KEY,
    host_id         TEXT NOT NULL REFERENCES monitored_hosts(host_id) ON DELETE CASCADE,
    token_hash      TEXT NOT NULL UNIQUE,
    issued_at       TEXT NOT NULL,
    last_used_at    TEXT,
    revoked_at      TEXT
);

CREATE INDEX agent_credentials_host ON agent_credentials(host_id);
CREATE INDEX agent_credentials_active_token
    ON agent_credentials(token_hash)
    WHERE revoked_at IS NULL;

CREATE TABLE agent_instance_invites (
    invite_id             TEXT PRIMARY KEY,
    instance_id           TEXT NOT NULL,
    activation_code_hash  TEXT NOT NULL UNIQUE,
    display_name          TEXT NOT NULL,
    status                TEXT NOT NULL DEFAULT 'pending',
    expires_at            TEXT NOT NULL,
    created_at            TEXT NOT NULL,
    activated_at          TEXT,
    cancelled_at          TEXT
);

CREATE INDEX agent_instance_invites_created
    ON agent_instance_invites(created_at DESC);
CREATE UNIQUE INDEX agent_instance_invites_one_pending
    ON agent_instance_invites(instance_id)
    WHERE status = 'pending';

CREATE TABLE agent_pairing_requests (
    request_id           TEXT PRIMARY KEY,
    requested_host_id    TEXT NOT NULL,
    os                   TEXT NOT NULL,
    os_version           TEXT,
    kernel_version       TEXT,
    arch                 TEXT NOT NULL,
    agent_version        TEXT NOT NULL,
    token_hash           TEXT NOT NULL UNIQUE,
    polling_secret_hash  TEXT NOT NULL UNIQUE,
    status               TEXT NOT NULL DEFAULT 'pending',
    invite_id            TEXT UNIQUE,
    instance_id          TEXT,
    expires_at           TEXT NOT NULL,
    created_at           TEXT NOT NULL,
    activated_at         TEXT
);

CREATE INDEX agent_pairing_requests_expiry
    ON agent_pairing_requests(expires_at)
    WHERE status = 'pending';
CREATE INDEX agent_pairing_requests_pending_device
    ON agent_pairing_requests(requested_host_id, expires_at)
    WHERE status = 'pending';

CREATE TABLE audit_events (
    event_id      INTEGER PRIMARY KEY AUTOINCREMENT,
    action        TEXT NOT NULL,
    target        TEXT NOT NULL,
    detail        TEXT,
    actor         TEXT NOT NULL,
    created_at    TEXT NOT NULL
);

CREATE INDEX audit_events_created ON audit_events(created_at DESC);

CREATE TABLE auth_users (
    user_id          TEXT PRIMARY KEY,
    username         TEXT NOT NULL UNIQUE CHECK (
        length(username) BETWEEN 3 AND 64
        AND username = lower(username)
        AND username NOT GLOB '*[^a-z0-9._-]*'
        AND substr(username, 1, 1) GLOB '[a-z0-9]'
        AND substr(username, -1, 1) GLOB '[a-z0-9]'
    ),
    password_hash    TEXT NOT NULL CHECK (length(password_hash) > 0),
    active           INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    created_at       TEXT NOT NULL,
    session_version  INTEGER NOT NULL DEFAULT 1 CHECK (session_version > 0)
);

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
CREATE INDEX auth_sessions_expiry
    ON auth_sessions(idle_expires_at, absolute_expires_at)
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

CREATE TABLE agent_metric_hourly_aggregates (
    host_id                                       TEXT NOT NULL REFERENCES monitored_hosts(host_id) ON DELETE CASCADE,
    bucket_start                                  TEXT NOT NULL,
    interval_start                                TEXT NOT NULL,
    interval_end                                  TEXT NOT NULL,
    sample_count                                  INTEGER NOT NULL CHECK (sample_count > 0),

    cpu_usage_percent_count                       INTEGER NOT NULL,
    cpu_usage_percent_min                         REAL,
    cpu_usage_percent_max                         REAL,
    cpu_usage_percent_avg                         REAL,

    memory_usage_percent_count                    INTEGER NOT NULL,
    memory_usage_percent_min                      REAL,
    memory_usage_percent_max                      REAL,
    memory_usage_percent_avg                      REAL,

    network_received_bytes_per_second_count       INTEGER NOT NULL,
    network_received_bytes_per_second_min         REAL,
    network_received_bytes_per_second_max         REAL,
    network_received_bytes_per_second_avg         REAL,

    network_transmitted_bytes_per_second_count    INTEGER NOT NULL,
    network_transmitted_bytes_per_second_min      REAL,
    network_transmitted_bytes_per_second_max      REAL,
    network_transmitted_bytes_per_second_avg      REAL,

    disk_read_bytes_per_second_count              INTEGER NOT NULL,
    disk_read_bytes_per_second_min                REAL,
    disk_read_bytes_per_second_max                REAL,
    disk_read_bytes_per_second_avg                REAL,

    disk_written_bytes_per_second_count           INTEGER NOT NULL,
    disk_written_bytes_per_second_min             REAL,
    disk_written_bytes_per_second_max             REAL,
    disk_written_bytes_per_second_avg             REAL,

    max_temperature_celsius_count                 INTEGER NOT NULL,
    max_temperature_celsius_min                   REAL,
    max_temperature_celsius_max                   REAL,
    max_temperature_celsius_avg                   REAL,

    gpu_utilization_percent_count                 INTEGER NOT NULL,
    gpu_utilization_percent_min                   REAL,
    gpu_utilization_percent_max                   REAL,
    gpu_utilization_percent_avg                   REAL,

    gpu_memory_usage_percent_count                INTEGER NOT NULL,
    gpu_memory_usage_percent_min                  REAL,
    gpu_memory_usage_percent_max                  REAL,
    gpu_memory_usage_percent_avg                  REAL,

    updated_at                                    TEXT NOT NULL,
    PRIMARY KEY (host_id, bucket_start),
    CHECK (bucket_start <= interval_start),
    CHECK (interval_start <= interval_end),
    CHECK (cpu_usage_percent_count BETWEEN 0 AND sample_count),
    CHECK (memory_usage_percent_count BETWEEN 0 AND sample_count),
    CHECK (network_received_bytes_per_second_count BETWEEN 0 AND sample_count),
    CHECK (network_transmitted_bytes_per_second_count BETWEEN 0 AND sample_count),
    CHECK (disk_read_bytes_per_second_count BETWEEN 0 AND sample_count),
    CHECK (disk_written_bytes_per_second_count BETWEEN 0 AND sample_count),
    CHECK (max_temperature_celsius_count BETWEEN 0 AND sample_count),
    CHECK (gpu_utilization_percent_count BETWEEN 0 AND sample_count),
    CHECK (gpu_memory_usage_percent_count BETWEEN 0 AND sample_count)
);

CREATE INDEX agent_metric_hourly_aggregates_retention
    ON agent_metric_hourly_aggregates(interval_end, host_id, bucket_start);
