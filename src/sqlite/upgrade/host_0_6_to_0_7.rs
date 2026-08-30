#[cfg(test)]
use anyhow::Context;
use anyhow::ensure;
use rusqlite::{Connection, OptionalExtension};

use super::{Adapter, Product, expected_migration_checksum};
use crate::sqlite::HOST_CURRENT_SCHEMA_SHA256;

const MIGRATION_0001: &str =
    include_str!("../../upgrades/host_0_6_to_0_7/0001_host_monitoring.sql");
const MIGRATION_0002: &str = include_str!("../../upgrades/host_0_6_to_0_7/0002_auth_users.sql");
const MIGRATION_0003: &str =
    include_str!("../../upgrades/host_0_6_to_0_7/0003_browser_sessions.sql");
const MIGRATION_0004: &str =
    include_str!("../../upgrades/host_0_6_to_0_7/0004_pairing_admission.sql");
const MIGRATION_0005: &str =
    include_str!("../../upgrades/host_0_6_to_0_7/0005_telemetry_retention.sql");
const TARGET_SCHEMA_SQL: &str = include_str!("../../upgrades/host_0_6_to_0_7/target.sql");

#[cfg(test)]
const SQLX_LEDGER_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS _sqlx_migrations (
    version BIGINT PRIMARY KEY,
    description TEXT NOT NULL,
    installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    success BOOLEAN NOT NULL,
    checksum BLOB NOT NULL,
    execution_time BIGINT NOT NULL
);
                "#;

const MIGRATIONS: [(i64, &str, &str); 5] = [
    (1, "host monitoring", MIGRATION_0001),
    (2, "auth users", MIGRATION_0002),
    (3, "browser sessions", MIGRATION_0003),
    (4, "pairing admission", MIGRATION_0004),
    (5, "telemetry retention", MIGRATION_0005),
];

pub(super) const SOURCE_SCHEMA_SHA256: &str =
    "bcd52ab6e338c3e9cbe5aeba3da07e25ba12eb76d0998d648b231ee6604b3be8";
pub(super) const TARGET_SCHEMA_SHA256: &str = HOST_CURRENT_SCHEMA_SHA256;

pub(super) const ADAPTER: Adapter = Adapter {
    product: Product::HostMonitoring,
    from_version: "0.6.0",
    to_version: "0.7.0",
    // Host 0.6.0 declared schema revision 3 in its release manifest even
    // though its exact SQLx ledger contains five entries.
    source_revision: 3,
    source_schema_sha256: SOURCE_SCHEMA_SHA256,
    target_revision: 1,
    target_schema_sha256: TARGET_SCHEMA_SHA256,
    target_schema_sql: TARGET_SCHEMA_SQL,
    verify_ledger,
    copy_rows,
};

#[cfg(test)]
pub(super) const DATA_TABLES: [&str; 10] = [
    "monitored_hosts",
    "agent_metric_reports",
    "agent_credentials",
    "agent_instance_invites",
    "agent_pairing_requests",
    "audit_events",
    "auth_users",
    "auth_sessions",
    "auth_session_csrf_tokens",
    "agent_metric_hourly_aggregates",
];

fn verify_ledger(connection: &Connection) -> anyhow::Result<()> {
    let mut statement = connection.prepare(
        "SELECT version, description, typeof(installed_on), CAST(installed_on AS TEXT), \
                strftime('%Y-%m-%d %H:%M:%S', installed_on) = installed_on, \
                success, checksum, execution_time \
         FROM _sqlx_migrations ORDER BY version",
    )?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, Vec<u8>>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    ensure!(
        rows.len() == MIGRATIONS.len(),
        "Host 0.6 SQLx ledger must contain exactly five migrations"
    );
    for (row, (version, description, sql)) in rows.iter().zip(MIGRATIONS) {
        ensure!(
            row.0 == version
                && row.1 == description
                && row.2 == "text"
                && !row.3.is_empty()
                && row.4 == 1
                && row.5 == 1
                && row.6 == expected_migration_checksum(sql)
                && row.7 >= 0,
            "Host 0.6 SQLx migration {version} identity is invalid"
        );
    }
    Ok(())
}

