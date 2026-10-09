//! Opt-in checks against an actual current product. Only the service-manager
//! lifecycle is replaced. Current init/schema validation, ordinary pending and
//! locks, current runtime and business readiness execute the actual product.
//! These exercise a same-contract release/reinstall; no imaginary future
//! schema or historical conversion is supplied.
use super::*;
use sqlx::{Connection, Row, TypeInfo, ValueRef};
use std::os::unix::{fs::PermissionsExt, process::CommandExt};
use std::process::{Command, Stdio};

struct ProductFixture {
    _root: tempfile::TempDir,
    plan: UpgradePlan,
    source_rows: Vec<(String, Vec<Vec<String>>)>,
}
fn rows(path: &Path) -> Vec<(String, Vec<Vec<String>>)> {
    futures_executor::block_on(async {
        let mut connection = sqlx::SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new().filename(path),
        )
        .await
        .unwrap();
        let names: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_list WHERE schema='main' AND type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .fetch_all(&mut connection).await.unwrap();
        let mut result = Vec::new();
        for name in names {
            // The identifier comes from this initialized schema and is quoted,
            // including escaping embedded quotes; no value enters SQL syntax.
            let statement = sqlx::AssertSqlSafe(format!(
                "SELECT * FROM \"{}\" ORDER BY rowid",
                name.replace('"', "\"\"")
            ));
            let records = sqlx::query(statement)
                .fetch_all(&mut connection)
                .await
                .unwrap();
            let values = records
                .iter()
                .map(|row| {
                    (0..row.len())
                        .map(|index| {
                            let raw = row.try_get_raw(index).unwrap();
                            if raw.is_null() {
                                return "Null".into();
                            }
                            match raw.type_info().name() {
                                "INTEGER" => {
                                    format!("Integer({})", row.try_get::<i64, _>(index).unwrap())
                                }
                                "REAL" => {
                                    format!("Real({:?})", row.try_get::<f64, _>(index).unwrap())
                                }
                                kind @ ("TEXT" | "BLOB") => {
                                    // Preserve exact SQLite bytes, including embedded NULs.
                                    // The runtime type above justifies bypassing only the
                                    // text/blob type check of the safe SQLx row decoder.
                                    let bytes = row.try_get_unchecked::<Vec<u8>, _>(index).unwrap();
                                    format!("{kind}({bytes:?})")
                                }
                                kind => {
                                    panic!("unexpected initialized SQLite fixture type: {kind}")
                                }
                            }
                        })
                        .collect()
                })
                .collect();
            result.push((name, values));
        }
        connection.close().await.unwrap();
        result
    })
}
impl ProductFixture {
    fn new() -> Self {
        let executable = PathBuf::from(
            std::env::var_os("XCSS_TEST_POETIZE_BINARY")
                .expect("supply the actual current Xocs executable"),
        );
        let root =
            tempfile::tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("target")).unwrap();
        let path = root.path();
        let data = path.join("data");
        snapshot::private_directory(&data).unwrap();
        let configuration = path.join("configuration");
        snapshot::private_directory(&configuration).unwrap();
        let config = configuration.join("config.json");
        let address = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        fs::write(
            &config,
            serde_json::to_vec(&serde_json::json!({"bind":address,"development_http":true}))
                .unwrap(),
        )
        .unwrap();
        fs::set_permissions(&config, fs::Permissions::from_mode(0o600)).unwrap();
        let mut initializer = Command::new(&executable)
            .args(["init", "--config"])
            .arg(&config)
            .arg("--data-dir")
            .arg(&data)
            .arg("--json")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        initializer
            .stdin
            .take()
            .unwrap()
            .write_all(b"FixturePassphrase123\n")
            .unwrap();
        let output = initializer.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "actual current initialization failed"
        );
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let schema: SchemaIdentity =
            serde_json::from_value(report["schema_identity"].clone()).unwrap();
        let database = data.join("site.sqlite");
        futures_executor::block_on(async {
            let mut connection = sqlx::SqliteConnection::connect_with(
                &sqlx::sqlite::SqliteConnectOptions::new().filename(&database),
            )
            .await
            .unwrap();
            sqlx::raw_sql("INSERT INTO user(id,username,user_type) VALUES(900,'fixture-member',2); INSERT INTO article(id,user_id,sort_id,label_id,article_title,article_content) VALUES(901,1,1,1,'Current fixture title','Current protected fixture content'); INSERT INTO _xcss_security_audit_events(event_id,action,outcome,detail_json,occurred_at_micros) VALUES('fixture-audit','fixture-test','success','{}',10);")
                .execute(&mut connection).await.unwrap();
            connection.close().await.unwrap();
        });
        fs::write(data.join("media/fixture.bin"), b"current-media-bytes").unwrap();
        let installed = path.join("server");
        let target = path.join("new-server");
        // Distinct regular copies also remove Cargo's build-output hardlinks.
        for destination in [&installed, &target] {
            fs::copy(&executable, destination).unwrap();
            fs::set_permissions(destination, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let private = path.join("fixture-signing.pem");
        let public = path.join("fixture-public.pem");
        run_bounded(
            "/usr/bin/openssl",
            &[
                "genpkey".into(),
                "-algorithm".into(),
                "ED25519".into(),
                "-out".into(),
                private.to_string_lossy().into_owned(),
            ],
            10,
        )
        .unwrap();
        run_bounded(
            "/usr/bin/openssl",
            &[
                "pkey".into(),
                "-in".into(),
                private.to_string_lossy().into_owned(),
                "-pubout".into(),
                "-out".into(),
                public.to_string_lossy().into_owned(),
            ],
            10,
        )
        .unwrap();
        let key = run_bounded(
            "/usr/bin/openssl",
            &[
                "pkey".into(),
                "-pubin".into(),
                "-in".into(),
                public.to_string_lossy().into_owned(),
                "-outform".into(),
                "DER".into(),
            ],
            10,
        )
        .unwrap();
        let plan = UpgradePlan {
            plan_version: 1,
            service: "poetize-current-fixture.service".into(),
            additional_services: Vec::new(),
            installed_binary: installed,
            target_binary: target,
            release_manifest: path.join("release.json"),
            release_signature: path.join("release.sig"),
            trusted_public_key: public,
            trusted_public_key_sha256: digest_hex(Sha256::digest(key)),
            config: config.clone(),
            data_dir: data.clone(),
            resources: vec![
                PersistentResource {
                    name: "config".into(),
                    kind: ResourceType::File,
                    path: config,
                },
                PersistentResource {
                    name: "data".into(),
                    kind: ResourceType::Directory,
                    path: data.clone(),
                },
            ],
            work_directory: path.join("upgrade"),
            readiness_address: address,
            timeout_seconds: 90,
            max_backup_bytes: 1024 * 1024 * 1024,
            native_release: None,
        };
        let bytes = run_bounded(
            &plan.target_binary,
            &["release-identity".into(), "--json".into()],
            90,
        )
        .expect("integration checks require a real source-bound current product");
        let identity: ReleaseIdentity = serde_json::from_slice(&bytes).unwrap();
        identity.validate().unwrap();
        let release = UpgradeRelease {
            manifest_version: 1,
            source_identity: identity.clone(),
            identity,
            binary_sha256: snapshot::digest_file(&plan.target_binary).unwrap(),
            source_schema: schema.clone(),
            target_schema: schema,
            artifact: None,
            additional_service_roles: Vec::new(),
            resources: plan
                .resources
                .iter()
                .map(|resource| ReleaseResource {
                    name: resource.name.clone(),
                    kind: resource.kind,
                })
                .collect(),
        };
        fs::write(
            &plan.release_manifest,
            serde_json::to_vec(&release).unwrap(),
        )
        .unwrap();
        run_bounded(
            "/usr/bin/openssl",
            &[
                "pkeyutl".into(),
                "-sign".into(),
                "-inkey".into(),
                private.to_string_lossy().into_owned(),
                "-rawin".into(),
                "-in".into(),
                plan.release_manifest.to_string_lossy().into_owned(),
                "-out".into(),
                plan.release_signature.to_string_lossy().into_owned(),
            ],
            10,
        )
        .unwrap();
        let source_rows = rows(&database);
        let fixture = Self {
            _root: root,
            plan,
            source_rows,
        };
        if rustix::process::geteuid().as_raw() == 0 {
            fs::set_permissions(fixture._root.path(), fs::Permissions::from_mode(0o755)).unwrap();
            let parent = std::fs::File::open(fixture.plan.config.parent().unwrap()).unwrap();
            rustix::fs::fchown(
                &parent,
                Some(rustix::process::Uid::from_raw(65534)),
                Some(rustix::process::Gid::from_raw(65534)),
            )
            .unwrap();
            for resource in &fixture.plan.resources {
                let entries = snapshot::inventory(
                    &resource.path,
                    resource.kind == ResourceType::Directory,
                    fixture.plan.max_backup_bytes,
                    false,
                )
                .unwrap();
                let ownership = entries
                    .iter()
                    .map(|entry| snapshot::Ownership {
                        path: entry.path.clone(),
                        uid: 65534,
                        gid: 65534,
                    })
                    .collect::<Vec<_>>();
                snapshot::restore_ownership(&resource.path, &entries, &ownership).unwrap();
            }
        }
        fixture
    }
}
#[derive(Default)]
struct ProductControl {
    systemd: SystemdControl,
    child: Option<std::process::Child>,
    validate_calls: usize,
    damage_target_validation: bool,
}
impl Drop for ProductControl {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
impl ServiceControl for ProductControl {
    fn verify_release_identity(
        &mut self,
        binary: &Path,
        plan: &UpgradePlan,
        identity: &ReleaseIdentity,
    ) -> anyhow::Result<()> {
        self.systemd.verify_release_identity(binary, plan, identity)
    }
    fn is_stopped(&mut self, _: &UpgradePlan) -> anyhow::Result<bool> {
        Ok(self.child.is_none())
    }
    #[allow(unsafe_code)] // Real product process credential boundary only.
    fn start(&mut self, plan: &UpgradePlan) -> anyhow::Result<()> {
        let (uid, gid) = execution::identity(plan)?;
        let mut command = Command::new(&plan.installed_binary);
        // SAFETY: fork child calls only credential syscalls with owned numeric
        // IDs, clears supplementary groups first and never enters the runtime.
        unsafe {
            command.pre_exec(move || {
                if libc::geteuid() == 0
                    && (libc::setgroups(0, std::ptr::null()) != 0
                        || libc::setgid(gid) != 0
                        || libc::setuid(uid) != 0)
                {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        self.child = Some(
            command
                .arg("run")
                .arg("--config")
                .arg(&plan.config)
                .arg("--data-dir")
                .arg(&plan.data_dir)
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("LANG", "C.UTF-8")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?,
        );
        Ok(())
    }
    fn validate(
        &mut self,
        binary: &Path,
        plan: &UpgradePlan,
        artifact: Option<&ReleaseArtifact>,
    ) -> anyhow::Result<ValidatedState> {
        self.validate_calls += 1;
        if self.damage_target_validation && self.validate_calls == 2 {
            fs::write(&plan.config, b"{\"unknown_test_field\":true}")?;
            fs::write(
                plan.data_dir.join("media/fixture.bin"),
                b"fault-injected-current-media",
            )?;
        }
        self.systemd.validate(binary, plan, artifact)
    }
    fn ready(&mut self, plan: &UpgradePlan, hash: &str, product: &str) -> anyhow::Result<bool> {
        let Some(child) = self.child.as_mut() else {
            return Ok(false);
        };
        if child.try_wait()?.is_some() {
            return Ok(false);
        }
        let executable = fs::read_link(format!("/proc/{}/exe", child.id()))?;
        if executable != plan.installed_binary
            || snapshot::digest_file(&executable)? != hash
            || fs::metadata(format!("/proc/{}", child.id()))?.uid() != execution::identity(plan)?.0
        {
            return Ok(false);
        }
        http_ready(plan.readiness_address, product)
    }
}
#[test]
#[ignore = "requires XCSS_TEST_POETIZE_BINARY; actual current product and runtime sockets"]
fn actual_current_product_preserves_state_and_becomes_business_ready() {
    let fixture = ProductFixture::new();
    let mut control = ProductControl::default();
    assert_eq!(
        apply(&fixture.plan, &mut control).unwrap().phase,
        UpgradePhase::Ready
    );
    assert_eq!(
        rows(&fixture.plan.data_dir.join("site.sqlite")),
        fixture.source_rows
    );
    assert_eq!(
        fs::read(fixture.plan.data_dir.join("media/fixture.bin")).unwrap(),
        b"current-media-bytes"
    );
    assert!(!fixture.plan.data_dir.join(PENDING).exists());
    if rustix::process::geteuid().as_raw() == 0 {
        assert_eq!(
            fs::metadata(fixture.plan.data_dir.join("site.sqlite"))
                .unwrap()
                .uid(),
            65534
        );
    }
}
#[test]
#[ignore = "requires XCSS_TEST_POETIZE_BINARY; actual current product and recovery sockets"]
fn actual_current_validation_failure_restores_current_group_and_real_readiness() {
    let fixture = ProductFixture::new();
    let original_config = fs::read(&fixture.plan.config).unwrap();
    let original_hash = snapshot::digest_file(&fixture.plan.installed_binary).unwrap();
    let mut control = ProductControl::default();
    control.damage_target_validation = true;
    assert_eq!(
        apply(&fixture.plan, &mut control).unwrap_err().code,
        "UPGRADE_STATE_VALIDATION_FAILED"
    );
    assert_eq!(
        inspect(&fixture.plan.work_directory).unwrap().phase,
        UpgradePhase::BackupComplete
    );
    assert_eq!(
        recover(&fixture.plan.work_directory, false, &mut control)
            .unwrap()
            .phase,
        UpgradePhase::RolledBack
    );
    assert_eq!(fs::read(&fixture.plan.config).unwrap(), original_config);
    assert_eq!(
        snapshot::digest_file(&fixture.plan.installed_binary).unwrap(),
        original_hash
    );
    assert_eq!(
        rows(&fixture.plan.data_dir.join("site.sqlite")),
        fixture.source_rows
    );
    assert_eq!(
        fs::read(fixture.plan.data_dir.join("media/fixture.bin")).unwrap(),
        b"current-media-bytes"
    );
    if rustix::process::geteuid().as_raw() == 0 {
        for path in [
            &fixture.plan.config,
            &fixture.plan.data_dir.join("site.sqlite"),
            &fixture.plan.data_dir.join("media/fixture.bin"),
        ] {
            assert_eq!(fs::metadata(path).unwrap().uid(), 65534);
        }
    }
    assert!(
        control
            .ready(&fixture.plan, &original_hash, "xocs")
            .unwrap()
    );
}
