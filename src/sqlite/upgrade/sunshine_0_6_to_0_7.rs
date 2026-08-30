#[cfg(test)]
use anyhow::Context;
use anyhow::ensure;
use rusqlite::{Connection, OptionalExtension};

use super::{Adapter, Product, expected_migration_checksum};

const MIGRATION_0001: &str =
    include_str!("../../upgrades/sunshine_0_6_to_0_7/202608270001_initial.sql");
const MIGRATION_0002: &str =
    include_str!("../../upgrades/sunshine_0_6_to_0_7/202608290001_auth_users.sql");
const MIGRATION_0003: &str =
    include_str!("../../upgrades/sunshine_0_6_to_0_7/202608290002_auth_sessions.sql");
const MIGRATION_0004: &str =
    include_str!("../../upgrades/sunshine_0_6_to_0_7/202608290003_persistent_operations.sql");
const TARGET_SCHEMA_SQL: &str = include_str!("../../upgrades/sunshine_0_6_to_0_7/target.sql");

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

const MIGRATIONS: [(i64, &str, &str); 4] = [
    (202608270001, "initial", MIGRATION_0001),
    (202608290001, "auth users", MIGRATION_0002),
    (202608290002, "auth sessions", MIGRATION_0003),
    (202608290003, "persistent operations", MIGRATION_0004),
];

pub(super) const SOURCE_SCHEMA_SHA256: &str =
    "bcbde7b2f8589c19ec3b8ba92b1e73a6d8ce91cfc708769dc1b48b38b6df4e09";
pub(super) const TARGET_SCHEMA_SHA256: &str =
    "1e55653f9b9b4805873164e52b79d399aec4fe327a8648218d4cbcb16b561b98";

pub(super) const ADAPTER: Adapter = Adapter {
    product: Product::SunshineManager,
    from_version: "0.6.0",
    to_version: "0.7.0",
    // The 0.6.0 release manifest declares revision 3; the four exact SQLx
    // ledger rows remain independently mandatory below.
    source_revision: 3,
    source_schema_sha256: SOURCE_SCHEMA_SHA256,
    target_revision: 1,
    target_schema_sha256: TARGET_SCHEMA_SHA256,
    target_schema_sql: TARGET_SCHEMA_SQL,
    verify_ledger,
    copy_rows,
};

#[cfg(test)]
pub(super) const DATA_TABLES: [&str; 6] = [
    "hosts",
    "audit_logs",
    "auth_users",
    "auth_sessions",
    "operations",
    "audit_outbox",
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
        "Sunshine 0.6 SQLx ledger must contain exactly four migrations"
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
            "Sunshine 0.6 SQLx migration {version} identity is invalid"
        );
    }
    Ok(())
}