fn copy_rows(connection: &Connection) -> anyhow::Result<()> {
    let transaction = connection.unchecked_transaction()?;
    let copies = [
        (
            "monitored_hosts",
            "host_id,name,os,os_version,kernel_version,arch,agent_version,capabilities,registered_at,last_seen_at,latest_report_id,latest_collected_at,latest_interval_seconds,lifecycle_status,revoked_at",
        ),
        (
            "agent_metric_reports",
            "report_id,host_id,schema_version,collected_at,received_at,interval_seconds,payload,cpu_usage_percent,memory_usage_percent,network_received_bytes_per_second,network_transmitted_bytes_per_second,disk_read_bytes_per_second,disk_written_bytes_per_second,max_temperature_celsius,gpu_utilization_percent,gpu_memory_usage_percent,aggregated_at",
        ),
        (
            "agent_credentials",
            "credential_id,host_id,token_hash,issued_at,last_used_at,revoked_at",
        ),
        (
            "agent_instance_invites",
            "invite_id,instance_id,activation_code_hash,display_name,status,expires_at,created_at,activated_at,cancelled_at",
        ),
        (
            "agent_pairing_requests",
            "request_id,requested_host_id,os,os_version,kernel_version,arch,agent_version,token_hash,polling_secret_hash,status,invite_id,instance_id,expires_at,created_at,activated_at",
        ),
        (
            "audit_events",
            "event_id,action,target,detail,actor,created_at",
        ),
        (
            "auth_users",
            "user_id,email,password_hash,active,created_at,session_version",
        ),
        (
            "auth_sessions",
            "session_id,user_id,token_hash,user_session_version,created_at,last_seen_at,idle_expires_at,absolute_expires_at,revoked_at",
        ),
        (
            "auth_session_csrf_tokens",
            "csrf_id,session_id,token_hash,created_at",
        ),
        (
            "agent_metric_hourly_aggregates",
            "host_id,bucket_start,interval_start,interval_end,sample_count,cpu_usage_percent_count,cpu_usage_percent_min,cpu_usage_percent_max,cpu_usage_percent_avg,memory_usage_percent_count,memory_usage_percent_min,memory_usage_percent_max,memory_usage_percent_avg,network_received_bytes_per_second_count,network_received_bytes_per_second_min,network_received_bytes_per_second_max,network_received_bytes_per_second_avg,network_transmitted_bytes_per_second_count,network_transmitted_bytes_per_second_min,network_transmitted_bytes_per_second_max,network_transmitted_bytes_per_second_avg,disk_read_bytes_per_second_count,disk_read_bytes_per_second_min,disk_read_bytes_per_second_max,disk_read_bytes_per_second_avg,disk_written_bytes_per_second_count,disk_written_bytes_per_second_min,disk_written_bytes_per_second_max,disk_written_bytes_per_second_avg,max_temperature_celsius_count,max_temperature_celsius_min,max_temperature_celsius_max,max_temperature_celsius_avg,gpu_utilization_percent_count,gpu_utilization_percent_min,gpu_utilization_percent_max,gpu_utilization_percent_avg,gpu_memory_usage_percent_count,gpu_memory_usage_percent_min,gpu_memory_usage_percent_max,gpu_memory_usage_percent_avg,updated_at",
        ),
    ];
    for (table, columns) in copies {
        transaction.execute_batch(&format!(
            "INSERT INTO main.{table} ({columns}) SELECT {columns} FROM legacy.{table};"
        ))?;
        let source_rows: i64 =
            transaction.query_row(&format!("SELECT COUNT(*) FROM legacy.{table}"), [], |row| {
                row.get(0)
            })?;
        let target_rows: i64 =
            transaction.query_row(&format!("SELECT COUNT(*) FROM main.{table}"), [], |row| {
                row.get(0)
            })?;
        ensure!(
            source_rows == target_rows,
            "row-count mismatch while copying {table}"
        );
    }

    let unexpected_sequences: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM legacy.sqlite_sequence \
         WHERE name NOT IN ('audit_events','auth_session_csrf_tokens')",
        [],
        |row| row.get(0),
    )?;
    ensure!(
        unexpected_sequences == 0,
        "Host 0.6 contains an unexpected AUTOINCREMENT sequence"
    );
    for (table, key) in [
        ("audit_events", "event_id"),
        ("auth_session_csrf_tokens", "csrf_id"),
    ] {
        let source_sequence: Option<i64> = transaction
            .query_row(
                "SELECT seq FROM legacy.sqlite_sequence WHERE name=?1",
                [table],
                |row| row.get(0),
            )
            .optional()?;
        let maximum: Option<i64> = transaction.query_row(
            &format!("SELECT MAX({key}) FROM legacy.{table}"),
            [],
            |row| row.get(0),
        )?;
        ensure!(
            match (source_sequence, maximum) {
                (Some(sequence), Some(maximum)) => sequence >= maximum,
                (Some(sequence), None) => sequence >= 0,
                (None, None) => true,
                (None, Some(_)) => false,
            },
            "Host 0.6 AUTOINCREMENT sequence for {table} is inconsistent"
        );
        transaction.execute("DELETE FROM main.sqlite_sequence WHERE name=?1", [table])?;
        if let Some(sequence) = source_sequence {
            transaction.execute(
                "INSERT INTO main.sqlite_sequence (name,seq) VALUES (?1,?2)",
                (table, sequence),
            )?;
        }
    }
    transaction.commit()?;
    Ok(())
}

