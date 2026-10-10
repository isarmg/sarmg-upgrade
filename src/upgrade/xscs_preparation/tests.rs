use super::*;

const DEVICE: &str = "11111111-1111-4111-8111-111111111111";
const FROZEN_SCHEMA: &str = include_str!("../../../tests/fixtures/xscs-0.15.0-schema.sql");

fn capabilities() -> String {
    serde_json::json!({
        "protocol":"xscs-management/3", "client_version":"0.3.5",
        "os":"linux_x86_64", "sunshine_version":"2026.914.233613",
        "restart_allowed":true, "managed_fields":["fps"],
        "application_management":true, "application_host_commands_allowed":false,
        "moonlight_pairing_management":true, "diagnostics":true,
        "maintenance":true, "service_control":true
    })
    .to_string()
}

async fn fixture(path: &Path, observation: &str) -> anyhow::Result<()> {
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true),
    )
    .await?;
    sqlx::raw_sql(FROZEN_SCHEMA)
        .execute(&mut connection)
        .await?;
    sqlx::query("INSERT INTO product_metadata VALUES(1,'xscs','0.10.1',7,?)")
        .bind(SCHEMA_SHA)
        .execute(&mut connection)
        .await?;
    sqlx::query("INSERT INTO devices(device_id,name,authorization_code_enc,session_id,last_seen_at_micros,health_at_micros,sunshine_reachable,capabilities_json,snapshot_json,saved_revision,created_at_micros,updated_at_micros) VALUES(?,'Preserved',?,'old-session',10,11,1,?,'{\"business\":\"snapshot\"}','saved',1,2)")
        .bind(DEVICE).bind("s".repeat(80)).bind(observation).execute(&mut connection).await?;
    sqlx::query("INSERT INTO audit_logs(action,target,detail,actor,created_at_micros) VALUES('unchanged',?,'audit-detail','admin',12)")
        .bind(DEVICE).execute(&mut connection).await?;
    sqlx::query("INSERT INTO _xcss_operations(operation_id,namespace,target_key,action,idempotency_digest,request_fingerprint,request_payload,state,attempt,max_attempts,not_before_micros,created_at_micros,updated_at_micros) VALUES('op_fixture','xscs',?,'task',?,?,?,'unknown',1,3,0,1,2)")
        .bind(DEVICE).bind(vec![1u8; 32]).bind(vec![2u8; 32])
        .bind(b"unchanged task contract/fingerprint".as_slice()).execute(&mut connection).await?;
    connection.close().await?;
    Ok(())
}

#[test]
fn exact_old_observation_shape_is_required_without_inventing_new_flags() {
    validate_old_capabilities(&capabilities()).unwrap();
    for mutation in ["protocol", "restart_allowed", "configuration_overwrite"] {
        let mut value: serde_json::Value = serde_json::from_str(&capabilities()).unwrap();
        match mutation {
            "protocol" => value["protocol"] = "xscs-management/99".into(),
            "restart_allowed" => value
                .as_object_mut()
                .unwrap()
                .remove("restart_allowed")
                .map(|_| ())
                .unwrap(),
            _ => value["configuration_overwrite"] = true.into(),
        }
        assert!(validate_old_capabilities(&value.to_string()).is_err());
    }
    assert!(
        validate_old_capabilities(
            &capabilities().replace("\"restart_allowed\":true", "\"restart_allowed\":\"yes\"")
        )
        .is_err()
    );
    let duplicate = capabilities().replacen('{', "{\"protocol\":\"xscs-management/3\",", 1);
    assert!(validate_old_capabilities(&duplicate).is_err());

    // The upstream service name is a third-party proper noun, not this tool's
    // canonical project protocol namespace. Earlier project naming is rejected.
    let managed_service = "Sunshine";
    let mut former_name: serde_json::Value = serde_json::from_str(&capabilities()).unwrap();
    former_name["protocol"] =
        format!("{}-management/3", managed_service.to_ascii_lowercase()).into();
    assert!(validate_old_capabilities(&former_name.to_string()).is_err());
}

