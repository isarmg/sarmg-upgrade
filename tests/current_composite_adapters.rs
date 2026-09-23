use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

use rusqlite::Connection;
use sarmg_upgrade::{
    CompositeCurrentOptions, CurrentRestoreOptions, NamedFile, Product, backup_current,
    restore_current, verify_current_backup,
};

fn database(
    path: &Path,
    schema: &str,
    product: &str,
    version: &str,
    revision: i64,
    fingerprint: &str,
) {
    let connection = Connection::open(path).unwrap();
    connection.execute_batch(schema).unwrap();
    connection
        .execute(
            "INSERT INTO product_metadata VALUES(1,?1,?2,?3,?4)",
            (product, version, revision, fingerprint),
        )
        .unwrap();
}

fn named(root: &Path, values: &[(&str, &str)]) -> Vec<NamedFile> {
    values
        .iter()
        .map(|(name, content)| {
            let path = root.join(name);
            fs::write(&path, content).unwrap();
            NamedFile {
                name: (*name).into(),
                path,
            }
        })
        .collect()
}

fn bind_dufs(database: &Path, tree: &Path) {
    let metadata = fs::metadata(tree).unwrap();
    let connection = Connection::open(database).unwrap();
    for (name, value) in [
        ("root-device-be", metadata.dev()),
        ("root-inode-be", metadata.ino()),
    ] {
        connection
            .execute(
                "INSERT INTO store_meta(key,value) VALUES(?1,?2)",
                (name, value.to_be_bytes().to_vec()),
            )
            .unwrap();
    }
}

fn backup_and_restore(
    product: Product,
    version: &str,
    revision: i64,
    schema: &str,
    fingerprint: &str,
    configuration: &[(&str, &str)],
    credentials: Option<(&str, [u8; 32])>,
) {
    let temporary = tempfile::tempdir().unwrap();
    let source_database = temporary.path().join("source.sqlite3");
    let source_tree = temporary.path().join("source-tree");
    fs::create_dir(&source_tree).unwrap();
    fs::write(source_tree.join("unicode-é-文件"), b"current-state").unwrap();
    database(
        &source_database,
        schema,
        product.slug(),
        version,
        revision,
        fingerprint,
    );
    if product == Product::DufsRam {
        bind_dufs(&source_database, &source_tree);
    }
    let source_configuration = named(temporary.path(), configuration);
    let output = temporary.path().join("backup");
    let (credentials_key_id, credentials_key) = credentials
        .map(|(id, key)| (Some(id.to_owned()), Some(key)))
        .unwrap_or_default();
    let options = CompositeCurrentOptions {
        product,
        database: source_database,
        tree: source_tree,
        output: output.clone(),
        runtime_directory: None,
        configuration: source_configuration,
        credentials_key_id: credentials_key_id.clone(),
        credentials_key,
    };
    backup_current(&options).unwrap();
    let backup_configuration = configuration
        .iter()
        .map(|(name, _)| NamedFile {
            name: (*name).into(),
            path: output.join(name),
        })
        .collect();
    verify_current_backup(&CompositeCurrentOptions {
        database: output.join("database.sqlite3"),
        tree: output.join("tree"),
        configuration: backup_configuration,
        ..options.clone()
    })
    .unwrap();

    let restore_root = temporary.path().join("restore-config");
    fs::create_dir(&restore_root).unwrap();
    let restore_configuration = configuration
        .iter()
        .map(|(name, _)| NamedFile {
            name: (*name).into(),
            path: restore_root.join(name),
        })
        .collect();
    let destination = temporary.path().join("restored.sqlite3");
    let destination_tree = temporary.path().join("restored-tree");
    restore_current(&CurrentRestoreOptions {
        product,
        input: output,
        database: destination.clone(),
        tree: destination_tree.clone(),
        runtime_directory: None,
        configuration: restore_configuration,
        replace_existing: false,
        credentials_key_id,
        credentials_key,
    })
    .unwrap();
    assert!(destination.is_file());
    if product == Product::DufsRam {
        let stored: Vec<u8> = Connection::open(&destination)
            .unwrap()
            .query_row(
                "SELECT value FROM store_meta WHERE key='root-inode-be'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            stored,
            fs::metadata(&destination_tree).unwrap().ino().to_be_bytes()
        );
    }
    assert_eq!(
        fs::read(destination_tree.join("unicode-é-文件")).unwrap(),
        b"current-state"
    );
    for (name, content) in configuration {
        assert_eq!(
            fs::read(restore_root.join(name)).unwrap(),
            content.as_bytes()
        );
    }
}

