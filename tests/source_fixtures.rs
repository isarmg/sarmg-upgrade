use rusqlite::Connection;
use sarmg_schema_identity::{SQLITE_SCHEMA_ROWS_QUERY, SchemaRow, schema_fingerprint};

struct Fixture {
    application: &'static str,
    version: &'static str,
    fingerprint: &'static str,
    schema: &'static str,
    seed: &'static str,
    administrator_table: &'static str,
    session_table: &'static str,
    business_table: &'static str,
    audit_table: Option<&'static str>,
}

fn fixtures() -> [Fixture; 4] {
    [
        Fixture {
            application: "dufs-ram",
            version: "0.50.1",
            fingerprint: "3659ff0c703515f555af95f0f1c08c35fa0555a8978f5f0e5a658fd93d225423",
            schema: include_str!("fixtures/sources/dufs-ram/0.50.1/database.sql"),
            seed: include_str!("fixtures/sources/dufs-ram/0.50.1/seed.sql"),
            administrator_table: "",
            session_table: "",
            business_table: "store_meta",
            audit_table: None,
        },
        Fixture {
            application: "host-monitoring",
            version: "0.8.0",
            fingerprint: "12dd1e61426b6b99df3d429b8c36ee3a5b22d1da776d98fc960b45b4f58c8e05",
            schema: include_str!("fixtures/sources/host-monitoring/0.8.0/database.sql"),
            seed: include_str!("fixtures/sources/host-monitoring/0.8.0/seed.sql"),
            administrator_table: "auth_users",
            session_table: "auth_sessions",
            business_table: "monitored_hosts",
            audit_table: Some("audit_events"),
        },
        Fixture {
            application: "media-backup",
            version: "0.2.0",
            fingerprint: "2563e6afc3fff272d02b7a5615272cc773862243bfd15aec51655abf1d9c6b1c",
            schema: include_str!("fixtures/sources/media-backup/0.2.0/database.sql"),
            seed: include_str!("fixtures/sources/media-backup/0.2.0/seed.sql"),
            administrator_table: "auth_users",
            session_table: "auth_sessions",
            business_table: "assets",
            audit_table: Some("audit_events"),
        },
        Fixture {
            application: "sentinel-monitor",
            version: "0.2.0",
            fingerprint: "f547ddc817d830d23b5305bb1f88b29898d6531568edd6eb194c2b629eb560c0",
            schema: include_str!("fixtures/sources/sentinel-monitor/0.2.0/database.sql"),
            seed: include_str!("fixtures/sources/sentinel-monitor/0.2.0/seed.sql"),
            administrator_table: "users",
            session_table: "browser_sessions",
            business_table: "cameras",
            audit_table: Some("audit_logs"),
        },
    ]
}

fn row_count(connection: &Connection, table: &str) -> i64 {
    connection
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

#[test]
fn current_source_fixtures_are_executable_sanitized_and_exact() {
    for fixture in fixtures() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(fixture.schema).unwrap();
        connection.execute_batch(fixture.seed).unwrap();

        let identity: (String, String, i64, String) = connection
            .query_row(
                "SELECT application, application_version, schema_revision, schema_sha256
                 FROM product_metadata WHERE singleton = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(identity.0, fixture.application);
        assert_eq!(identity.1, fixture.version);
        assert_eq!(identity.2, 1);
        assert_eq!(identity.3, fixture.fingerprint);

        let mut statement = connection.prepare(SQLITE_SCHEMA_ROWS_QUERY).unwrap();
        let rows = statement
            .query_map([], |row| {
                Ok(SchemaRow::new(
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(schema_fingerprint(&rows).unwrap(), fixture.fingerprint);
        assert_eq!(row_count(&connection, fixture.business_table), 1);
        if !fixture.administrator_table.is_empty() {
            assert_eq!(row_count(&connection, fixture.administrator_table), 1);
            assert_eq!(row_count(&connection, fixture.session_table), 1);
        }
        if let Some(table) = fixture.audit_table {
            assert_eq!(row_count(&connection, table), 1);
        }
        let foreign_key_errors: i64 = connection
            .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(foreign_key_errors, 0);
    }
}

#[test]
fn dufs_static_administrator_fixture_records_restart_invalidation() {
    let authentication = include_str!("fixtures/sources/dufs-ram/0.50.1/auth.yaml");
    let session = include_str!("fixtures/sources/dufs-ram/0.50.1/browser-session.json");
    assert!(authentication.contains("$argon2id$v=19$m=19456,t=2,p=1$"));
    let session: serde_json::Value = serde_json::from_str(session).unwrap();
    assert_eq!(session["persistence"], "memory-only");
    assert_eq!(session["invalidated_by_restart"], true);
}
