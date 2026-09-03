use std::{
    fs,
    path::{Path, PathBuf},
};

use rusqlite::Connection;
use sarmg_upgrade::{
    CompositeCurrentOptions, CurrentRestoreOptions, NamedFile, Product, backup_current,
    restore_current, verify_current_backup,
};

fn database(path: &Path, schema: &str, product: &str, version: &str, fingerprint: &str) {
    let connection = Connection::open(path).unwrap();
    connection.execute_batch(schema).unwrap();
    connection
        .execute(
            "INSERT INTO product_metadata VALUES(1,?1,?2,1,?3)",
            (product, version, fingerprint),
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

fn backup_and_restore(
    product: Product,
    version: &str,
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
        fingerprint,
    );
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
        "0.2.0",
        include_str!("fixtures/sources/sentinel-monitor/0.2.0/database.sql"),
        "f547ddc817d830d23b5305bb1f88b29898d6531568edd6eb194c2b629eb560c0",
        &[
            ("sentinel.env", "CURRENT=1"),
            ("mediamtx.yml", "record: yes"),
            ("mediamtx.lock", "sha256=current"),
        ],
        Some(("sentinel-credentials-0.2.0-key-1", [7; 32])),
    );
}

#[test]
fn dufs_current_adapter_backs_up_verifies_and_restores_composite_state() {
    backup_and_restore(
        Product::DufsRam,
        "0.50.1",
        include_str!("fixtures/sources/dufs-ram/0.50.1/database.sql"),
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
    let schema = include_str!("fixtures/sources/dufs-ram/0.50.1/database.sql");
    let fingerprint = "3659ff0c703515f555af95f0f1c08c35fa0555a8978f5f0e5a658fd93d225423";
    let source_database = temporary.path().join("source.sqlite3");
    let source_tree = temporary.path().join("source-tree");
    let source_configuration = temporary.path().join("source-dufs.yaml");
    let backup = temporary.path().join("backup");
    database(
        &source_database,
        schema,
        Product::DufsRam.slug(),
        "0.50.1",
        fingerprint,
    );
    fs::create_dir(&source_tree).unwrap();
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
    database(
        &destination,
        schema,
        Product::DufsRam.slug(),
        "0.50.1",
        fingerprint,
    );
    fs::create_dir(&destination_tree).unwrap();
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