#[test]
fn sentinel_current_adapter_backs_up_verifies_and_restores_composite_state() {
    backup_and_restore(
        Product::SentinelMonitor,
        "0.2.2",
        7,
        include_str!("fixtures/current/sentinel-monitor.sql"),
        "bb64805d1434fa953b5a215c636c086d98bce467825f7e9b6d3a5c1c0bd359c4",
        &[
            ("sentinel.env", "CURRENT=1"),
            ("mediamtx.yml", "record: yes"),
            ("mediamtx.lock", "sha256=current"),
        ],
        Some(("primary", [7; 32])),
    );
}

#[test]
fn media_current_adapter_backs_up_verifies_and_restores_composite_state() {
    backup_and_restore(
        Product::MediaBackup,
        "0.3.0",
        5,
        include_str!("fixtures/current/media-backup.sql"),
        "a07c5723568cfcbf379a2173225122dc5db4e2168a50700d7f256aba3de5957e",
        &[],
        None,
    );
}

#[test]
fn dufs_current_adapter_backs_up_verifies_and_restores_composite_state() {
    backup_and_restore(
        Product::DufsRam,
        "0.51.0",
        1,
        include_str!("fixtures/current/dufs-ram.sql"),
        "3659ff0c703515f555af95f0f1c08c35fa0555a8978f5f0e5a658fd93d225423",
        &[("dufs.yaml", "auth:\n  - admin:current")],
        None,
    );
}

#[test]
fn composite_adapter_rejects_wrong_resource_sets() {
    let root = PathBuf::from("/tmp");
    let result = backup_current(&CompositeCurrentOptions {
        product: Product::DufsRam,
        database: root.join("missing.sqlite3"),
        tree: root.join("missing-tree"),
        output: root.join("missing-output"),
        runtime_directory: None,
        configuration: Vec::new(),
        credentials_key_id: None,
        credentials_key: None,
    });
    assert!(result.unwrap_err().to_string().contains("dufs.yaml"));
}

#[test]
fn composite_restore_replaces_configuration_with_the_same_generation() {
    let temporary = tempfile::tempdir().unwrap();
    let schema = include_str!("fixtures/current/dufs-ram.sql");
    let fingerprint = "3659ff0c703515f555af95f0f1c08c35fa0555a8978f5f0e5a658fd93d225423";
    let source_database = temporary.path().join("source.sqlite3");
    let source_tree = temporary.path().join("source-tree");
    let source_configuration = temporary.path().join("source-dufs.yaml");
    let backup = temporary.path().join("backup");
    fs::create_dir(&source_tree).unwrap();
    database(
        &source_database,
        schema,
        Product::DufsRam.slug(),
        "0.51.0",
        1,
        fingerprint,
    );
    bind_dufs(&source_database, &source_tree);
    fs::write(source_tree.join("state"), b"incoming").unwrap();
    fs::write(&source_configuration, b"auth: incoming").unwrap();
    backup_current(&CompositeCurrentOptions {
        product: Product::DufsRam,
        database: source_database,
        tree: source_tree,
        output: backup.clone(),
        runtime_directory: None,
        configuration: vec![NamedFile {
            name: "dufs.yaml".into(),
            path: source_configuration,
        }],
        credentials_key_id: None,
        credentials_key: None,
    })
    .unwrap();

    let destination = temporary.path().join("destination.sqlite3");
    let destination_tree = temporary.path().join("destination-tree");
    let destination_configuration = temporary.path().join("dufs.yaml");
    fs::create_dir(&destination_tree).unwrap();
    database(
        &destination,
        schema,
        Product::DufsRam.slug(),
        "0.51.0",
        1,
        fingerprint,
    );
    bind_dufs(&destination, &destination_tree);
    fs::write(destination_tree.join("state"), b"original").unwrap();
    fs::write(&destination_configuration, b"auth: original").unwrap();
    restore_current(&CurrentRestoreOptions {
        product: Product::DufsRam,
        input: backup,
        database: destination,
        tree: destination_tree.clone(),
        runtime_directory: None,
        configuration: vec![NamedFile {
            name: "dufs.yaml".into(),
            path: destination_configuration.clone(),
        }],
        replace_existing: true,
        credentials_key_id: None,
        credentials_key: None,
    })
    .unwrap();
    assert_eq!(
        fs::read(destination_tree.join("state")).unwrap(),
        b"incoming"
    );
    assert_eq!(
        fs::read(destination_configuration).unwrap(),
        b"auth: incoming"
    );
}
