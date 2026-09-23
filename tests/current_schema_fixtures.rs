use rusqlite::Connection;
use sarmg_schema_identity::{SQLITE_SCHEMA_ROWS_QUERY, SchemaRow, schema_fingerprint};

#[test]
fn server_schema_snapshots_match_the_pinned_current_contracts() {
    let fixtures = [
        (
            "host-monitoring",
            include_str!("fixtures/current/host-monitoring.sql"),
            "5c4a32f3f1813e6e6ef528b55e25e912bfe0191f79332ad5538943746c8f17a3",
        ),
        (
            "sunshine-manager",
            include_str!("fixtures/current/sunshine-manager.sql"),
            "1acc8f2d9fac7ec4e973dd7e43cf5099e4a0b713b58a59e4969797602030d5d2",
        ),
        (
            "media-backup",
            include_str!("fixtures/current/media-backup.sql"),
            "a07c5723568cfcbf379a2173225122dc5db4e2168a50700d7f256aba3de5957e",
        ),
        (
            "sentinel-monitor",
            include_str!("fixtures/current/sentinel-monitor.sql"),
            "bb64805d1434fa953b5a215c636c086d98bce467825f7e9b6d3a5c1c0bd359c4",
        ),
        (
            "dufs-ram",
            include_str!("fixtures/current/dufs-ram.sql"),
            "3659ff0c703515f555af95f0f1c08c35fa0555a8978f5f0e5a658fd93d225423",
        ),
    ];
    for (product, schema, expected) in fixtures {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(schema).unwrap();
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
        assert_eq!(schema_fingerprint(&rows).unwrap(), expected, "{product}");
    }
}
