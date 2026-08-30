CREATE TABLE product_metadata (
    singleton INTEGER PRIMARY KEY NOT NULL CHECK (singleton = 1),
    application TEXT NOT NULL,
    application_version TEXT NOT NULL,
    schema_revision INTEGER NOT NULL,
    schema_sha256 TEXT NOT NULL
);

CREATE TABLE store_meta (
    key   TEXT PRIMARY KEY,
    value BLOB NOT NULL
) STRICT, WITHOUT ROWID;

CREATE TABLE operations (
    owner_digest   BLOB NOT NULL CHECK(length(owner_digest) = 32),
    operation_id   BLOB NOT NULL CHECK(length(operation_id) = 16),
    fingerprint    BLOB NOT NULL CHECK(length(fingerprint) = 32),
    lease_token    BLOB NOT NULL CHECK(length(lease_token) = 16),
    state          INTEGER NOT NULL CHECK(state IN (0, 1, 2)),
    terminal_state INTEGER CHECK(terminal_state IN (0, 1, 2)),
    http_status    INTEGER,
    error_code     TEXT,
    created_at_ms  INTEGER NOT NULL,
    updated_at_ms  INTEGER NOT NULL,
    expires_at_ms  INTEGER,
    PRIMARY KEY(owner_digest, operation_id),
    CHECK(error_code IS NULL OR length(error_code) BETWEEN 1 AND 128),
    CHECK(
        (state IN (0, 1)
         AND terminal_state IS NULL
         AND http_status IS NULL
         AND error_code IS NULL
         AND expires_at_ms IS NULL)
        OR
        (state = 2
         AND terminal_state IS NOT NULL
         AND http_status BETWEEN 100 AND 599
         AND expires_at_ms IS NOT NULL
         AND (
             (terminal_state = 0
              AND http_status BETWEEN 200 AND 299
              AND error_code IS NULL)
             OR
             (terminal_state IN (1, 2)
              AND NOT (http_status BETWEEN 200 AND 299)
              AND error_code IS NOT NULL)
         ))
    )
) STRICT, WITHOUT ROWID;

CREATE INDEX operations_expiry
ON operations(expires_at_ms) WHERE state = 2;

CREATE TABLE upload_sessions (
    owner_digest    BLOB NOT NULL CHECK(length(owner_digest) = 32),
    upload_id       BLOB NOT NULL CHECK(length(upload_id) = 16),
    target_path     BLOB NOT NULL CHECK(length(target_path) BETWEEN 1 AND 65536),
    stage_path      BLOB NOT NULL CHECK(length(stage_path) BETWEEN 1 AND 65536),
    upload_length   INTEGER NOT NULL CHECK(upload_length >= 0),
    durable_offset  INTEGER NOT NULL CHECK(durable_offset >= 0),
    state           INTEGER NOT NULL CHECK(state IN (0, 1, 2, 3, 4, 5)),
    stage_device_be BLOB CHECK(stage_device_be IS NULL OR length(stage_device_be) = 8),
    stage_inode_be  BLOB CHECK(stage_inode_be IS NULL OR length(stage_inode_be) = 8),
    target_revision BLOB CHECK(target_revision IS NULL OR length(target_revision) = 32),
    updated_at_ms   INTEGER NOT NULL,
    expires_at_ms   INTEGER NOT NULL,
    PRIMARY KEY(owner_digest, upload_id),
    CHECK(target_path != stage_path),
    CHECK(durable_offset <= upload_length),
    CHECK((stage_device_be IS NULL) = (stage_inode_be IS NULL)),
    CHECK(state != 2 OR durable_offset = upload_length),
    CHECK(state != 1 OR durable_offset = upload_length),
    CHECK(state != 5 OR durable_offset = upload_length)
) STRICT, WITHOUT ROWID;

CREATE INDEX upload_sessions_expiry
ON upload_sessions(expires_at_ms);

CREATE TABLE purge_jobs (
    owner_digest      BLOB NOT NULL CHECK(length(owner_digest) = 32),
    job_id            BLOB NOT NULL CHECK(length(job_id) = 16),
    target_path       BLOB NOT NULL CHECK(length(target_path) BETWEEN 1 AND 65536),
    trash_path        BLOB NOT NULL UNIQUE CHECK(length(trash_path) BETWEEN 1 AND 65536),
    source_device_be  BLOB NOT NULL CHECK(length(source_device_be) = 8),
    source_inode_be   BLOB NOT NULL CHECK(length(source_inode_be) = 8),
    trash_revision    BLOB CHECK(trash_revision IS NULL OR length(trash_revision) = 32),
    is_directory      INTEGER NOT NULL CHECK(is_directory IN (0, 1)),
    state             INTEGER NOT NULL CHECK(state IN (0, 1, 2)),
    attempts          INTEGER NOT NULL CHECK(attempts BETWEEN 0 AND 4294967295),
    next_attempt_at_ms INTEGER NOT NULL,
    created_at_ms     INTEGER NOT NULL,
    updated_at_ms     INTEGER NOT NULL,
    PRIMARY KEY(owner_digest, job_id),
    CHECK(target_path != trash_path)
) STRICT, WITHOUT ROWID;

CREATE INDEX purge_jobs_due
ON purge_jobs(state, next_attempt_at_ms, created_at_ms)
WHERE state = 1;

CREATE INDEX purge_jobs_prepared
ON purge_jobs(created_at_ms)
WHERE state = 0;