#[test]
fn transaction_invalidates_only_observations_and_is_repeatable() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("xscs.sqlite3");
    xcsc::sqlite::block_on_sqlite_connection(async {
        fixture(&path, &capabilities()).await?;
        assert_eq!(invalidate_cache(&path).await?, 1);
        assert_eq!(invalidate_cache(&path).await?, 0);
        let mut connection =
            SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path)).await?;
        let row = sqlx::query("SELECT * FROM devices")
            .fetch_one(&mut connection)
            .await?;
        for field in ["capabilities_json", "session_id"] {
            assert_eq!(row.try_get::<Option<String>, _>(field)?, None);
        }
        for field in [
            "last_seen_at_micros",
            "health_at_micros",
            "sunshine_reachable",
        ] {
            assert_eq!(row.try_get::<Option<i64>, _>(field)?, None);
        }
        assert_eq!(
            row.try_get::<String, _>("authorization_code_enc")?,
            "s".repeat(80)
        );
        assert_eq!(
            row.try_get::<String, _>("snapshot_json")?,
            "{\"business\":\"snapshot\"}"
        );
        assert_eq!(row.try_get::<String, _>("saved_revision")?, "saved");
        assert_eq!(row.try_get::<i64, _>("updated_at_micros")?, 2);
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT detail FROM audit_logs")
                .fetch_one(&mut connection)
                .await?,
            "audit-detail"
        );
        let operation =
            sqlx::query("SELECT request_fingerprint,request_payload,state FROM _xcss_operations")
                .fetch_one(&mut connection)
                .await?;
        assert_eq!(
            operation.try_get::<Vec<u8>, _>("request_fingerprint")?,
            vec![2u8; 32]
        );
        assert_eq!(
            operation.try_get::<Vec<u8>, _>("request_payload")?,
            b"unchanged task contract/fingerprint"
        );
        assert_eq!(operation.try_get::<String, _>("state")?, "unknown");
        connection.close().await?;
        Ok(())
    })
}

#[test]
fn malformed_or_unknown_source_leaves_all_rows_unchanged() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("xscs.sqlite3");
    xcsc::sqlite::block_on_sqlite_connection(async {
        fixture(&path, "{\"protocol\":\"xscs-management/3\"}").await?;
        assert!(invalidate_cache(&path).await.is_err());
        let mut connection =
            SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path)).await?;
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT session_id FROM devices")
                .fetch_one(&mut connection)
                .await?,
            "old-session"
        );
        sqlx::query("UPDATE product_metadata SET schema_revision=8")
            .execute(&mut connection)
            .await?;
        connection.close().await?;
        assert!(invalidate_cache(&path).await.is_err());
        let mut connection =
            SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path)).await?;
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT sunshine_reachable FROM devices")
                .fetch_one(&mut connection)
                .await?,
            1
        );
        connection.close().await?;
        Ok(())
    })
}

#[cfg(target_os = "linux")]
fn preparation_fixture(observation: &str) -> anyhow::Result<(tempfile::TempDir, PreparationPlan)> {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir()?;
    let data_dir = root.path().join("data");
    snapshot::private_directory(&data_dir)?;
    let database = data_dir.join("xscs.sqlite3");
    xcsc::sqlite::block_on_sqlite_connection(fixture(&database, observation))?;
    fs::set_permissions(&database, fs::Permissions::from_mode(0o600))?;
    let config = root.path().join("config.json");
    fs::write(&config, b"{}")?;
    fs::set_permissions(&config, fs::Permissions::from_mode(0o600))?;
    let identity = ReleaseIdentity {
        product: PRODUCT.into(),
        version: SOURCE_VERSION.into(),
        source_revision: "a".repeat(40),
        target: "x86_64-unknown-linux-gnu".into(),
        state_contract_sha256: "b".repeat(64),
    };
    // This frozen source-interface fixture is deliberately not claimed to be
    // an actual signed xscs release or evidence of systemd deployment.
    let source_binary = root.path().join("source-fixture");
    fs::write(
        &source_binary,
        format!(
            "#!/bin/bash\nif [[ \"$1\" == release-identity ]]; then\ncat <<'IDENTITY'\n{}\nIDENTITY\nelse\ncat <<'STATE'\n{}\nSTATE\nfi\n",
            serde_json::to_string(&identity)?,
            serde_json::json!({"status":"valid", "schema_identity":schema_identity()?, "state_paths":[data_dir,config]})
        ),
    )?;
    fs::set_permissions(&source_binary, fs::Permissions::from_mode(0o700))?;
    let plan = PreparationPlan {
        format: 1,
        service: "xscs-fixture.service".into(),
        source_binary_sha256: snapshot::digest_file(&source_binary)?,
        source_binary,
        source_identity: identity,
        config,
        data_dir,
        database,
        work_directory: root.path().join("prepared"),
        max_backup_bytes: 16 * 1024 * 1024,
    };
    Ok((root, plan))
}

