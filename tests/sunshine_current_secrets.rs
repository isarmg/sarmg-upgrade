use std::path::Path;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use rusqlite::Connection;
use sarmg_secret::{SecretBytes, SecretKey};
use sarmg_secret_envelope::EnvelopeDomain;
use sarmg_upgrade::{Product, create_sqlite_backup_with_credentials};
use sha2::{Digest, Sha256};

struct ClientAuthorization;
impl EnvelopeDomain for ClientAuthorization {
    const DOMAIN: &'static [u8] = b"sunshine-manager/client-authorization";
    const REVISION: u16 = 1;
}

struct OperationRequest;
impl EnvelopeDomain for OperationRequest {
    const DOMAIN: &'static [u8] = b"sunshine-manager/operation-request";
    const REVISION: u16 = 1;
}

fn operation_binding(id: &str, action: &str) -> Vec<u8> {
    let mut binding = Vec::new();
    for value in [
        b"sunshine-manager:aes-256-gcm:aad:v1".as_slice(),
        b"operation-request",
        id.as_bytes(),
        action.as_bytes(),
        b"request_ciphertext",
    ] {
        binding.extend_from_slice(&(value.len() as u64).to_be_bytes());
        binding.extend_from_slice(value);
    }
    binding
}

fn current_database(path: &Path, key: [u8; 32]) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch(include_str!("fixtures/current/sunshine-manager.sql"))
        .unwrap();
    connection
        .execute(
            "INSERT INTO product_metadata VALUES(1,'sunshine-manager','0.10.1',7,?1)",
            ["1acc8f2d9fac7ec4e973dd7e43cf5099e4a0b713b58a59e4969797602030d5d2"],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO _sarmg_platform_metadata VALUES(1,1,1,'server-control-plane',1)",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO manager_identity VALUES(1,'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa')",
            [],
        )
        .unwrap();
    let id = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    let code = "a1".repeat(18);
    let payload = sarmg_secret_envelope::seal::<ClientAuthorization>(
        &SecretKey::new(key),
        id.as_bytes(),
        &SecretBytes::new(code.as_bytes().to_vec()),
    )
    .unwrap();
    let encrypted = format!("sunshine:sgev1:primary:{}", STANDARD.encode(payload));
    connection.execute(
        "INSERT INTO devices(device_id,name,authorization_code_enc,enrollment_hash,created_at_micros,updated_at_micros) VALUES(?1,'Client',?2,?3,1,1)",
        (id, encrypted, Sha256::digest(code.as_bytes()).to_vec()),
    ).unwrap();
    let operation_id = "operation-1";
    let action = "sunshine.config.patch";
    let request = r#"{"binding":{},"command":{}}"#;
    let sealed = sarmg_secret_envelope::seal::<OperationRequest>(
        &SecretKey::new(key),
        &operation_binding(operation_id, action),
        &SecretBytes::new(request.as_bytes().to_vec()),
    )
    .unwrap();
    let payload = serde_json::json!({"actor":"operator","request_ciphertext": format!("sunshine:sgev1:primary:{}", STANDARD.encode(sealed))});
    let derivation = Hkdf::<Sha256>::new(
        Some(b"sunshine-manager:credential-master-key:hkdf-sha256:v1"),
        &key,
    );
    let mut fingerprint_key = [0_u8; 32];
    derivation
        .expand(
            b"sunshine-manager:operation-request-fingerprint:hmac-sha256:v1",
            &mut fingerprint_key,
        )
        .unwrap();
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&fingerprint_key).unwrap();
    mac.update(request.as_bytes());
    connection.execute(
        "INSERT INTO _sarmg_operations(operation_id,namespace,target_key,action,idempotency_digest,request_fingerprint,request_payload,state,attempt,max_attempts,not_before_micros,created_at_micros,updated_at_micros) VALUES(?1,'sunshine',?2,?3,?4,?5,?6,'pending',0,3,1,1,1)",
        rusqlite::params![operation_id,id,action,vec![1_u8;32],mac.finalize().into_bytes().to_vec(),serde_json::to_vec(&payload).unwrap()],
    ).unwrap();
}

#[test]
fn sunshine_current_backup_authenticates_the_actual_client_envelope() {
    let root = tempfile::tempdir().unwrap();
    let database = root.path().join("sunshine.sqlite3");
    current_database(&database, [7; 32]);
    assert!(
        create_sqlite_backup_with_credentials(
            Product::SunshineManager,
            &database,
            &root.path().join("wrong"),
            "primary",
            &[8; 32],
        )
        .is_err()
    );
    assert!(
        create_sqlite_backup_with_credentials(
            Product::SunshineManager,
            &database,
            &root.path().join("wrong-id"),
            "other",
            &[7; 32],
        )
        .is_err()
    );
    create_sqlite_backup_with_credentials(
        Product::SunshineManager,
        &database,
        &root.path().join("backup"),
        "primary",
        &[7; 32],
    )
    .unwrap();
    Connection::open(&database)
        .unwrap()
        .execute("DELETE FROM manager_identity", [])
        .unwrap();
    assert!(
        create_sqlite_backup_with_credentials(
            Product::SunshineManager,
            &database,
            &root.path().join("missing-manager"),
            "primary",
            &[7; 32],
        )
        .is_err()
    );
    Connection::open(&database)
        .unwrap()
        .execute(
            "INSERT INTO manager_identity VALUES(1,'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa')",
            [],
        )
        .unwrap();
    Connection::open(&database).unwrap().execute(
        "UPDATE _sarmg_operations SET action='sunshine.restart' WHERE operation_id='operation-1'", [],
    ).unwrap();
    assert!(
        create_sqlite_backup_with_credentials(
            Product::SunshineManager,
            &database,
            &root.path().join("tampered"),
            "primary",
            &[7; 32],
        )
        .is_err()
    );
}