#[cfg(test)]
pub(super) fn create_fixture(path: &std::path::Path) -> anyhow::Result<()> {
    let connection = Connection::open(path)?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.execute_batch(SQLX_LEDGER_DDL)?;
    for (version, description, sql) in MIGRATIONS {
        connection.execute_batch(sql)?;
        connection.execute(
            "INSERT INTO _sqlx_migrations \
             (version,description,success,checksum,execution_time) VALUES (?1,?2,1,?3,0)",
            (version, description, expected_migration_checksum(sql)),
        )?;
    }
    connection.execute_batch(
        "INSERT INTO monitored_hosts (
           host_id,name,os,arch,agent_version,registered_at,last_seen_at
         ) VALUES ('host-1','fixture','linux','x86_64','0.6.0','2026-01-01T00:00:00Z','2026-01-01T00:00:01Z');
         INSERT INTO agent_metric_reports (
           report_id,host_id,schema_version,collected_at,received_at,interval_seconds
         ) VALUES ('report-1','host-1',1,'2026-01-01T00:00:00Z','2026-01-01T00:00:01Z',1.0);
         INSERT INTO agent_credentials (
           credential_id,host_id,token_hash,issued_at
         ) VALUES ('credential-1','host-1','credential-hash','2026-01-01T00:00:00Z');
         INSERT INTO agent_instance_invites (
           invite_id,instance_id,activation_code_hash,display_name,status,expires_at,created_at
         ) VALUES ('invite-1','instance-1','activation-hash','fixture','active','2027-01-01T00:00:00Z','2026-01-01T00:00:00Z');
         INSERT INTO agent_pairing_requests (
           request_id,requested_host_id,os,arch,agent_version,token_hash,polling_secret_hash,status,invite_id,instance_id,expires_at,created_at,activated_at
         ) VALUES ('request-1','host-1','linux','x86_64','0.6.0','pair-token','poll-token','active','invite-1','instance-1','2027-01-01T00:00:00Z','2026-01-01T00:00:00Z','2026-01-01T00:00:01Z');
         INSERT INTO audit_events (
           event_id,action,target,detail,actor,created_at
         ) VALUES (7,'fixture','host-1','detail','test','2026-01-01T00:00:00Z');
         INSERT INTO auth_users (
           user_id,email,password_hash,created_at
         ) VALUES ('user-1','fixture@example.invalid','hash','2026-01-01T00:00:00Z');
         INSERT INTO auth_sessions (
           session_id,user_id,token_hash,user_session_version,created_at,last_seen_at,idle_expires_at,absolute_expires_at
         ) VALUES ('session-1','user-1','session-hash',1,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z','2026-01-02T00:00:00Z','2026-01-03T00:00:00Z');
         INSERT INTO auth_session_csrf_tokens (
           csrf_id,session_id,token_hash,created_at
         ) VALUES (9,'session-1','csrf-hash','2026-01-01T00:00:00Z');
         INSERT INTO agent_metric_hourly_aggregates (
           host_id,bucket_start,interval_start,interval_end,sample_count,
           cpu_usage_percent_count,memory_usage_percent_count,
           network_received_bytes_per_second_count,network_transmitted_bytes_per_second_count,
           disk_read_bytes_per_second_count,disk_written_bytes_per_second_count,
           max_temperature_celsius_count,gpu_utilization_percent_count,
           gpu_memory_usage_percent_count,updated_at
         ) VALUES (
           'host-1','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z','2026-01-01T00:59:59Z',1,
           0,0,0,0,0,0,0,0,0,'2026-01-01T01:00:00Z'
         );",
    )?;
    let fingerprint = super::schema_fingerprint_connection(&connection)?;
    ensure!(
        fingerprint == SOURCE_SCHEMA_SHA256,
        "Host fixture source fingerprint mismatch: expected {SOURCE_SCHEMA_SHA256}, got {fingerprint}"
    );
    connection.execute_batch("PRAGMA journal_mode=DELETE;")?;
    drop(connection);
    std::fs::File::open(path)
        .context("open Host fixture for sync")?
        .sync_all()?;
    Ok(())
}