#[cfg(target_os = "linux")]
#[test]
fn complete_preparation_retains_verified_original_backup_and_task() -> anyhow::Result<()> {
    let (_root, plan) = preparation_fixture(&capabilities())?;
    let mut stage = PreparationStage::Plan;
    let report = prepare_with_stop_check(&plan, |_| Ok(()), &mut stage)?;
    assert_eq!(report.phase, "prepared");
    assert_eq!(report.invalidated_devices, 1);
    assert_eq!(
        snapshot::inventory(
            &plan.work_directory.join("data"),
            true,
            plan.max_backup_bytes,
            true
        )?,
        report.data_entries
    );
    let stored: PreparationReport = serde_json::from_slice(&snapshot::read_bounded(
        &plan.work_directory.join(REPORT),
        MAX_JSON,
    )?)?;
    assert_eq!(stored.phase, "prepared");
    xcsc::sqlite::block_on_sqlite_connection(async {
        for (database, expected) in [
            (plan.database.clone(), None),
            (
                plan.work_directory.join("data/xscs.sqlite3"),
                Some("old-session"),
            ),
        ] {
            let mut connection =
                SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(database))
                    .await?;
            assert_eq!(
                sqlx::query_scalar::<_, Option<String>>("SELECT session_id FROM devices")
                    .fetch_one(&mut connection)
                    .await?
                    .as_deref(),
                expected
            );
            assert_eq!(
                sqlx::query_scalar::<_, String>("SELECT state FROM _xcss_operations")
                    .fetch_one(&mut connection)
                    .await?,
                "unknown"
            );
            assert_eq!(
                sqlx::query_scalar::<_, String>("SELECT detail FROM audit_logs")
                    .fetch_one(&mut connection)
                    .await?,
                "audit-detail"
            );
            connection.close().await?;
        }
        Ok::<_, anyhow::Error>(())
    })?;
    // Reusing an existing work directory must not overwrite the original group.
    assert!(prepare_with_stop_check(&plan, |_| Ok(()), &mut stage).is_err());
    assert_eq!(stage.public_failure(None).code, "XSCS_BACKUP_FAILED");
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn writer_and_identity_refusals_never_create_backup_or_modify_database() -> anyhow::Result<()> {
    let (_root, plan) = preparation_fixture(&capabilities())?;
    let original = snapshot::digest_file(&plan.database)?;
    let mut stage = PreparationStage::Plan;
    assert!(
        prepare_with_stop_check(&plan, |_| anyhow::bail!("active fixture"), &mut stage).is_err()
    );
    assert_eq!(stage.public_failure(None).code, "XSCS_WRITER_NOT_EXCLUDED");
    assert!(!plan.work_directory.exists());
    let state = PrivateStateDirectory::open(&plan.data_dir)?;
    let instance = state.try_instance_lock()?;
    assert!(prepare_with_stop_check(&plan, |_| Ok(()), &mut stage).is_err());
    assert_eq!(stage.public_failure(None).code, "XSCS_WRITER_NOT_EXCLUDED");
    drop(instance);
    let product_lock = state.create_file(".xscs.sqlite3.xscs.instance.lock")?;
    rustix::fs::flock(
        product_lock.file(),
        rustix::fs::FlockOperation::NonBlockingLockExclusive,
    )?;
    assert!(prepare_with_stop_check(&plan, |_| Ok(()), &mut stage).is_err());
    assert_eq!(stage.public_failure(None).code, "XSCS_WRITER_NOT_EXCLUDED");
    drop(product_lock);
    let mut wrong = plan.clone();
    wrong.source_identity.source_revision = "c".repeat(40);
    assert!(prepare_with_stop_check(&wrong, |_| Ok(()), &mut stage).is_err());
    assert_eq!(stage.public_failure(None).code, "XSCS_SOURCE_INVALID");
    wrong = plan.clone();
    wrong.source_binary_sha256 = "d".repeat(64);
    assert!(prepare_with_stop_check(&wrong, |_| Ok(()), &mut stage).is_err());
    assert_eq!(stage.public_failure(None).code, "XSCS_SOURCE_INVALID");
    assert!(!plan.work_directory.exists());
    assert_eq!(snapshot::digest_file(&plan.database)?, original);
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn cache_rejection_keeps_backup_and_write_intent_without_claiming_completion() -> anyhow::Result<()>
{
    let (_root, plan) = preparation_fixture("{\"protocol\":\"xscs-management/3\"}")?;
    let original = snapshot::digest_file(&plan.database)?;
    let mut stage = PreparationStage::Plan;
    assert!(prepare_with_stop_check(&plan, |_| Ok(()), &mut stage).is_err());
    assert_eq!(
        stage.public_failure(None).code,
        "XSCS_CACHE_STATE_UNCONFIRMED"
    );
    let stored: PreparationReport = serde_json::from_slice(&snapshot::read_bounded(
        &plan.work_directory.join(REPORT),
        MAX_JSON,
    )?)?;
    assert_eq!(stored.phase, "cache-write-intent");
    assert_eq!(snapshot::digest_file(&plan.database)?, original);
    assert_eq!(
        snapshot::digest_file(&plan.work_directory.join("data/xscs.sqlite3"))?,
        original
    );
    Ok(())
}