fn copy_rows(connection: &Connection) -> anyhow::Result<()> {
    let transaction = connection.unchecked_transaction()?;
    let copies = [
        (
            "hosts",
            "host_id,name,address,web_port,username,secret,verify_tls,position,created_at_micros,updated_at_micros",
        ),
        (
            "audit_logs",
            "audit_id,action,target,detail,actor,created_at_micros,outbox_id",
        ),
        (
            "auth_users",
            "user_id,email,password_hash,active,created_at_micros,session_version",
        ),
        (
            "auth_sessions",
            "session_id,user_id,token_hash,csrf_hash,user_session_version,created_at_micros,last_seen_at_micros,idle_expires_at_micros,absolute_expires_at_micros,revoked_at_micros",
        ),
        (
            "operations",
            "operation_id,actor,host_id,action,idempotency_key_hash,request_fingerprint,request_ciphertext,state,attempt,created_at_micros,updated_at_micros,started_at_micros,completed_at_micros,error_code",
        ),
        (
            "audit_outbox",
            "outbox_id,operation_id,event_kind,action,target,actor,detail,created_at_micros,delivered_at_micros,delivery_attempt",
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
        "SELECT COUNT(*) FROM legacy.sqlite_sequence WHERE name <> 'audit_logs'",
        [],
        |row| row.get(0),
    )?;
    ensure!(
        unexpected_sequences == 0,
        "Sunshine 0.6 contains an unexpected AUTOINCREMENT sequence"
    );
    let source_sequence: Option<i64> = transaction
        .query_row(
            "SELECT seq FROM legacy.sqlite_sequence WHERE name='audit_logs'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let maximum: Option<i64> =
        transaction.query_row("SELECT MAX(audit_id) FROM legacy.audit_logs", [], |row| {
            row.get(0)
        })?;
    ensure!(
        match (source_sequence, maximum) {
            (Some(sequence), Some(maximum)) => sequence >= maximum,
            (Some(sequence), None) => sequence >= 0,
            (None, None) => true,
            (None, Some(_)) => false,
        },
        "Sunshine 0.6 audit sequence is inconsistent"
    );
    transaction.execute(
        "DELETE FROM main.sqlite_sequence WHERE name='audit_logs'",
        [],
    )?;
    if let Some(sequence) = source_sequence {
        transaction.execute(
            "INSERT INTO main.sqlite_sequence (name,seq) VALUES ('audit_logs',?1)",
            [sequence],
        )?;
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
        "INSERT INTO hosts (
           host_id,name,address,web_port,username,secret,verify_tls,position,created_at_micros,updated_at_micros
         ) VALUES ('host-1','fixture','sunshine.invalid',47990,'user','sunshine:v1:key:AAECAwQFBgcICQoLN9AFP7iPL2p+9oDUU1Zq8CO+rSApePz7',1,0,1,2);
         INSERT INTO audit_logs (
           audit_id,action,target,detail,actor,created_at_micros,outbox_id
         ) VALUES (7,'fixture','host-1','detail','test',3,'outbox-1');
         INSERT INTO auth_users (
           user_id,email,password_hash,active,created_at_micros,session_version
         ) VALUES ('user-1','fixture@example.invalid','hash',1,4,1);
         INSERT INTO auth_sessions (
           session_id,user_id,token_hash,csrf_hash,user_session_version,created_at_micros,last_seen_at_micros,idle_expires_at_micros,absolute_expires_at_micros
         ) VALUES ('session-1','user-1',zeroblob(32),randomblob(32),1,5,6,7,8);
         INSERT INTO operations (
           operation_id,actor,host_id,action,idempotency_key_hash,request_fingerprint,request_ciphertext,state,attempt,created_at_micros,updated_at_micros
         ) VALUES (
           'operation-1','test','host-1','sunshine.system.restart',
           x'66e7c82b49bb291dd09c8e020448311c4a7bb96aeb5c5db769f66812b13a50b5',
           x'2a52f16cc4f4cfc7e423ffb0442c8059f6991008ad3f5bf90caa057d736b6d5b',
           'sunshine:v1:key:DA0ODxAREhMUFRYXzelF/frJB/aP4aKoi77qA1z/DWq77USVscNuhWsdS+rckg==',
           'pending',0,9,10
         );
         INSERT INTO audit_outbox (
           outbox_id,operation_id,event_kind,action,target,actor,detail,created_at_micros,delivery_attempt
         ) VALUES ('outbox-1','operation-1','requested','sunshine.system.restart.requested','host-1','test','detail',11,0);",
    )?;
    let fingerprint = super::schema_fingerprint_connection(&connection)?;
    ensure!(
        fingerprint == SOURCE_SCHEMA_SHA256,
        "Sunshine fixture source fingerprint mismatch: expected {SOURCE_SCHEMA_SHA256}, got {fingerprint}"
    );
    connection.execute_batch("PRAGMA journal_mode=DELETE;")?;
    drop(connection);
    std::fs::File::open(path)
        .context("open Sunshine fixture for sync")?
        .sync_all()?;
    Ok(())
}
