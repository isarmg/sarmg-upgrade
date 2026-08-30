use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{OsStr, OsString},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::fd::AsRawFd,
    os::unix::fs::OpenOptionsExt,
    path::{Component, Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use anyhow::{Context, ensure};
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use hkdf::Hkdf;
use rand::{RngCore, rngs::OsRng};
use rusqlite::{Connection, OpenFlags};
use rustix::{
    fs::{AtFlags, FileType, FlockOperation, Mode, OFlags, fstat, mkdirat, openat2, statat},
    io::Errno,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{
    Adapter, DATABASE_FILE, MaintenanceLock, PRODUCT_METADATA_DDL, PendingDirectory, Product,
    RestorePoint, SchemaIdentity, SecureDirectory, SourceClone, TargetStaging,
    copy_database_online, copy_regular_file_at, create_private_empty_file,
    expected_migration_checksum, hash_regular_file, open_pending_directory,
    recover_sqlite_restore_under_lock_with_verifier, replace_with_staged_database,
    schema_fingerprint_connection, secure_resolve_flags, sqlite_read_only_uri, sync_directory,
    verify_current_database, verify_source_database,
};
use crate::{RecoveryAction, RecoveryResult};

const MIGRATION_0001: &str = include_str!("../../upgrades/sentinel_0_1_to_0_2/0001_init.sql");
const MIGRATION_0002: &str =
    include_str!("../../upgrades/sentinel_0_1_to_0_2/0002_browser_sessions.sql");
const MIGRATION_0003: &str =
    include_str!("../../upgrades/sentinel_0_1_to_0_2/0003_media_reconciliation.sql");
const TARGET_SCHEMA_SQL: &str = include_str!("../../upgrades/sentinel_0_1_to_0_2/target.sql");

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

const MIGRATIONS: [(i64, &str, &str); 3] = [
    (1, "init", MIGRATION_0001),
    (2, "browser sessions", MIGRATION_0002),
    (3, "media reconciliation", MIGRATION_0003),
];
const SOURCE_SCHEMA_SHA256: &str =
    "b1c025356eb3ac3f17ff6e94e262dccb05be08d78ef1350670cd6a6c08aca4ea";
const TARGET_SCHEMA_SHA256: &str =
    "73a26cfd0d8d55f1559407904fe6445e278310614750cc1c0f3306a2803b7df6";
const TARGET_SOURCE_COMMIT: &str = "e51a4a90933547c1f95b625635a4430e4632acb9";
const CREDENTIAL_PRODUCT: &str = "sentinel-monitor";
const CREDENTIAL_APPLICATION_VERSION: &str = "0.2.0";
const CREDENTIAL_ENVELOPE_REVISION: u32 = 1;
const CREDENTIAL_KEY_ID: &str = "sentinel-credentials-0.2.0-key-1";
const CREDENTIAL_KEY_DERIVATION_SALT: &[u8] = b"sentinel-monitor/0.2.0/credential-envelope/key/v1";
const CREDENTIAL_KEY_DERIVATION_INFO: &[u8] = b"sentinel-credential-envelope/aes-256-gcm";
const CREDENTIAL_AAD_DOMAIN: &str = "sentinel-monitor/0.2.0/credential-envelope/aad/v1";
const MAX_CREDENTIAL_ENVELOPE_BYTES: usize = 64 * 1024;
const MAX_CREDENTIAL_PLAINTEXT_BYTES: usize = 16 * 1024;
const CONTRACT_VERSION: &str = "v1.20.0";
const CONTRACT_PLATFORM: &str = "linux_amd64";
const CONTRACT_BINARY_SHA256: &str =
    "25947caac403f37ec881c9be213af2cad67e344a6c7098905b0d31c17f40e336";
const BUNDLE_MANIFEST_VERSION: u32 = 2;
const BUNDLE_MANIFEST_FILE: &str = "manifest.json";
const BUNDLE_CONFIG_FILE: &str = "mediamtx.yml";
const BUNDLE_CONTRACT_FILE: &str = "mediamtx.lock";
const BUNDLE_RECORDINGS_DIRECTORY: &str = "recordings";
const MAX_MANIFEST_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
const MAX_CONTRACT_BYTES: u64 = 16 * 1024;
const MAX_CREDENTIAL_FILE_BYTES: u64 = 1024;
const MAX_RECORDING_FILES: usize = 1_000_000;
const MAX_RECORDING_DIRECTORIES: usize = 250_000;
const MAX_RECORDING_ENTRIES: usize = 1_000_000;
const MAX_RECORDING_PATH_COMPONENTS: usize = 64;
const MAX_ENTRIES_PER_DIRECTORY: usize = 100_000;

const ADAPTER: Adapter = Adapter {
    product: Product::SentinelMonitor,
    from_version: "0.1.0",
    to_version: "0.2.0",
    source_revision: 3,
    source_schema_sha256: SOURCE_SCHEMA_SHA256,
    target_revision: 1,
    target_schema_sha256: TARGET_SCHEMA_SHA256,
    target_schema_sql: TARGET_SCHEMA_SQL,
    verify_ledger,
    copy_rows,
};

const DATA_TABLES: [&str; 9] = [
    "users",
    "cameras",
    "events",
    "audit_logs",
    "browser_sessions",
    "media_desired_states",
    "media_operations",
    "media_actual_paths",
    "media_reconciler_leases",
];

pub struct SentinelUpgradeOptions {
    pub product: Product,
    pub from_version: String,
    pub to_version: String,
    pub database: PathBuf,
    pub backup_output: PathBuf,
    pub runtime_directory: PathBuf,
    pub mediamtx_config: PathBuf,
    pub mediamtx_contract: PathBuf,
    pub recordings_directory: PathBuf,
    pub credentials_key: [u8; 32],
}

impl std::fmt::Debug for SentinelUpgradeOptions {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SentinelUpgradeOptions")
            .field("product", &self.product)
            .field("from_version", &self.from_version)
            .field("to_version", &self.to_version)
            .field("database", &self.database)
            .field("backup_output", &self.backup_output)
            .field("runtime_directory", &self.runtime_directory)
            .field("mediamtx_config", &self.mediamtx_config)
            .field("mediamtx_contract", &self.mediamtx_contract)
            .field("recordings_directory", &self.recordings_directory)
            .field("credentials_key", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone)]
pub struct SentinelRecoveryOptions {
    pub product: Product,
    pub from_version: String,
    pub to_version: String,
    pub database: PathBuf,
    pub runtime_directory: PathBuf,
    pub recovery_directory: PathBuf,
    pub action: RecoveryAction,
    pub credentials_key: Option<[u8; 32]>,
}

impl std::fmt::Debug for SentinelRecoveryOptions {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SentinelRecoveryOptions")
            .field("product", &self.product)
            .field("from_version", &self.from_version)
            .field("to_version", &self.to_version)
            .field("database", &self.database)
            .field("runtime_directory", &self.runtime_directory)
            .field("recovery_directory", &self.recovery_directory)
            .field("action", &self.action)
            .field("credentials_key", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct SentinelUpgradeResult {
    pub product: Product,
    pub from_version: String,
    pub to_version: String,
    pub source_backup: PathBuf,
    pub database: PathBuf,
    pub schema_identity: SchemaIdentity,
}

#[derive(Clone, Debug)]
pub struct VerifiedSentinelSourceBackup {
    pub directory: PathBuf,
    pub manifest: SentinelSourceBackupManifest,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SentinelSourceBackupManifest {
    pub manifest_version: u32,
    pub tool_version: String,
    pub product: Product,
    pub from_version: String,
    pub to_version: String,
    pub source_schema_identity: SchemaIdentity,
    pub target_schema_identity: SchemaIdentity,
    pub target_source_commit: String,
    pub credential_envelope_contract: BTreeMap<String, String>,
    pub created_at_epoch_seconds: u64,
    pub database: SentinelStoredFile,
    pub database_records: BTreeMap<String, u64>,
    pub mediamtx_config: SentinelStoredFile,
    pub mediamtx_contract: SentinelCompanionContract,
    pub recording_root_sha256: String,
    pub recordings: SentinelRecordingArchive,
    pub credentials_key_included: bool,
    pub credentials_key_required_for_upgrade: bool,
    pub credentials_key_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SentinelStoredFile {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SentinelCompanionContract {
    pub contract_file: SentinelStoredFile,
    pub version: String,
    pub platform: String,
    pub binary_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SentinelRecordingArchive {
    pub directory: String,
    pub directories: Vec<String>,
    pub files: Vec<SentinelStoredFile>,
    pub bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SentinelDatabaseSummary {
    records: BTreeMap<String, u64>,
    recording_paths: BTreeSet<String>,
}

type CredentialInventory = BTreeMap<(Uuid, CredentialField), [u8; 32]>;

#[derive(Clone, Debug, Eq, PartialEq)]
struct RecordingInventory {
    directories: Vec<String>,
    files: Vec<SentinelStoredFile>,
    bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ParsedMediaConfig {
    recording_root: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ParsedContract {
    version: String,
    platform: String,
    binary_sha256: String,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum CredentialField {
    MainStreamUrl,
    SubStreamUrl,
    Username,
    Password,
}

impl CredentialField {
    const ALL: [Self; 4] = [
        Self::MainStreamUrl,
        Self::SubStreamUrl,
        Self::Username,
        Self::Password,
    ];

    const fn database_name(self) -> &'static str {
        match self {
            Self::MainStreamUrl => "main_stream_url_enc",
            Self::SubStreamUrl => "sub_stream_url_enc",
            Self::Username => "username_enc",
            Self::Password => "password_enc",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CredentialEnvelope {
    product: String,
    application_version: String,
    envelope_revision: u32,
    key_id: String,
    nonce: String,
    ciphertext: String,
}

struct CredentialTransformer {
    legacy: Aes256Gcm,
    current: Aes256Gcm,
}

impl CredentialTransformer {
    fn new(master_key: &[u8; 32]) -> Self {
        let mut derived = [0_u8; 32];
        Hkdf::<Sha256>::new(Some(CREDENTIAL_KEY_DERIVATION_SALT), master_key)
            .expand(CREDENTIAL_KEY_DERIVATION_INFO, &mut derived)
            .expect("32-byte Sentinel HKDF output is valid");
        let current = Aes256Gcm::new_from_slice(&derived).expect("32-byte Sentinel derived key");
        derived.fill(0);
        Self {
            legacy: Aes256Gcm::new_from_slice(master_key)
                .expect("32-byte Sentinel legacy credentials key"),
            current,
        }
    }

    fn decrypt_legacy(&self, encoded: &[u8]) -> anyhow::Result<String> {
        ensure!(
            (12 + 16..=12 + 16 + MAX_CREDENTIAL_PLAINTEXT_BYTES).contains(&encoded.len()),
            "stored Sentinel 0.1 credential is malformed"
        );
        let plaintext = self
            .legacy
            .decrypt(Nonce::from_slice(&encoded[..12]), &encoded[12..])
            .map_err(|_| {
                anyhow::anyhow!("credentials key cannot authenticate Sentinel 0.1 camera data")
            })?;
        String::from_utf8(plaintext)
            .map_err(|_| anyhow::anyhow!("Sentinel 0.1 credential plaintext is not UTF-8"))
    }

    fn encrypt_current(
        &self,
        camera_id: Uuid,
        field: CredentialField,
        plaintext: &str,
    ) -> anyhow::Result<Vec<u8>> {
        let mut nonce = [0_u8; 12];
        OsRng.fill_bytes(&mut nonce);
        self.encrypt_current_with_nonce(camera_id, field, plaintext, nonce)
    }

    fn encrypt_current_with_nonce(
        &self,
        camera_id: Uuid,
        field: CredentialField,
        plaintext: &str,
        nonce: [u8; 12],
    ) -> anyhow::Result<Vec<u8>> {
        ensure!(
            plaintext.len() <= MAX_CREDENTIAL_PLAINTEXT_BYTES,
            "Sentinel credential exceeds the 0.2 plaintext limit"
        );
        let aad = credential_aad(camera_id, field);
        let ciphertext = self
            .current
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: plaintext.as_bytes(),
                    aad: &aad,
                },
            )
            .map_err(|_| anyhow::anyhow!("Sentinel 0.2 credential encryption failed"))?;
        let envelope = CredentialEnvelope {
            product: CREDENTIAL_PRODUCT.to_owned(),
            application_version: CREDENTIAL_APPLICATION_VERSION.to_owned(),
            envelope_revision: CREDENTIAL_ENVELOPE_REVISION,
            key_id: CREDENTIAL_KEY_ID.to_owned(),
            nonce: URL_SAFE_NO_PAD.encode(nonce),
            ciphertext: URL_SAFE_NO_PAD.encode(ciphertext),
        };
        let encoded = serde_json::to_vec(&envelope)
            .map_err(|_| anyhow::anyhow!("Sentinel 0.2 envelope serialization failed"))?;
        ensure!(
            encoded.len() <= MAX_CREDENTIAL_ENVELOPE_BYTES,
            "Sentinel 0.2 credential envelope exceeds its limit"
        );
        ensure!(
            self.decrypt_current(camera_id, field, &encoded)? == plaintext,
            "Sentinel 0.2 credential envelope self-verification failed"
        );
        Ok(encoded)
    }

    fn decrypt_current(
        &self,
        camera_id: Uuid,
        field: CredentialField,
        encoded: &[u8],
    ) -> anyhow::Result<String> {
        ensure!(
            !encoded.is_empty() && encoded.len() <= MAX_CREDENTIAL_ENVELOPE_BYTES,
            "Sentinel credential envelope is not exactly current or authenticated"
        );
        let envelope: CredentialEnvelope =
            serde_json::from_slice(encoded).map_err(|_| malformed_current_credential())?;
        let canonical =
            serde_json::to_vec(&envelope).map_err(|_| malformed_current_credential())?;
        ensure!(
            canonical == encoded
                && envelope.product == CREDENTIAL_PRODUCT
                && envelope.application_version == CREDENTIAL_APPLICATION_VERSION
                && envelope.envelope_revision == CREDENTIAL_ENVELOPE_REVISION
                && envelope.key_id == CREDENTIAL_KEY_ID,
            "Sentinel credential envelope is not exactly current or authenticated"
        );
        let nonce = decode_canonical_base64url(&envelope.nonce)?;
        let nonce: [u8; 12] = nonce
            .try_into()
            .map_err(|_| malformed_current_credential())?;
        let ciphertext = decode_canonical_base64url(&envelope.ciphertext)?;
        ensure!(
            (16..=MAX_CREDENTIAL_PLAINTEXT_BYTES + 16).contains(&ciphertext.len()),
            "Sentinel credential envelope is not exactly current or authenticated"
        );
        let aad = credential_aad(camera_id, field);
        let plaintext = self
            .current
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| malformed_current_credential())?;
        String::from_utf8(plaintext).map_err(|_| malformed_current_credential())
    }
}

fn malformed_current_credential() -> anyhow::Error {
    anyhow::anyhow!("Sentinel credential envelope is not exactly current or authenticated")
}

fn decode_canonical_base64url(encoded: &str) -> anyhow::Result<Vec<u8>> {
    let decoded = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| malformed_current_credential())?;
    ensure!(
        URL_SAFE_NO_PAD.encode(&decoded) == encoded,
        "Sentinel credential envelope is not exactly current or authenticated"
    );
    Ok(decoded)
}

fn credential_aad(camera_id: Uuid, field: CredentialField) -> Vec<u8> {
    let camera_id = camera_id.hyphenated().to_string();
    let revision = CREDENTIAL_ENVELOPE_REVISION.to_string();
    let mut aad = Vec::new();
    for value in [
        CREDENTIAL_AAD_DOMAIN,
        CREDENTIAL_PRODUCT,
        CREDENTIAL_APPLICATION_VERSION,
        revision.as_str(),
        CREDENTIAL_KEY_ID,
        camera_id.as_str(),
        field.database_name(),
    ] {
        aad.extend_from_slice(&(value.len() as u64).to_be_bytes());
        aad.extend_from_slice(value.as_bytes());
    }
    aad
}

fn credential_envelope_contract() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("aad-domain".to_owned(), CREDENTIAL_AAD_DOMAIN.to_owned()),
        (
            "aad-fields".to_owned(),
            CredentialField::ALL
                .into_iter()
                .map(CredentialField::database_name)
                .collect::<Vec<_>>()
                .join(","),
        ),
        (
            "application-version".to_owned(),
            CREDENTIAL_APPLICATION_VERSION.to_owned(),
        ),
        ("cipher".to_owned(), "AES-256-GCM".to_owned()),
        (
            "encoding".to_owned(),
            "canonical-json+base64url-no-pad".to_owned(),
        ),
        (
            "envelope-revision".to_owned(),
            CREDENTIAL_ENVELOPE_REVISION.to_string(),
        ),
        (
            "key-derivation-info".to_owned(),
            String::from_utf8_lossy(CREDENTIAL_KEY_DERIVATION_INFO).into_owned(),
        ),
        (
            "key-derivation-salt".to_owned(),
            String::from_utf8_lossy(CREDENTIAL_KEY_DERIVATION_SALT).into_owned(),
        ),
        ("key-id".to_owned(), CREDENTIAL_KEY_ID.to_owned()),
        (
            "max-plaintext-bytes".to_owned(),
            MAX_CREDENTIAL_PLAINTEXT_BYTES.to_string(),
        ),
        ("product".to_owned(), CREDENTIAL_PRODUCT.to_owned()),
        (
            "source-format".to_owned(),
            "aes-256-gcm/raw-nonce12-ciphertext-tag16".to_owned(),
        ),
    ])
}

fn target_schema_identity() -> SchemaIdentity {
    SchemaIdentity {
        application: Product::SentinelMonitor.slug().to_owned(),
        application_version: ADAPTER.to_version.to_owned(),
        schema_revision: ADAPTER.target_revision,
        schema_sha256: ADAPTER.target_schema_sha256.to_owned(),
    }
}

pub fn sentinel_credentials_key_from_file(path: &Path) -> anyhow::Result<[u8; 32]> {
    let file = SecureFile::open(path, "Sentinel credentials key file")?;
    let named_before = statat(&file.parent.file, &file.name, AtFlags::SYMLINK_NOFOLLOW)?;
    ensure!(
        named_before.st_mode & 0o077 == 0,
        "Sentinel credentials key file must be private"
    );
    let fd = openat2(
        &file.parent.file,
        &file.name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
        secure_resolve_flags(),
    )?;
    let opened_before = fstat(&fd)?;
    ensure!(
        FileType::from_raw_mode(opened_before.st_mode) == FileType::RegularFile
            && opened_before.st_nlink == 1
            && opened_before.st_mode & 0o077 == 0
            && opened_before.st_size >= 0
            && opened_before.st_size as u64 <= MAX_CREDENTIAL_FILE_BYTES
            && opened_before.st_dev == named_before.st_dev
            && opened_before.st_ino == named_before.st_ino,
        "Sentinel credentials key path changed while it was opened"
    );
    let mut opened = File::from(fd);
    let mut bytes = Vec::with_capacity(opened_before.st_size as usize);
    Read::by_ref(&mut opened)
        .take(MAX_CREDENTIAL_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    let opened_after = fstat(&opened)?;
    let named_after = statat(&file.parent.file, &file.name, AtFlags::SYMLINK_NOFOLLOW)?;
    ensure!(
        bytes.len() as i64 == opened_before.st_size
            && opened_after.st_dev == opened_before.st_dev
            && opened_after.st_ino == opened_before.st_ino
            && opened_after.st_size == opened_before.st_size
            && opened_after.st_nlink == 1
            && opened_after.st_mode & 0o077 == 0
            && opened_after.st_mtime == opened_before.st_mtime
            && opened_after.st_mtime_nsec == opened_before.st_mtime_nsec
            && opened_after.st_ctime == opened_before.st_ctime
            && opened_after.st_ctime_nsec == opened_before.st_ctime_nsec
            && named_after.st_dev == opened_after.st_dev
            && named_after.st_ino == opened_after.st_ino
            && named_after.st_size == opened_after.st_size
            && named_after.st_nlink == 1
            && FileType::from_raw_mode(named_after.st_mode) == FileType::RegularFile
            && named_after.st_mtime == opened_after.st_mtime
            && named_after.st_mtime_nsec == opened_after.st_mtime_nsec
            && named_after.st_mode & 0o077 == 0,
        "Sentinel credentials key file changed while it was read"
    );
    let encoded = String::from_utf8(bytes).context("credentials key file is not UTF-8")?;
    let decoded = STANDARD
        .decode(encoded.trim())
        .context("credentials key file must contain base64")?;
    decoded
        .try_into()
        .map_err(|_| anyhow::anyhow!("credentials key must decode to exactly 32 bytes"))
}

pub fn upgrade_sentinel(options: &SentinelUpgradeOptions) -> anyhow::Result<SentinelUpgradeResult> {
    upgrade_sentinel_with_hook(options, |_| Ok(()))
}

fn upgrade_sentinel_with_hook(
    options: &SentinelUpgradeOptions,
    hook: impl FnMut(RestorePoint) -> anyhow::Result<()>,
) -> anyhow::Result<SentinelUpgradeResult> {
    validate_exact_selection(options.product, &options.from_version, &options.to_version)?;
    validate_source_paths(options)?;
    let locks = SentinelServiceLocks::acquire(
        &options.database,
        &options.runtime_directory,
        options.product,
    )?;

    let source_clone = SourceClone::create(&locks.maintenance, options.product)?;
    let source_identity = verify_source_database(&source_clone.database(), ADAPTER)
        .context("verify exact Sentinel 0.1 database contract")?;
    let source_summary =
        summarize_database(&source_clone.database(), SentinelDatabaseContract::Source)?;
    let source_credentials =
        inspect_source_credentials(&source_clone.database(), &options.credentials_key)?;
    let external = inspect_external_sources(options, &source_summary)?;

    let source_backup = create_composite_backup(
        options,
        &source_clone,
        &source_identity,
        &source_summary,
        &external,
    )?;
    ensure!(
        inspect_source_credentials(
            &source_backup.directory.join(DATABASE_FILE),
            &options.credentials_key,
        )? == source_credentials,
        "Sentinel credential plaintext changed while publishing the source backup"
    );

    let staging = TargetStaging::create(&locks.maintenance, options.product)?;
    let target_identity = create_sentinel_target_database(
        &source_backup.directory.join(DATABASE_FILE),
        &staging.database(),
        &options.credentials_key,
        &source_credentials,
    )?;
    let target_summary = summarize_database(&staging.database(), SentinelDatabaseContract::Target)?;
    ensure!(
        target_summary == source_summary,
        "Sentinel target data summary differs from the verified source"
    );
    inspect_current_credentials(&staging.database(), &options.credentials_key)?;
    cross_check_recordings(&target_summary, &external.recordings)?;
    external.ensure_unchanged(options)?;
    source_clone.ensure_source_unchanged()?;

    replace_with_staged_database(
        &locks.maintenance,
        &staging.directory(),
        options.product,
        &target_identity,
        hook,
    )?;

    Ok(SentinelUpgradeResult {
        product: options.product,
        from_version: options.from_version.clone(),
        to_version: options.to_version.clone(),
        source_backup: source_backup.directory,
        database: super::super::absolute_path(&options.database)?,
        schema_identity: target_identity,
    })
}

pub fn recover_sentinel_upgrade(
    options: &SentinelRecoveryOptions,
) -> anyhow::Result<RecoveryResult> {
    validate_exact_selection(options.product, &options.from_version, &options.to_version)?;
    for path in [
        &options.database,
        &options.runtime_directory,
        &options.recovery_directory,
    ] {
        ensure!(
            path.is_absolute(),
            "Sentinel recovery paths must be absolute"
        );
        super::super::absolute_path(path)?;
    }
    let locks = SentinelServiceLocks::acquire(
        &options.database,
        &options.runtime_directory,
        options.product,
    )?;
    let target_identity = target_schema_identity();
    recover_sqlite_restore_under_lock_with_verifier(
        &options.recovery_directory,
        options.product,
        &options.database,
        &target_identity,
        &locks.maintenance,
        options.action,
        |candidate| {
            let credentials_key = options
                .credentials_key
                .as_ref()
                .context("Sentinel recovery commit requires the exact external credentials key")?;
            verify_sentinel_target_database(candidate, credentials_key).map(|_| ())
        },
    )
}

fn validate_exact_selection(
    product: Product,
    from_version: &str,
    to_version: &str,
) -> anyhow::Result<()> {
    ensure!(
        product == Product::SentinelMonitor
            && from_version == ADAPTER.from_version
            && to_version == ADAPTER.to_version,
        "no exact composite adapter for {product} {from_version} -> {to_version}"
    );
    Ok(())
}

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
        "Sentinel 0.1 SQLx ledger must contain exactly three migrations"
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
            "Sentinel 0.1 SQLx migration {version} identity is invalid"
        );
    }
    validate_database_rows(connection, SentinelDatabaseContract::Source)
}

fn copy_rows(_connection: &Connection) -> anyhow::Result<()> {
    anyhow::bail!("Sentinel target creation requires the explicit credentials key")
}

fn copy_rows_with_credentials(
    connection: &Connection,
    credentials_key: &[u8; 32],
) -> anyhow::Result<()> {
    let transaction = connection.unchecked_transaction()?;
    copy_table_rows(
        &transaction,
        "users",
        "id,email,password_hash,role,active,last_login_at,created_at,updated_at,session_version",
    )?;
    copy_camera_rows(&transaction, credentials_key)?;
    let copies = [
        (
            "events",
            "id,camera_id,kind,severity,message,details,acknowledged_at,acknowledged_by,created_at",
        ),
        (
            "audit_logs",
            "id,user_id,action,entity_type,entity_id,details,created_at",
        ),
        (
            "browser_sessions",
            "id,user_id,token_digest,csrf_digest,session_version,created_at,last_seen_at,idle_expires_at,absolute_expires_at,revoked_at",
        ),
        (
            "media_desired_states",
            "camera_id,generation,desired_present,main_path,sub_path,record_enabled,updated_at",
        ),
        (
            "media_operations",
            "id,camera_id,generation,kind,state,reason,requested_by,attempt,created_at,started_at,finished_at,retry_at,lease_owner,lease_expires_at,result_json,error_code,error_message",
        ),
        (
            "media_actual_paths",
            "path_name,camera_id,profile,present,ready,publisher_active,recording_active,source_digest,source_on_demand,record_configured,applied_generation,last_operation_id,observed_at",
        ),
    ];
    for (table, columns) in copies {
        copy_table_rows(&transaction, table, columns)?;
    }
    transaction.commit()?;
    validate_database_rows(connection, SentinelDatabaseContract::Target)
}

fn copy_table_rows(
    transaction: &rusqlite::Transaction<'_>,
    table: &str,
    columns: &str,
) -> anyhow::Result<()> {
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
        "row-count mismatch while copying Sentinel {table}"
    );
    Ok(())
}

fn copy_camera_rows(
    transaction: &rusqlite::Transaction<'_>,
    credentials_key: &[u8; 32],
) -> anyhow::Result<()> {
    let transformer = CredentialTransformer::new(credentials_key);
    let mut select = transaction.prepare(
        "SELECT id,name,location,main_stream_url_enc,sub_stream_url_enc,onvif_url,username,\
                password_enc,enabled,record_enabled,status,last_seen_at,created_by,created_at,\
                updated_at,deleted_at FROM legacy.cameras ORDER BY id",
    )?;
    let mut rows = select.query([])?;
    let mut copied = 0_i64;
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let camera_id = parse_camera_id(&id)?;
        let main = transformer.decrypt_legacy(&row.get::<_, Vec<u8>>(3)?)?;
        let sub = row
            .get::<_, Option<Vec<u8>>>(4)?
            .map(|value| transformer.decrypt_legacy(&value))
            .transpose()?;
        let username = row.get::<_, Option<String>>(6)?;
        validate_plaintext(username.as_deref())?;
        let password = row
            .get::<_, Option<Vec<u8>>>(7)?
            .map(|value| transformer.decrypt_legacy(&value))
            .transpose()?;
        let main = transformer.encrypt_current(camera_id, CredentialField::MainStreamUrl, &main)?;
        let sub = sub
            .as_deref()
            .map(|value| {
                transformer.encrypt_current(camera_id, CredentialField::SubStreamUrl, value)
            })
            .transpose()?;
        let username = username
            .as_deref()
            .map(|value| transformer.encrypt_current(camera_id, CredentialField::Username, value))
            .transpose()?;
        let password = password
            .as_deref()
            .map(|value| transformer.encrypt_current(camera_id, CredentialField::Password, value))
            .transpose()?;
        transaction.execute(
            "INSERT INTO main.cameras (id,name,location,main_stream_url_enc,sub_stream_url_enc,\
                 onvif_url,username_enc,password_enc,enabled,record_enabled,status,last_seen_at,\
                 created_by,created_at,updated_at,deleted_at)\
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
            rusqlite::params![
                id,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                main,
                sub,
                row.get::<_, Option<String>>(5)?,
                username,
                password,
                row.get::<_, i64>(8)?,
                row.get::<_, i64>(9)?,
                row.get::<_, String>(10)?,
                row.get::<_, Option<String>>(11)?,
                row.get::<_, Option<String>>(12)?,
                row.get::<_, String>(13)?,
                row.get::<_, String>(14)?,
                row.get::<_, Option<String>>(15)?,
            ],
        )?;
        copied += 1;
    }
    let source_rows: i64 =
        transaction.query_row("SELECT COUNT(*) FROM legacy.cameras", [], |row| row.get(0))?;
    let target_rows: i64 =
        transaction.query_row("SELECT COUNT(*) FROM main.cameras", [], |row| row.get(0))?;
    ensure!(
        copied == source_rows && target_rows == source_rows,
        "row-count mismatch while transforming Sentinel cameras"
    );
    Ok(())
}

fn create_sentinel_target_database(
    source_backup: &Path,
    target: &Path,
    credentials_key: &[u8; 32],
    expected_credentials: &CredentialInventory,
) -> anyhow::Result<SchemaIdentity> {
    create_private_empty_file(target)?;
    let connection = Connection::open_with_flags(
        target,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.busy_timeout(Duration::from_secs(3))?;
    connection.pragma_update(None, "trusted_schema", "OFF")?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "journal_mode", "DELETE")?;
    connection.pragma_update(None, "synchronous", "FULL")?;
    connection.execute_batch(TARGET_SCHEMA_SQL)?;
    connection.execute_batch(PRODUCT_METADATA_DDL)?;
    let fingerprint = schema_fingerprint_connection(&connection)?;
    ensure!(
        fingerprint == TARGET_SCHEMA_SHA256,
        "embedded Sentinel target schema does not match the pinned product contract"
    );
    connection.execute(
        "INSERT INTO product_metadata (singleton,application,application_version,\
             schema_revision,schema_sha256) VALUES (1,?1,?2,?3,?4)",
        rusqlite::params![
            Product::SentinelMonitor.slug(),
            ADAPTER.to_version,
            i64::try_from(ADAPTER.target_revision)?,
            TARGET_SCHEMA_SHA256,
        ],
    )?;
    let source_uri = sqlite_read_only_uri(source_backup)?;
    connection.execute("ATTACH DATABASE ?1 AS legacy", [source_uri])?;
    let copy = copy_rows_with_credentials(&connection, credentials_key);
    let detach = connection.execute_batch("DETACH DATABASE legacy;");
    copy?;
    detach?;
    let integrity: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    ensure!(
        integrity.eq_ignore_ascii_case("ok"),
        "Sentinel target SQLite integrity check failed"
    );
    let mut foreign_keys = connection.prepare("PRAGMA foreign_key_check")?;
    ensure!(
        foreign_keys.query([])?.next()?.is_none(),
        "Sentinel target SQLite foreign-key check failed"
    );
    drop(foreign_keys);
    connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    drop(connection);
    File::open(target)?.sync_all()?;
    sync_directory(
        target
            .parent()
            .context("Sentinel target database has no parent")?,
    )?;
    let identity = verify_sentinel_target_database(target, credentials_key)?;
    ensure!(
        inspect_current_credentials(target, credentials_key)? == *expected_credentials,
        "Sentinel credential plaintext changed during the exact 0.2 envelope conversion"
    );
    Ok(identity)
}

fn verify_sentinel_target_database(
    target: &Path,
    credentials_key: &[u8; 32],
) -> anyhow::Result<SchemaIdentity> {
    let identity = verify_current_database(target, Product::SentinelMonitor)?;
    ensure!(
        identity == target_schema_identity(),
        "Sentinel target database identity is not the pinned current product contract"
    );
    let connection = Connection::open_with_flags(
        target,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    validate_database_rows(&connection, SentinelDatabaseContract::Target)?;
    drop(connection);
    inspect_current_credentials(target, credentials_key)?;
    Ok(identity)
}

#[derive(Clone, Copy)]
enum SentinelDatabaseContract {
    Source,
    Target,
}

fn summarize_database(
    path: &Path,
    contract: SentinelDatabaseContract,
) -> anyhow::Result<SentinelDatabaseSummary> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    validate_database_rows(&connection, contract)?;
    let mut records = BTreeMap::new();
    for table in DATA_TABLES {
        let count: i64 =
            connection.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })?;
        ensure!(count >= 0, "negative Sentinel table count");
        records.insert(table.to_owned(), count as u64);
    }
    let mut recording_paths = BTreeSet::new();
    let mut cameras = connection.prepare(
        "SELECT camera.id, desired.main_path, desired.sub_path
           FROM cameras camera
           JOIN media_desired_states desired ON desired.camera_id=camera.id
          ORDER BY camera.id",
    )?;
    let rows = cameras.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
        ))
    })?;
    for row in rows {
        let (id, main_path, sub_path) = row?;
        Uuid::parse_str(&id).context("camera ID is not a UUID")?;
        recording_paths.insert(main_path);
        if let Some(sub_path) = sub_path {
            recording_paths.insert(sub_path);
        }
    }
    Ok(SentinelDatabaseSummary {
        records,
        recording_paths,
    })
}

fn validate_database_rows(
    connection: &Connection,
    contract: SentinelDatabaseContract,
) -> anyhow::Result<()> {
    let invalid_json: i64 = connection.query_row(
        "SELECT
           (SELECT COUNT(*) FROM events WHERE json_valid(details) <> 1) +
           (SELECT COUNT(*) FROM audit_logs WHERE json_valid(details) <> 1) +
           (SELECT COUNT(*) FROM media_operations
             WHERE result_json IS NOT NULL AND json_valid(result_json) <> 1)",
        [],
        |row| row.get(0),
    )?;
    ensure!(invalid_json == 0, "Sentinel contains invalid stored JSON");

    let invalid_audit_state: i64 = connection.query_row(
        "SELECT
           (SELECT COUNT(*) FROM events
             WHERE trim(kind)='' OR trim(message)='' OR datetime(created_at) IS NULL
                OR (acknowledged_at IS NOT NULL AND datetime(acknowledged_at) IS NULL)
                OR (acknowledged_by IS NOT NULL AND acknowledged_at IS NULL)) +
           (SELECT COUNT(*) FROM audit_logs
             WHERE trim(action)='' OR trim(entity_type)='' OR datetime(created_at) IS NULL)",
        [],
        |row| row.get(0),
    )?;
    ensure!(
        invalid_audit_state == 0,
        "Sentinel event or audit state is invalid"
    );

    let invalid_sessions: i64 = connection.query_row(
        "SELECT COUNT(*) FROM browser_sessions
         WHERE session_version <= 0
            OR datetime(created_at) IS NULL OR datetime(last_seen_at) IS NULL
            OR datetime(idle_expires_at) IS NULL OR datetime(absolute_expires_at) IS NULL
            OR (revoked_at IS NOT NULL AND datetime(revoked_at) IS NULL)",
        [],
        |row| row.get(0),
    )?;
    ensure!(
        invalid_sessions == 0,
        "Sentinel browser session state is invalid"
    );

    let invalid_desired: i64 = connection.query_row(
        "SELECT COUNT(*) FROM cameras camera
         LEFT JOIN media_desired_states desired ON desired.camera_id = camera.id
         WHERE desired.camera_id IS NULL
            OR desired.main_path <> 'cam_' || lower(replace(camera.id, '-', '')) || '_main'
            OR desired.sub_path IS NOT CASE WHEN camera.sub_stream_url_enc IS NULL THEN NULL
                 ELSE 'cam_' || lower(replace(camera.id, '-', '')) || '_sub' END
            OR desired.record_enabled <> camera.record_enabled",
        [],
        |row| row.get(0),
    )?;
    ensure!(
        invalid_desired == 0,
        "Sentinel desired media state is inconsistent with cameras"
    );
    let extra_desired: i64 = connection.query_row(
        "SELECT COUNT(*) FROM media_desired_states desired
         WHERE NOT EXISTS (SELECT 1 FROM cameras WHERE id=desired.camera_id)
            OR NOT EXISTS (
                SELECT 1 FROM media_operations operation
                 WHERE operation.camera_id=desired.camera_id
                   AND operation.generation=desired.generation
            )",
        [],
        |row| row.get(0),
    )?;
    ensure!(
        extra_desired == 0,
        "Sentinel desired generation has no matching operation"
    );

    let invalid_operations: i64 = connection.query_row(
        "SELECT COUNT(*) FROM media_operations operation
         JOIN media_desired_states desired ON desired.camera_id=operation.camera_id
         WHERE operation.generation > desired.generation
            OR ((operation.lease_owner IS NULL) <> (operation.lease_expires_at IS NULL))
            OR (operation.state='running' AND (
                  operation.attempt <= 0 OR operation.started_at IS NULL
                  OR operation.finished_at IS NOT NULL OR operation.lease_owner IS NULL
                ))
            OR (operation.state <> 'running' AND operation.lease_owner IS NOT NULL)
            OR (operation.state IN ('succeeded','failed') AND operation.finished_at IS NULL)",
        [],
        |row| row.get(0),
    )?;
    ensure!(
        invalid_operations == 0,
        "Sentinel operation state is invalid"
    );

    let invalid_actual: i64 = connection.query_row(
        "SELECT COUNT(*) FROM media_actual_paths actual
         JOIN media_desired_states desired ON desired.camera_id=actual.camera_id
         WHERE actual.path_name <> 'cam_' || lower(replace(actual.camera_id, '-', '')) || '_' || actual.profile
            OR (actual.profile='sub' AND desired.sub_path IS NULL)
            OR (actual.applied_generation IS NOT NULL
                AND actual.applied_generation > desired.generation)
            OR (actual.last_operation_id IS NOT NULL AND NOT EXISTS (
                SELECT 1 FROM media_operations operation
                 WHERE operation.id=actual.last_operation_id
                   AND operation.camera_id=actual.camera_id
                   AND (actual.applied_generation IS NULL
                        OR operation.generation=actual.applied_generation)
            ))",
        [],
        |row| row.get(0),
    )?;
    ensure!(
        invalid_actual == 0,
        "Sentinel actual media state is invalid"
    );

    match contract {
        SentinelDatabaseContract::Source => {
            let invalid_leases: i64 = connection.query_row(
                "SELECT COUNT(*) FROM media_reconciler_leases
                  WHERE scope <> 'global'
                     OR ((lease_owner IS NULL) <> (lease_expires_at IS NULL))
                     OR (lease_expires_at IS NOT NULL AND datetime(lease_expires_at) IS NULL)
                     OR datetime(updated_at) IS NULL",
                [],
                |row| row.get(0),
            )?;
            ensure!(
                invalid_leases == 0,
                "Sentinel 0.1 global reconciler lease is invalid"
            );
        }
        SentinelDatabaseContract::Target => {
            let exact_reset_lease: i64 = connection.query_row(
                "SELECT COUNT(*) FROM media_reconciler_leases
                  WHERE typeof(singleton)='integer' AND singleton=1
                    AND typeof(lease_owner)='null' AND lease_owner IS NULL
                    AND typeof(lease_expires_at)='null' AND lease_expires_at IS NULL
                    AND typeof(updated_at)='text'
                    AND updated_at='1970-01-01T00:00:00+00:00'",
                [],
                |row| row.get(0),
            )?;
            ensure!(
                exact_reset_lease == 1,
                "Sentinel 0.2 global reconciler lease is not the exact reset state"
            );
        }
    }
    let lease_rows: i64 =
        connection.query_row("SELECT COUNT(*) FROM media_reconciler_leases", [], |row| {
            row.get(0)
        })?;
    ensure!(
        lease_rows == 1,
        "Sentinel must contain exactly one global reconciler lease"
    );
    Ok(())
}

fn inspect_source_credentials(path: &Path, key: &[u8; 32]) -> anyhow::Result<CredentialInventory> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let transformer = CredentialTransformer::new(key);
    let mut statement = connection.prepare(
        "SELECT id,main_stream_url_enc,sub_stream_url_enc,username,password_enc \
         FROM cameras ORDER BY id",
    )?;
    let mut rows = statement.query([])?;
    let mut inventory = CredentialInventory::new();
    while let Some(row) = rows.next()? {
        let camera_id = parse_camera_id(&row.get::<_, String>(0)?)?;
        let main = transformer.decrypt_legacy(&row.get::<_, Vec<u8>>(1)?)?;
        insert_credential_digest(
            &mut inventory,
            camera_id,
            CredentialField::MainStreamUrl,
            &main,
        )?;
        let sub = row
            .get::<_, Option<Vec<u8>>>(2)?
            .map(|value| transformer.decrypt_legacy(&value))
            .transpose()?;
        if let Some(sub) = sub.as_deref() {
            insert_credential_digest(
                &mut inventory,
                camera_id,
                CredentialField::SubStreamUrl,
                sub,
            )?;
        }
        let username = row.get::<_, Option<String>>(3)?;
        validate_plaintext(username.as_deref())?;
        if let Some(username) = username.as_deref() {
            insert_credential_digest(
                &mut inventory,
                camera_id,
                CredentialField::Username,
                username,
            )?;
        }
        let password = row
            .get::<_, Option<Vec<u8>>>(4)?
            .map(|value| transformer.decrypt_legacy(&value))
            .transpose()?;
        if let Some(password) = password.as_deref() {
            insert_credential_digest(
                &mut inventory,
                camera_id,
                CredentialField::Password,
                password,
            )?;
        }
    }
    Ok(inventory)
}

fn inspect_current_credentials(path: &Path, key: &[u8; 32]) -> anyhow::Result<CredentialInventory> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let transformer = CredentialTransformer::new(key);
    let mut statement = connection.prepare(
        "SELECT id,main_stream_url_enc,sub_stream_url_enc,username_enc,password_enc \
         FROM cameras ORDER BY id",
    )?;
    let mut rows = statement.query([])?;
    let mut inventory = CredentialInventory::new();
    while let Some(row) = rows.next()? {
        let camera_id = parse_camera_id(&row.get::<_, String>(0)?)?;
        for (index, field) in CredentialField::ALL.into_iter().enumerate() {
            let encoded = if field == CredentialField::MainStreamUrl {
                Some(row.get::<_, Vec<u8>>(index + 1)?)
            } else {
                row.get::<_, Option<Vec<u8>>>(index + 1)?
            };
            if let Some(encoded) = encoded {
                let plaintext = transformer.decrypt_current(camera_id, field, &encoded)?;
                insert_credential_digest(&mut inventory, camera_id, field, &plaintext)?;
            }
        }
    }
    Ok(inventory)
}

fn parse_camera_id(value: &str) -> anyhow::Result<Uuid> {
    Uuid::parse_str(value).map_err(|_| anyhow::anyhow!("stored Sentinel camera ID is malformed"))
}

fn validate_plaintext(value: Option<&str>) -> anyhow::Result<()> {
    ensure!(
        value.is_none_or(|value| value.len() <= MAX_CREDENTIAL_PLAINTEXT_BYTES),
        "Sentinel credential exceeds the 0.2 plaintext limit"
    );
    Ok(())
}

fn insert_credential_digest(
    inventory: &mut CredentialInventory,
    camera_id: Uuid,
    field: CredentialField,
    plaintext: &str,
) -> anyhow::Result<()> {
    validate_plaintext(Some(plaintext))?;
    let mut digest = Sha256::new();
    digest.update(b"sentinel-upgrade-credential-plaintext-v1\0");
    digest.update((field.database_name().len() as u64).to_be_bytes());
    digest.update(field.database_name().as_bytes());
    digest.update((plaintext.len() as u64).to_be_bytes());
    digest.update(plaintext.as_bytes());
    ensure!(
        inventory
            .insert((camera_id, field), digest.finalize().into())
            .is_none(),
        "Sentinel credential inventory contains a duplicate field"
    );
    Ok(())
}

fn credentials_key_sha256(key: &[u8; 32]) -> String {
    super::super::lower_hex(&Sha256::digest(key))
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ExternalSnapshot {
    config: SentinelStoredFile,
    contract_file: SentinelStoredFile,
    contract: ParsedContract,
    recording_root_sha256: String,
    recordings: RecordingInventory,
    database_summary: SentinelDatabaseSummary,
}

impl ExternalSnapshot {
    fn ensure_unchanged(&self, options: &SentinelUpgradeOptions) -> anyhow::Result<()> {
        let current = inspect_external_sources(options, &self.database_summary)?;
        ensure!(
            &current == self,
            "Sentinel MediaMTX or recording resources changed during upgrade preparation"
        );
        Ok(())
    }
}

fn validate_source_paths(options: &SentinelUpgradeOptions) -> anyhow::Result<()> {
    let inputs = [
        &options.database,
        &options.runtime_directory,
        &options.mediamtx_config,
        &options.mediamtx_contract,
        &options.recordings_directory,
        &options.backup_output,
    ];
    for path in inputs {
        ensure!(
            path.is_absolute(),
            "Sentinel upgrade paths must be absolute"
        );
        super::super::absolute_path(path)?;
    }
    let database = super::super::absolute_path(&options.database)?;
    let runtime = super::super::absolute_path(&options.runtime_directory)?;
    let config = super::super::absolute_path(&options.mediamtx_config)?;
    let contract = super::super::absolute_path(&options.mediamtx_contract)?;
    let recordings = super::super::absolute_path(&options.recordings_directory)?;
    let output = super::super::absolute_path(&options.backup_output)?;
    let paths = [&database, &runtime, &config, &contract, &recordings];
    for (position, left) in paths.iter().enumerate() {
        for right in paths.iter().skip(position + 1) {
            ensure!(left != right, "Sentinel source paths must be distinct");
        }
    }
    ensure!(
        !output.starts_with(&recordings)
            && !recordings.starts_with(&output)
            && !output.starts_with(&runtime)
            && !runtime.starts_with(&output),
        "backup output must be disjoint from runtime and recordings trees"
    );
    for file in [&database, &config, &contract] {
        ensure!(
            !file.starts_with(&recordings) && !recordings.starts_with(file),
            "Sentinel database/config/contract must be outside recordings"
        );
    }
    Ok(())
}

fn inspect_external_sources(
    options: &SentinelUpgradeOptions,
    database_summary: &SentinelDatabaseSummary,
) -> anyhow::Result<ExternalSnapshot> {
    let config_file = SecureFile::open(&options.mediamtx_config, "MediaMTX config")?;
    let contract_file = SecureFile::open(&options.mediamtx_contract, "MediaMTX contract")?;
    let config = parse_media_config(&config_file)?;
    let expected_recording_root = super::super::absolute_path(&options.recordings_directory)?;
    ensure!(
        super::super::absolute_path(&config.recording_root)? == expected_recording_root,
        "MediaMTX config points at a different recordings directory"
    );
    let contract = parse_contract(&contract_file)?;
    validate_exact_contract(&contract)?;
    let recordings_root = SecureDirectory::open(
        &options.recordings_directory,
        "Sentinel recordings directory",
    )?;
    let recordings = inventory_recordings(&recordings_root)?;
    cross_check_recordings(database_summary, &recordings)?;
    Ok(ExternalSnapshot {
        config: stored_secure_file(BUNDLE_CONFIG_FILE, &config_file)?,
        contract_file: stored_secure_file(BUNDLE_CONTRACT_FILE, &contract_file)?,
        contract,
        recording_root_sha256: path_identity_sha256(&config.recording_root)?,
        recordings,
        database_summary: database_summary.clone(),
    })
}

fn create_composite_backup(
    options: &SentinelUpgradeOptions,
    source_clone: &SourceClone,
    source_identity: &SchemaIdentity,
    source_summary: &SentinelDatabaseSummary,
    external: &ExternalSnapshot,
) -> anyhow::Result<VerifiedSentinelSourceBackup> {
    let mut pending = PendingDirectory::create(&options.backup_output)?;
    let pending_directory = open_pending_directory(&pending)?;

    let database_output = pending.path().join(DATABASE_FILE);
    create_private_empty_file(&database_output)?;
    copy_database_online(&source_clone.database(), &database_output)?;
    let snapshot_identity = verify_source_database(&database_output, ADAPTER)?;
    ensure!(
        &snapshot_identity == source_identity,
        "Sentinel database identity changed while its backup was created"
    );
    let snapshot_summary = summarize_database(&database_output, SentinelDatabaseContract::Source)?;
    ensure!(
        &snapshot_summary == source_summary,
        "Sentinel database data changed while its backup was created"
    );
    inspect_source_credentials(&database_output, &options.credentials_key)?;

    let source_config = SecureFile::open(&options.mediamtx_config, "MediaMTX config")?;
    let source_contract = SecureFile::open(&options.mediamtx_contract, "MediaMTX contract")?;
    copy_regular_file_at(
        &source_config.parent.file,
        &source_config.name,
        &pending_directory.file,
        OsStr::new(BUNDLE_CONFIG_FILE),
    )?;
    copy_regular_file_at(
        &source_contract.parent.file,
        &source_contract.name,
        &pending_directory.file,
        OsStr::new(BUNDLE_CONTRACT_FILE),
    )?;

    mkdirat(
        &pending_directory.file,
        BUNDLE_RECORDINGS_DIRECTORY,
        Mode::from_raw_mode(0o700),
    )?;
    let recordings_output = open_child_directory(
        &pending_directory.file,
        OsStr::new(BUNDLE_RECORDINGS_DIRECTORY),
        "backup recordings directory",
    )?;
    let recordings_source = SecureDirectory::open(
        &options.recordings_directory,
        "Sentinel recordings directory",
    )?;
    let copied_recordings = copy_recording_tree(&recordings_source, &recordings_output)?;
    ensure!(
        copied_recordings == external.recordings,
        "Sentinel recordings changed while they were copied"
    );

    let database = stored_file(DATABASE_FILE, &database_output)?;
    let config = stored_file(
        BUNDLE_CONFIG_FILE,
        &pending_directory.child_path(BUNDLE_CONFIG_FILE),
    )?;
    let contract_file = stored_file(
        BUNDLE_CONTRACT_FILE,
        &pending_directory.child_path(BUNDLE_CONTRACT_FILE),
    )?;
    ensure!(
        config == external.config && contract_file == external.contract_file,
        "Sentinel config or contract changed while it was copied"
    );
    cross_check_recordings(&snapshot_summary, &copied_recordings)?;

    let manifest = SentinelSourceBackupManifest {
        manifest_version: BUNDLE_MANIFEST_VERSION,
        tool_version: env!("CARGO_PKG_VERSION").to_owned(),
        product: options.product,
        from_version: options.from_version.clone(),
        to_version: options.to_version.clone(),
        source_schema_identity: snapshot_identity,
        target_schema_identity: target_schema_identity(),
        target_source_commit: TARGET_SOURCE_COMMIT.to_owned(),
        credential_envelope_contract: credential_envelope_contract(),
        created_at_epoch_seconds: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock is before the Unix epoch")?
            .as_secs(),
        database,
        database_records: snapshot_summary.records,
        mediamtx_config: config,
        mediamtx_contract: SentinelCompanionContract {
            contract_file,
            version: external.contract.version.clone(),
            platform: external.contract.platform.clone(),
            binary_sha256: external.contract.binary_sha256.clone(),
        },
        recording_root_sha256: external.recording_root_sha256.clone(),
        recordings: SentinelRecordingArchive {
            directory: BUNDLE_RECORDINGS_DIRECTORY.to_owned(),
            directories: copied_recordings.directories,
            files: copied_recordings.files,
            bytes: copied_recordings.bytes,
        },
        credentials_key_included: false,
        credentials_key_required_for_upgrade: true,
        credentials_key_sha256: credentials_key_sha256(&options.credentials_key),
    };
    validate_manifest(&manifest)?;
    write_bundle_manifest(
        &pending_directory.child_path(BUNDLE_MANIFEST_FILE),
        &manifest,
    )?;
    pending_directory.file.sync_all()?;
    pending.commit()?;
    verify_sentinel_source_backup(
        options.product,
        &options.from_version,
        &options.to_version,
        &options.backup_output,
        &options.credentials_key,
    )
}

pub fn verify_sentinel_source_backup(
    product: Product,
    from_version: &str,
    to_version: &str,
    input: &Path,
    credentials_key: &[u8; 32],
) -> anyhow::Result<VerifiedSentinelSourceBackup> {
    validate_exact_selection(product, from_version, to_version)?;
    let root = SecureDirectory::open(input, "Sentinel source backup")?;
    let manifest: SentinelSourceBackupManifest =
        serde_json::from_slice(&root.read_bounded(BUNDLE_MANIFEST_FILE, MAX_MANIFEST_BYTES)?)?;
    validate_manifest(&manifest)?;
    ensure!(
        manifest.product == product
            && manifest.from_version == from_version
            && manifest.to_version == to_version,
        "Sentinel source backup does not match the explicit adapter"
    );
    ensure!(
        manifest.credentials_key_sha256 == credentials_key_sha256(credentials_key),
        "credentials key does not match the Sentinel source backup requirement"
    );
    verify_bundle_inventory(&root, &manifest)?;
    verify_stored_file(&root, &manifest.database)?;
    verify_stored_file(&root, &manifest.mediamtx_config)?;
    verify_stored_file(&root, &manifest.mediamtx_contract.contract_file)?;

    let database_path = root.child_path(DATABASE_FILE);
    let identity = verify_source_database(&database_path, ADAPTER)?;
    ensure!(
        identity == manifest.source_schema_identity,
        "Sentinel source backup schema identity mismatch"
    );
    let summary = summarize_database(&database_path, SentinelDatabaseContract::Source)?;
    ensure!(
        summary.records == manifest.database_records,
        "Sentinel source backup table counts mismatch"
    );
    inspect_source_credentials(&database_path, credentials_key)?;

    let config_file = SecureFile::from_directory(
        &root,
        OsStr::new(BUNDLE_CONFIG_FILE),
        "backup MediaMTX config",
    )?;
    let contract_file = SecureFile::from_directory(
        &root,
        OsStr::new(BUNDLE_CONTRACT_FILE),
        "backup MediaMTX contract",
    )?;
    let config = parse_media_config(&config_file)?;
    ensure!(
        path_identity_sha256(&config.recording_root)? == manifest.recording_root_sha256,
        "backup MediaMTX recording root identity mismatch"
    );
    let contract = parse_contract(&contract_file)?;
    validate_exact_contract(&contract)?;
    ensure!(
        contract.version == manifest.mediamtx_contract.version
            && contract.platform == manifest.mediamtx_contract.platform
            && contract.binary_sha256 == manifest.mediamtx_contract.binary_sha256,
        "backup MediaMTX contract manifest mismatch"
    );

    let recordings_root = open_child_directory(
        &root.file,
        OsStr::new(BUNDLE_RECORDINGS_DIRECTORY),
        "backup recordings directory",
    )?;
    let recordings = inventory_recordings(&recordings_root)?;
    ensure!(
        recordings.directories == manifest.recordings.directories
            && recordings.files == manifest.recordings.files
            && recordings.bytes == manifest.recordings.bytes,
        "Sentinel recording backup inventory mismatch"
    );
    cross_check_recordings(&summary, &recordings)?;

    Ok(VerifiedSentinelSourceBackup {
        directory: super::super::absolute_path(input)?,
        manifest,
    })
}

fn validate_manifest(manifest: &SentinelSourceBackupManifest) -> anyhow::Result<()> {
    ensure!(
        manifest.manifest_version == BUNDLE_MANIFEST_VERSION,
        "unsupported Sentinel source backup manifest version"
    );
    validate_exact_selection(
        manifest.product,
        &manifest.from_version,
        &manifest.to_version,
    )?;
    ensure!(
        manifest.source_schema_identity.application == Product::SentinelMonitor.slug()
            && manifest.source_schema_identity.application_version == ADAPTER.from_version
            && manifest.source_schema_identity.schema_revision == ADAPTER.source_revision
            && manifest.source_schema_identity.schema_sha256 == ADAPTER.source_schema_sha256,
        "Sentinel source backup has the wrong database identity"
    );
    ensure!(
        manifest.target_schema_identity == target_schema_identity()
            && manifest.target_source_commit == TARGET_SOURCE_COMMIT
            && manifest.credential_envelope_contract == credential_envelope_contract(),
        "Sentinel source backup has the wrong pinned 0.2 target contract"
    );
    ensure!(
        manifest.database.path == DATABASE_FILE
            && manifest.mediamtx_config.path == BUNDLE_CONFIG_FILE
            && manifest.mediamtx_contract.contract_file.path == BUNDLE_CONTRACT_FILE
            && manifest.recordings.directory == BUNDLE_RECORDINGS_DIRECTORY,
        "Sentinel source backup uses unexpected resource paths"
    );
    for file in [
        &manifest.database,
        &manifest.mediamtx_config,
        &manifest.mediamtx_contract.contract_file,
    ] {
        validate_stored_file(file, false)?;
    }
    ensure!(
        manifest.mediamtx_contract.version == CONTRACT_VERSION
            && manifest.mediamtx_contract.platform == CONTRACT_PLATFORM
            && manifest.mediamtx_contract.binary_sha256 == CONTRACT_BINARY_SHA256,
        "Sentinel backup companion contract is unsupported"
    );
    validate_sha256(&manifest.recording_root_sha256)?;
    ensure!(
        !manifest.credentials_key_included && manifest.credentials_key_required_for_upgrade,
        "Sentinel backups must require but never contain the credentials key"
    );
    validate_sha256(&manifest.credentials_key_sha256)?;
    let expected_tables = DATA_TABLES
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    ensure!(
        manifest
            .database_records
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>()
            == expected_tables,
        "Sentinel backup table-count inventory is incomplete"
    );
    ensure!(
        manifest.recordings.files.len() <= MAX_RECORDING_FILES,
        "Sentinel recording manifest has too many files"
    );
    ensure!(
        manifest.recordings.directories.len() <= MAX_RECORDING_DIRECTORIES,
        "Sentinel recording manifest has too many directories"
    );
    ensure!(
        manifest
            .recordings
            .files
            .len()
            .checked_add(manifest.recordings.directories.len())
            .is_some_and(|entries| entries <= MAX_RECORDING_ENTRIES),
        "Sentinel recording manifest has too many total entries"
    );
    ensure_strictly_sorted_unique(&manifest.recordings.directories)?;
    let file_paths = manifest
        .recordings
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect::<Vec<_>>();
    ensure_strictly_sorted_unique(&file_paths)?;
    let mut bytes = 0_u64;
    for directory in &manifest.recordings.directories {
        validate_relative_path(directory)?;
    }
    for file in &manifest.recordings.files {
        validate_stored_file(file, true)?;
        bytes = bytes
            .checked_add(file.bytes)
            .context("Sentinel recording byte count overflow")?;
    }
    ensure!(
        bytes == manifest.recordings.bytes,
        "Sentinel recording byte total mismatch"
    );
    Ok(())
}

fn write_bundle_manifest(
    path: &Path,
    manifest: &SentinelSourceBackupManifest,
) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec_pretty(manifest)?;
    ensure!(
        bytes.len() as u64 <= MAX_MANIFEST_BYTES,
        "Sentinel source backup manifest is too large"
    );
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

fn stored_file(path: &str, filesystem_path: &Path) -> anyhow::Result<SentinelStoredFile> {
    let (bytes, sha256) = hash_regular_file(filesystem_path)?;
    Ok(SentinelStoredFile {
        path: path.to_owned(),
        bytes,
        sha256,
    })
}

fn stored_secure_file(path: &str, file: &SecureFile) -> anyhow::Result<SentinelStoredFile> {
    stored_file(path, &file.path())
}

fn verify_stored_file(root: &SecureDirectory, stored: &SentinelStoredFile) -> anyhow::Result<()> {
    validate_stored_file(stored, false)?;
    let (bytes, sha256) = hash_regular_file(&root.child_path(&stored.path))?;
    ensure!(
        bytes == stored.bytes && sha256 == stored.sha256,
        "Sentinel backup resource hash mismatch"
    );
    Ok(())
}

fn validate_stored_file(file: &SentinelStoredFile, nested: bool) -> anyhow::Result<()> {
    validate_relative_path(&file.path)?;
    if !nested {
        ensure!(
            Path::new(&file.path).components().count() == 1,
            "top-level Sentinel backup resource path is nested"
        );
    }
    validate_sha256(&file.sha256)
}

fn validate_sha256(value: &str) -> anyhow::Result<()> {
    ensure!(
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "SHA-256 is not canonical lowercase hexadecimal"
    );
    Ok(())
}

fn validate_relative_path(value: &str) -> anyhow::Result<()> {
    ensure!(
        !value.is_empty() && value.len() <= 4096,
        "Sentinel backup path is empty or too long"
    );
    let path = Path::new(value);
    ensure!(
        !path.is_absolute()
            && path
                .components()
                .all(|component| matches!(component, Component::Normal(_))),
        "Sentinel backup path is unsafe"
    );
    ensure!(
        path.components().count() <= MAX_RECORDING_PATH_COMPONENTS,
        "Sentinel backup path is too deeply nested"
    );
    ensure!(
        !value.contains('\\') && !value.contains("//"),
        "Sentinel backup path is not portable"
    );
    Ok(())
}

fn ensure_strictly_sorted_unique(values: &[String]) -> anyhow::Result<()> {
    ensure!(
        values.windows(2).all(|window| window[0] < window[1]),
        "Sentinel backup inventory must be strictly sorted and unique"
    );
    Ok(())
}

fn path_identity_sha256(path: &Path) -> anyhow::Result<String> {
    let path = super::super::absolute_path(path)?;
    let value = path.to_str().context("recording root path must be UTF-8")?;
    Ok(super::super::lower_hex(&Sha256::digest(value.as_bytes())))
}

fn parse_contract(file: &SecureFile) -> anyhow::Result<ParsedContract> {
    let content = String::from_utf8(file.read_bounded(MAX_CONTRACT_BYTES)?)
        .context("MediaMTX contract is not UTF-8")?;
    let mut values = BTreeMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .context("MediaMTX contract contains an invalid line")?;
        ensure!(
            matches!(key, "version" | "platform" | "sha256"),
            "MediaMTX contract contains an unknown field"
        );
        ensure!(
            values.insert(key, value.trim()).is_none(),
            "MediaMTX contract contains a duplicate field"
        );
    }
    ensure!(values.len() == 3, "MediaMTX contract is incomplete");
    Ok(ParsedContract {
        version: values["version"].to_owned(),
        platform: values["platform"].to_owned(),
        binary_sha256: values["sha256"].to_ascii_lowercase(),
    })
}

fn validate_exact_contract(contract: &ParsedContract) -> anyhow::Result<()> {
    ensure!(
        contract.version == CONTRACT_VERSION
            && contract.platform == CONTRACT_PLATFORM
            && contract.binary_sha256 == CONTRACT_BINARY_SHA256,
        "MediaMTX contract is not the exact Sentinel 0.1 companion contract"
    );
    Ok(())
}

fn parse_media_config(file: &SecureFile) -> anyhow::Result<ParsedMediaConfig> {
    let content = String::from_utf8(file.read_bounded(MAX_CONFIG_BYTES)?)
        .context("MediaMTX config is not UTF-8")?;
    for (key, expected) in [
        ("authMethod", "http"),
        (
            "authHTTPAddress",
            "http://127.0.0.1:8080/internal/media/auth",
        ),
        ("apiAddress", "127.0.0.1:9997"),
        ("playbackAddress", "127.0.0.1:9996"),
        ("recordFormat", "fmp4"),
    ] {
        let values = config_values(&content, key);
        ensure!(
            values.len() == 1 && values[0].trim_matches(['\'', '"']) == expected,
            "MediaMTX config has a missing, duplicate, or invalid required setting"
        );
    }
    let record_paths = config_values(&content, "recordPath");
    ensure!(
        record_paths.len() == 1,
        "MediaMTX config must declare one recordPath"
    );
    let record_path = record_paths[0].trim_matches(['\'', '"']);
    let prefix = record_path
        .split_once("%path")
        .map(|(prefix, _)| prefix)
        .context("MediaMTX recordPath must contain %path")?
        .trim_end_matches('/');
    let recording_root = PathBuf::from(prefix);
    ensure!(
        recording_root.is_absolute(),
        "MediaMTX recording root must be absolute"
    );
    Ok(ParsedMediaConfig { recording_root })
}

fn config_values<'a>(content: &'a str, key: &str) -> Vec<&'a str> {
    let prefix = format!("{key}:");
    content
        .lines()
        .filter_map(|line| line.trim().strip_prefix(&prefix).map(str::trim))
        .collect()
}

fn cross_check_recordings(
    database: &SentinelDatabaseSummary,
    recordings: &RecordingInventory,
) -> anyhow::Result<()> {
    for file in &recordings.files {
        let mut components = Path::new(&file.path).components();
        let path_name = match components.next() {
            Some(Component::Normal(name)) => {
                name.to_str().context("recording path name is not UTF-8")?
            }
            _ => anyhow::bail!("recording path is missing a MediaMTX path component"),
        };
        ensure!(
            components.next().is_some(),
            "recording file is not below its MediaMTX path directory"
        );
        ensure!(
            database.recording_paths.contains(path_name),
            "recording tree contains a path for no Sentinel camera"
        );
    }
    for directory in &recordings.directories {
        let path_name = Path::new(directory)
            .components()
            .next()
            .and_then(|component| match component {
                Component::Normal(name) => name.to_str(),
                _ => None,
            })
            .context("recording directory has no MediaMTX path component")?;
        ensure!(
            database.recording_paths.contains(path_name),
            "recording directory belongs to no Sentinel camera"
        );
    }
    Ok(())
}

fn verify_bundle_inventory(
    root: &SecureDirectory,
    manifest: &SentinelSourceBackupManifest,
) -> anyhow::Result<()> {
    let mut expected_files = BTreeSet::from([
        BUNDLE_MANIFEST_FILE.to_owned(),
        DATABASE_FILE.to_owned(),
        BUNDLE_CONFIG_FILE.to_owned(),
        BUNDLE_CONTRACT_FILE.to_owned(),
    ]);
    expected_files.extend(
        manifest
            .recordings
            .files
            .iter()
            .map(|file| format!("{BUNDLE_RECORDINGS_DIRECTORY}/{}", file.path)),
    );
    let mut expected_directories = BTreeSet::from([BUNDLE_RECORDINGS_DIRECTORY.to_owned()]);
    expected_directories.extend(
        manifest
            .recordings
            .directories
            .iter()
            .map(|directory| format!("{BUNDLE_RECORDINGS_DIRECTORY}/{directory}")),
    );
    let inventory = inventory_directory_tree(root)?;
    ensure!(
        inventory.0 == expected_directories && inventory.1 == expected_files,
        "Sentinel source backup has missing or unexpected entries"
    );
    Ok(())
}

fn inventory_directory_tree(
    root: &SecureDirectory,
) -> anyhow::Result<(BTreeSet<String>, BTreeSet<String>)> {
    let mut directories = BTreeSet::new();
    let mut files = BTreeSet::new();
    inventory_directory_recursive(&root.file, Path::new(""), &mut directories, &mut files)?;
    Ok((directories, files))
}

fn inventory_directory_recursive(
    directory: &File,
    prefix: &Path,
    directories: &mut BTreeSet<String>,
    files: &mut BTreeSet<String>,
) -> anyhow::Result<()> {
    for name in sorted_entry_names(directory)? {
        ensure!(
            directories
                .len()
                .checked_add(files.len())
                .is_some_and(|entries| entries < MAX_RECORDING_ENTRIES + 5),
            "Sentinel backup has too many total entries"
        );
        let metadata = statat(directory, &name, AtFlags::SYMLINK_NOFOLLOW)?;
        let path = prefix.join(&name);
        ensure!(
            path.components().count() <= MAX_RECORDING_PATH_COMPONENTS + 1,
            "Sentinel backup is too deeply nested"
        );
        let portable = portable_path(&path)?;
        match FileType::from_raw_mode(metadata.st_mode) {
            FileType::Directory => {
                ensure!(
                    metadata.st_mode & 0o022 == 0,
                    "backup directory is writable by group or other users"
                );
                directories.insert(portable);
                let child = open_child_directory(directory, &name, "backup directory")?;
                inventory_directory_recursive(&child.file, &path, directories, files)?;
            }
            FileType::RegularFile => {
                ensure!(metadata.st_nlink == 1, "backup file has a hard-link alias");
                files.insert(portable);
            }
            _ => anyhow::bail!("backup contains a symbolic link or special file"),
        }
    }
    Ok(())
}

fn inventory_recordings(root: &SecureDirectory) -> anyhow::Result<RecordingInventory> {
    let mut inventory = RecordingInventory {
        directories: Vec::new(),
        files: Vec::new(),
        bytes: 0,
    };
    inventory_recordings_recursive(&root.file, Path::new(""), &mut inventory)?;
    ensure!(
        inventory.files.len() <= MAX_RECORDING_FILES,
        "recording tree contains too many files"
    );
    Ok(inventory)
}

fn inventory_recordings_recursive(
    directory: &File,
    prefix: &Path,
    output: &mut RecordingInventory,
) -> anyhow::Result<()> {
    for name in sorted_entry_names(directory)? {
        ensure!(
            output
                .files
                .len()
                .checked_add(output.directories.len())
                .is_some_and(|entries| entries < MAX_RECORDING_ENTRIES),
            "recording tree contains too many total entries"
        );
        let metadata = statat(directory, &name, AtFlags::SYMLINK_NOFOLLOW)?;
        let relative = prefix.join(&name);
        ensure!(
            relative.components().count() <= MAX_RECORDING_PATH_COMPONENTS,
            "recording tree is too deeply nested"
        );
        let portable = portable_path(&relative)?;
        match FileType::from_raw_mode(metadata.st_mode) {
            FileType::Directory => {
                ensure!(
                    metadata.st_mode & 0o022 == 0,
                    "recording directory is writable by group or other users"
                );
                ensure!(
                    output.directories.len() < MAX_RECORDING_DIRECTORIES,
                    "recording tree contains too many directories"
                );
                output.directories.push(portable);
                let child = open_child_directory(directory, &name, "recording directory")?;
                inventory_recordings_recursive(&child.file, &relative, output)?;
            }
            FileType::RegularFile => {
                ensure!(
                    metadata.st_nlink == 1,
                    "recording file must have exactly one hard link"
                );
                ensure!(
                    output.files.len() < MAX_RECORDING_FILES,
                    "recording tree contains too many files"
                );
                let path = fd_child_path(directory, &name);
                let (bytes, sha256) = hash_regular_file(&path)?;
                output.bytes = output
                    .bytes
                    .checked_add(bytes)
                    .context("recording byte count overflow")?;
                output.files.push(SentinelStoredFile {
                    path: portable,
                    bytes,
                    sha256,
                });
            }
            _ => anyhow::bail!("recording tree contains a symbolic link or special file"),
        }
    }
    Ok(())
}

fn copy_recording_tree(
    source: &SecureDirectory,
    destination: &SecureDirectory,
) -> anyhow::Result<RecordingInventory> {
    let mut inventory = RecordingInventory {
        directories: Vec::new(),
        files: Vec::new(),
        bytes: 0,
    };
    copy_recordings_recursive(
        &source.file,
        &destination.file,
        Path::new(""),
        &mut inventory,
    )?;
    destination.file.sync_all()?;
    ensure!(
        inventory == inventory_recordings(source)?,
        "recording source changed while it was copied"
    );
    Ok(inventory)
}

fn copy_recordings_recursive(
    source: &File,
    destination: &File,
    prefix: &Path,
    output: &mut RecordingInventory,
) -> anyhow::Result<()> {
    for name in sorted_entry_names(source)? {
        ensure!(
            output
                .files
                .len()
                .checked_add(output.directories.len())
                .is_some_and(|entries| entries < MAX_RECORDING_ENTRIES),
            "recording tree contains too many total entries"
        );
        let metadata = statat(source, &name, AtFlags::SYMLINK_NOFOLLOW)?;
        let relative = prefix.join(&name);
        ensure!(
            relative.components().count() <= MAX_RECORDING_PATH_COMPONENTS,
            "recording tree is too deeply nested"
        );
        let portable = portable_path(&relative)?;
        match FileType::from_raw_mode(metadata.st_mode) {
            FileType::Directory => {
                ensure!(
                    metadata.st_mode & 0o022 == 0,
                    "recording directory is writable by group or other users"
                );
                ensure!(
                    output.directories.len() < MAX_RECORDING_DIRECTORIES,
                    "recording tree contains too many directories"
                );
                mkdirat(destination, &name, Mode::from_raw_mode(0o700))?;
                let source_child = open_child_directory(source, &name, "recording directory")?;
                let destination_child =
                    open_child_directory(destination, &name, "backup recording directory")?;
                output.directories.push(portable);
                copy_recordings_recursive(
                    &source_child.file,
                    &destination_child.file,
                    &relative,
                    output,
                )?;
                destination_child.file.sync_all()?;
            }
            FileType::RegularFile => {
                ensure!(
                    output.files.len() < MAX_RECORDING_FILES,
                    "recording tree contains too many files"
                );
                copy_regular_file_at(source, &name, destination, &name)?;
                let source_hash = hash_regular_file(&fd_child_path(source, &name))?;
                let destination_hash = hash_regular_file(&fd_child_path(destination, &name))?;
                ensure!(
                    source_hash == destination_hash,
                    "recording changed while it was copied"
                );
                output.bytes = output
                    .bytes
                    .checked_add(destination_hash.0)
                    .context("recording byte count overflow")?;
                output.files.push(SentinelStoredFile {
                    path: portable,
                    bytes: destination_hash.0,
                    sha256: destination_hash.1,
                });
            }
            _ => anyhow::bail!("recording tree contains a symbolic link or special file"),
        }
    }
    Ok(())
}

fn sorted_entry_names(directory: &File) -> anyhow::Result<Vec<OsString>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(fd_child_path(directory, OsStr::new(".")))? {
        ensure!(
            names.len() < MAX_ENTRIES_PER_DIRECTORY,
            "directory contains too many entries"
        );
        names.push(entry?.file_name());
    }
    names.sort();
    Ok(names)
}

fn portable_path(path: &Path) -> anyhow::Result<String> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => parts.push(
                value
                    .to_str()
                    .context("recording path is not UTF-8")?
                    .to_owned(),
            ),
            _ => anyhow::bail!("recording path has an unsafe component"),
        }
    }
    ensure!(!parts.is_empty(), "recording path is empty");
    Ok(parts.join("/"))
}

fn open_child_directory(
    parent: &File,
    name: &OsStr,
    label: &str,
) -> anyhow::Result<SecureDirectory> {
    let fd = openat2(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
        secure_resolve_flags(),
    )
    .with_context(|| format!("open {label}"))?;
    let metadata = fstat(&fd)?;
    ensure!(
        FileType::from_raw_mode(metadata.st_mode) == FileType::Directory,
        "{label} is not a directory"
    );
    Ok(SecureDirectory {
        file: File::from(fd),
    })
}

fn fd_child_path(parent: &File, name: &OsStr) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", parent.as_raw_fd())).join(name)
}

struct SecureFile {
    parent: SecureDirectory,
    name: OsString,
}

impl SecureFile {
    fn open(path: &Path, label: &str) -> anyhow::Result<Self> {
        let path = super::super::absolute_path(path)?;
        let parent = SecureDirectory::open(
            path.parent().context("secure file must have a parent")?,
            label,
        )?;
        let name = path
            .file_name()
            .context("secure file must name a file")?
            .to_os_string();
        Self::from_owned_directory(parent, name, label)
    }

    fn from_directory(
        directory: &SecureDirectory,
        name: &OsStr,
        label: &str,
    ) -> anyhow::Result<Self> {
        Self::from_owned_directory(
            SecureDirectory {
                file: directory.file.try_clone()?,
            },
            name.to_os_string(),
            label,
        )
    }

    fn from_owned_directory(
        parent: SecureDirectory,
        name: OsString,
        label: &str,
    ) -> anyhow::Result<Self> {
        let metadata = statat(&parent.file, &name, AtFlags::SYMLINK_NOFOLLOW)
            .with_context(|| format!("inspect {label}"))?;
        ensure!(
            FileType::from_raw_mode(metadata.st_mode) == FileType::RegularFile
                && metadata.st_nlink == 1,
            "{label} must be one regular file"
        );
        Ok(Self { parent, name })
    }

    fn path(&self) -> PathBuf {
        self.parent.child_path(&self.name)
    }

    fn read_bounded(&self, limit: u64) -> anyhow::Result<Vec<u8>> {
        let fd = openat2(
            &self.parent.file,
            &self.name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
            secure_resolve_flags(),
        )?;
        let metadata = fstat(&fd)?;
        ensure!(
            FileType::from_raw_mode(metadata.st_mode) == FileType::RegularFile
                && metadata.st_nlink == 1
                && metadata.st_size >= 0
                && metadata.st_size as u64 <= limit,
            "secure file is not a bounded single-link regular file"
        );
        let mut bytes = Vec::with_capacity(metadata.st_size as usize);
        File::from(fd).take(limit + 1).read_to_end(&mut bytes)?;
        ensure!(bytes.len() as u64 <= limit, "secure file is too large");
        Ok(bytes)
    }
}

struct SentinelServiceLocks {
    maintenance: MaintenanceLock,
    _runtime_locks: Vec<File>,
}

impl SentinelServiceLocks {
    fn acquire(database: &Path, runtime: &Path, product: Product) -> anyhow::Result<Self> {
        // This is the product's established global order: database
        // maintenance, Sentinel runtime, then MediaMTX runtime.
        let maintenance = MaintenanceLock::exclusive(product, database)?;
        let runtime = SecureDirectory::open(runtime, "Sentinel runtime directory")?;
        let mut runtime_locks = Vec::new();
        for name in ["app.lock", "mediamtx.lock"] {
            let fd = openat2(
                &runtime.file,
                name,
                OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::from_raw_mode(0o600),
                secure_resolve_flags(),
            )?;
            let metadata = fstat(&fd)?;
            ensure!(
                FileType::from_raw_mode(metadata.st_mode) == FileType::RegularFile
                    && metadata.st_nlink == 1
                    && metadata.st_mode & 0o077 == 0,
                "Sentinel runtime lock must be one private regular file"
            );
            let named = statat(&runtime.file, name, AtFlags::SYMLINK_NOFOLLOW)?;
            ensure!(
                FileType::from_raw_mode(named.st_mode) == FileType::RegularFile
                    && named.st_nlink == 1
                    && named.st_mode & 0o077 == 0
                    && named.st_dev == metadata.st_dev
                    && named.st_ino == metadata.st_ino,
                "Sentinel runtime lock path changed while it was opened"
            );
            match rustix::fs::flock(&fd, FlockOperation::NonBlockingLockExclusive) {
                Ok(()) => runtime_locks.push(File::from(fd)),
                Err(Errno::WOULDBLOCK) => {
                    anyhow::bail!("Sentinel or MediaMTX is running; stop both before upgrading")
                }
                Err(error) => {
                    return Err(std::io::Error::from(error))
                        .context("acquire Sentinel service lock");
                }
            }
        }
        for name in ["app.pid", "mediamtx.pid"] {
            ensure_stopped_pid(&runtime, name)?;
        }
        Ok(Self {
            maintenance,
            _runtime_locks: runtime_locks,
        })
    }
}

fn ensure_stopped_pid(runtime: &SecureDirectory, name: &str) -> anyhow::Result<()> {
    match statat(&runtime.file, name, AtFlags::SYMLINK_NOFOLLOW) {
        Err(Errno::NOENT) => return Ok(()),
        Err(error) => return Err(std::io::Error::from(error).into()),
        Ok(metadata) => ensure!(
            FileType::from_raw_mode(metadata.st_mode) == FileType::RegularFile
                && metadata.st_nlink == 1,
            "Sentinel PID path must be one regular file"
        ),
    }
    let file = SecureFile::from_directory(runtime, OsStr::new(name), "Sentinel PID file")?;
    let content = String::from_utf8(file.read_bounded(64)?)?;
    let pid = content
        .trim()
        .parse::<u32>()
        .context("Sentinel PID file is invalid")?;
    ensure!(pid > 1, "Sentinel PID file contains an invalid PID");
    ensure!(
        !Path::new("/proc").join(pid.to_string()).exists(),
        "Sentinel or MediaMTX PID is still running"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    const TEST_KEY: [u8; 32] = [7; 32];
    const CAMERA_ID: &str = "22222222-2222-4222-8222-222222222222";
    const CAMERA_PATH: &str = "cam_22222222222242228222222222222222_main";

    struct TestLayout {
        root: tempfile::TempDir,
        database: PathBuf,
        runtime: PathBuf,
        config: PathBuf,
        contract: PathBuf,
        recordings: PathBuf,
    }

    impl TestLayout {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let database = root.path().join("data/sentinel.sqlite3");
            let runtime = root.path().join("runtime");
            let config = root.path().join("config/mediamtx.yml");
            let contract = root.path().join("contract/mediamtx.lock");
            let recordings = root.path().join("recordings");
            for directory in [
                database.parent().unwrap(),
                runtime.as_path(),
                config.parent().unwrap(),
                contract.parent().unwrap(),
                recordings.as_path(),
            ] {
                fs::create_dir_all(directory).unwrap();
                fs::set_permissions(directory, fs::Permissions::from_mode(0o700)).unwrap();
            }
            create_fixture_database(&database).unwrap();
            write_config(&config, &recordings);
            fs::write(
                &contract,
                format!(
                    "# exact Sentinel companion\nversion={CONTRACT_VERSION}\nplatform={CONTRACT_PLATFORM}\nsha256={CONTRACT_BINARY_SHA256}\n"
                ),
            )
            .unwrap();
            let recording = recordings.join(CAMERA_PATH).join("2026-01-01");
            fs::create_dir_all(&recording).unwrap();
            fs::set_permissions(
                recordings.join(CAMERA_PATH),
                fs::Permissions::from_mode(0o700),
            )
            .unwrap();
            fs::set_permissions(&recording, fs::Permissions::from_mode(0o700)).unwrap();
            fs::write(recording.join("segment.mp4"), b"sentinel-recording").unwrap();
            Self {
                root,
                database,
                runtime,
                config,
                contract,
                recordings,
            }
        }

        fn options(&self, backup_name: &str) -> SentinelUpgradeOptions {
            SentinelUpgradeOptions {
                product: Product::SentinelMonitor,
                from_version: "0.1.0".to_owned(),
                to_version: "0.2.0".to_owned(),
                database: self.database.clone(),
                backup_output: self.root.path().join(backup_name),
                runtime_directory: self.runtime.clone(),
                mediamtx_config: self.config.clone(),
                mediamtx_contract: self.contract.clone(),
                recordings_directory: self.recordings.clone(),
                credentials_key: TEST_KEY,
            }
        }

        fn recovery_options(
            &self,
            recovery: PathBuf,
            action: RecoveryAction,
        ) -> SentinelRecoveryOptions {
            SentinelRecoveryOptions {
                product: Product::SentinelMonitor,
                from_version: "0.1.0".to_owned(),
                to_version: "0.2.0".to_owned(),
                database: self.database.clone(),
                runtime_directory: self.runtime.clone(),
                recovery_directory: recovery,
                credentials_key: Some(TEST_KEY),
                action,
            }
        }
    }

    fn write_config(path: &Path, recordings: &Path) {
        fs::write(
            path,
            format!(
                "authMethod: http\n\
                 authHTTPAddress: http://127.0.0.1:8080/internal/media/auth\n\
                 apiAddress: 127.0.0.1:9997\n\
                 playbackAddress: 127.0.0.1:9996\n\
                 recordFormat: fmp4\n\
                 recordPath: {}/%path/%Y-%m-%d_%H-%M-%S-%f\n",
                recordings.display()
            ),
        )
        .unwrap();
    }

    fn create_fixture_database(path: &Path) -> anyhow::Result<()> {
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.execute_batch(SQLX_LEDGER_DDL)?;
        apply_migration(&connection, MIGRATIONS[0])?;
        let main = encrypt_for_test(&TEST_KEY, 1, "rtsp://camera/main");
        let sub = encrypt_for_test(&TEST_KEY, 2, "rtsp://camera/sub");
        let password = encrypt_for_test(&TEST_KEY, 3, "camera-password");
        connection.execute(
            "INSERT INTO users (
               id,email,password_hash,role,active,created_at,updated_at
             ) VALUES ('11111111-1111-4111-8111-111111111111','admin@example.invalid','hash','admin',1,?1,?1)",
            ["2026-01-01T00:00:00Z"],
        )?;
        connection.execute(
            "INSERT INTO cameras (
               id,name,main_stream_url_enc,sub_stream_url_enc,username,password_enc,
               created_by,created_at,updated_at
             ) VALUES (?1,'fixture',?2,?3,'camera-user',?4,
                       '11111111-1111-4111-8111-111111111111',?5,?5)",
            rusqlite::params![CAMERA_ID, main, sub, password, "2026-01-01T00:00:00Z"],
        )?;
        connection.execute(
            "INSERT INTO events (
               id,camera_id,kind,severity,message,details,created_at
             ) VALUES ('33333333-3333-4333-8333-333333333333',?1,'fixture','info','fixture','{}',?2)",
            (CAMERA_ID, "2026-01-01T00:00:01Z"),
        )?;
        connection.execute(
            "INSERT INTO audit_logs (
               id,user_id,action,entity_type,entity_id,details,created_at
             ) VALUES ('44444444-4444-4444-8444-444444444444','11111111-1111-4111-8111-111111111111','fixture','camera',?1,'{}',?2)",
            (CAMERA_ID, "2026-01-01T00:00:02Z"),
        )?;
        apply_migration(&connection, MIGRATIONS[1])?;
        connection.execute(
            "INSERT INTO browser_sessions (
               id,user_id,token_digest,csrf_digest,session_version,created_at,last_seen_at,idle_expires_at,absolute_expires_at
             ) VALUES ('55555555-5555-4555-8555-555555555555','11111111-1111-4111-8111-111111111111',zeroblob(32),randomblob(32),1,?1,?1,?2,?3)",
            (
                "2026-01-01T00:00:00Z",
                "2026-01-02T00:00:00Z",
                "2026-01-03T00:00:00Z",
            ),
        )?;
        apply_migration(&connection, MIGRATIONS[2])?;
        connection.execute(
            "UPDATE media_reconciler_leases
                SET lease_owner='legacy-reconciler',
                    lease_expires_at='2030-01-01T00:01:00Z',
                    updated_at='2030-01-01T00:00:00Z'
              WHERE scope='global'",
            [],
        )?;
        let operation_id: String = connection.query_row(
            "SELECT id FROM media_operations WHERE camera_id=?1",
            [CAMERA_ID],
            |row| row.get(0),
        )?;
        connection.execute(
            "INSERT INTO media_actual_paths (
               path_name,camera_id,profile,present,ready,publisher_active,recording_active,
               source_digest,source_on_demand,record_configured,applied_generation,last_operation_id,observed_at
             ) VALUES (?1,?2,'main',1,1,1,1,zeroblob(32),1,1,1,?3,?4)",
            rusqlite::params![CAMERA_PATH, CAMERA_ID, operation_id, "2026-01-01T00:00:03Z"],
        )?;
        let fingerprint = schema_fingerprint_connection(&connection)?;
        ensure!(
            fingerprint == SOURCE_SCHEMA_SHA256,
            "Sentinel fixture fingerprint mismatch: expected {SOURCE_SCHEMA_SHA256}, got {fingerprint}"
        );
        connection.execute_batch("PRAGMA journal_mode=DELETE;")?;
        drop(connection);
        File::open(path)?.sync_all()?;
        Ok(())
    }

    fn apply_migration(
        connection: &Connection,
        (version, description, sql): (i64, &str, &str),
    ) -> anyhow::Result<()> {
        connection.execute_batch(sql)?;
        connection.execute(
            "INSERT INTO _sqlx_migrations
             (version,description,success,checksum,execution_time) VALUES (?1,?2,1,?3,0)",
            (version, description, expected_migration_checksum(sql)),
        )?;
        Ok(())
    }

    fn encrypt_for_test(key: &[u8; 32], nonce_byte: u8, plaintext: &str) -> Vec<u8> {
        let cipher = Aes256Gcm::new_from_slice(key).unwrap();
        let nonce = [nonce_byte; 12];
        let mut encoded = nonce.to_vec();
        encoded.extend(
            cipher
                .encrypt(Nonce::from_slice(&nonce), plaintext.as_bytes())
                .unwrap(),
        );
        encoded
    }

    fn source_bytes(layout: &TestLayout) -> BTreeMap<String, Vec<u8>> {
        let mut values = BTreeMap::new();
        for (name, path) in [
            ("database", layout.database.clone()),
            ("config", layout.config.clone()),
            ("contract", layout.contract.clone()),
            (
                "recording",
                layout
                    .recordings
                    .join(CAMERA_PATH)
                    .join("2026-01-01/segment.mp4"),
            ),
        ] {
            values.insert(name.to_owned(), fs::read(path).unwrap());
        }
        values
    }

    fn recovery_directory(layout: &TestLayout) -> PathBuf {
        fs::read_dir(layout.database.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .contains(".restore-")
            })
            .unwrap()
    }

    #[test]
    fn upgrades_exact_sentinel_composite_generation() {
        let layout = TestLayout::new();
        let before = source_bytes(&layout);
        let options = layout.options("backup");
        let result = upgrade_sentinel(&options).unwrap();
        assert_eq!(result.schema_identity.application_version, "0.2.0");
        assert_eq!(result.schema_identity.schema_sha256, TARGET_SCHEMA_SHA256);
        assert_eq!(source_bytes(&layout).get("config"), before.get("config"));
        assert_eq!(
            source_bytes(&layout).get("contract"),
            before.get("contract")
        );
        assert_eq!(
            source_bytes(&layout).get("recording"),
            before.get("recording")
        );

        let backup = verify_sentinel_source_backup(
            Product::SentinelMonitor,
            "0.1.0",
            "0.2.0",
            &options.backup_output,
            &TEST_KEY,
        )
        .unwrap();
        assert!(!backup.manifest.credentials_key_included);
        assert!(backup.manifest.credentials_key_required_for_upgrade);
        assert_eq!(
            backup.manifest.credentials_key_sha256,
            credentials_key_sha256(&TEST_KEY)
        );
        assert_eq!(
            backup.manifest.target_schema_identity,
            target_schema_identity()
        );
        assert_eq!(backup.manifest.target_source_commit, TARGET_SOURCE_COMMIT);
        assert_eq!(
            backup.manifest.credential_envelope_contract,
            credential_envelope_contract()
        );
        let manifest_bytes = fs::read(options.backup_output.join(BUNDLE_MANIFEST_FILE)).unwrap();
        for secret in [
            b"camera-user".as_slice(),
            b"camera-password".as_slice(),
            b"rtsp://camera/main".as_slice(),
            b"rtsp://camera/sub".as_slice(),
        ] {
            assert!(
                !manifest_bytes
                    .windows(secret.len())
                    .any(|value| value == secret)
            );
        }
        assert!(
            verify_sentinel_source_backup(
                Product::SentinelMonitor,
                "0.1.0",
                "0.2.0",
                &options.backup_output,
                &[8; 32],
            )
            .is_err()
        );
        assert_eq!(backup.manifest.recordings.files.len(), 1);
        assert_eq!(
            fs::read(
                backup
                    .directory
                    .join(BUNDLE_RECORDINGS_DIRECTORY)
                    .join(CAMERA_PATH)
                    .join("2026-01-01/segment.mp4")
            )
            .unwrap(),
            b"sentinel-recording"
        );
        let identity = verify_current_database(&layout.database, Product::SentinelMonitor).unwrap();
        assert_eq!(identity.schema_sha256, TARGET_SCHEMA_SHA256);
        let target_bytes = fs::read(&layout.database).unwrap();
        assert!(
            !target_bytes
                .windows(b"camera-user".len())
                .any(|value| value == b"camera-user")
        );
        let current_credentials = inspect_current_credentials(&layout.database, &TEST_KEY).unwrap();
        assert_eq!(current_credentials.len(), 4);
        let connection = Connection::open(&layout.database).unwrap();
        let transformed: (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>) = connection
            .query_row(
                "SELECT main_stream_url_enc,sub_stream_url_enc,username_enc,password_enc
                   FROM cameras WHERE id=?1",
                [CAMERA_ID],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        let camera_id = Uuid::parse_str(CAMERA_ID).unwrap();
        let transformer = CredentialTransformer::new(&TEST_KEY);
        for (encoded, field, plaintext) in [
            (
                transformed.0,
                CredentialField::MainStreamUrl,
                "rtsp://camera/main",
            ),
            (
                transformed.1,
                CredentialField::SubStreamUrl,
                "rtsp://camera/sub",
            ),
            (transformed.2, CredentialField::Username, "camera-user"),
            (transformed.3, CredentialField::Password, "camera-password"),
        ] {
            assert_eq!(
                transformer
                    .decrypt_current(camera_id, field, &encoded)
                    .unwrap(),
                plaintext
            );
            let envelope: CredentialEnvelope = serde_json::from_slice(&encoded).unwrap();
            assert_eq!(serde_json::to_vec(&envelope).unwrap(), encoded);
        }
        let lease: (i64, Option<String>, Option<String>, String) = connection
            .query_row(
                "SELECT singleton,lease_owner,lease_expires_at,updated_at
                   FROM media_reconciler_leases",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            lease,
            (1, None, None, "1970-01-01T00:00:00+00:00".to_owned())
        );
        drop(connection);
        let current =
            summarize_database(&layout.database, SentinelDatabaseContract::Target).unwrap();
        assert!(current.records.values().all(|count| *count == 1));
        let ledger: i64 = Connection::open(&layout.database)
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE name='_sqlx_migrations'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(ledger, 0);
    }

    #[test]
    fn wrong_key_and_tampered_source_fail_without_modification() {
        let layout = TestLayout::new();
        let before = source_bytes(&layout);
        let mut wrong_key = layout.options("wrong-key-backup");
        wrong_key.credentials_key = [8; 32];
        assert!(upgrade_sentinel(&wrong_key).is_err());
        assert_eq!(source_bytes(&layout), before);
        assert!(!wrong_key.backup_output.exists());
        assert!(!format!("{wrong_key:?}").contains(&STANDARD.encode([8; 32])));

        Connection::open(&layout.database)
            .unwrap()
            .execute(
                "UPDATE _sqlx_migrations SET checksum=x'00' WHERE version=2",
                [],
            )
            .unwrap();
        let tampered = source_bytes(&layout);
        let options = layout.options("tampered-backup");
        assert!(upgrade_sentinel(&options).is_err());
        assert_eq!(source_bytes(&layout), tampered);
        assert!(!options.backup_output.exists());
    }

    #[test]
    fn current_envelopes_are_canonical_and_bound_to_key_camera_and_field() {
        let transformer = CredentialTransformer::new(&TEST_KEY);
        let camera = Uuid::parse_str(CAMERA_ID).unwrap();
        let other_camera = Uuid::parse_str("66666666-6666-4666-8666-666666666666").unwrap();
        let secret = "rtsp://operator:do-not-disclose@camera.invalid/main";
        let encoded = transformer
            .encrypt_current(camera, CredentialField::Username, secret)
            .unwrap();
        let envelope: CredentialEnvelope = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(envelope.product, CREDENTIAL_PRODUCT);
        assert_eq!(envelope.application_version, CREDENTIAL_APPLICATION_VERSION);
        assert_eq!(envelope.envelope_revision, CREDENTIAL_ENVELOPE_REVISION);
        assert_eq!(envelope.key_id, CREDENTIAL_KEY_ID);
        assert_eq!(serde_json::to_vec(&envelope).unwrap(), encoded);
        assert_eq!(
            transformer
                .decrypt_current(camera, CredentialField::Username, &encoded)
                .unwrap(),
            secret
        );

        for result in [
            transformer.decrypt_current(other_camera, CredentialField::Username, &encoded),
            transformer.decrypt_current(camera, CredentialField::MainStreamUrl, &encoded),
            transformer.decrypt_current(camera, CredentialField::SubStreamUrl, &encoded),
            transformer.decrypt_current(camera, CredentialField::Password, &encoded),
            CredentialTransformer::new(&[8; 32]).decrypt_current(
                camera,
                CredentialField::Username,
                &encoded,
            ),
            transformer.decrypt_current(
                camera,
                CredentialField::Username,
                &encrypt_for_test(&TEST_KEY, 9, secret),
            ),
        ] {
            let error = result.unwrap_err();
            assert!(!format!("{error:#}").contains(secret));
        }
    }

    #[test]
    fn current_envelope_matches_the_committed_product_crypto_golden() {
        let transformer = CredentialTransformer::new(&[0x42; 32]);
        let camera = Uuid::parse_str(CAMERA_ID).unwrap();
        let encoded = transformer
            .encrypt_current_with_nonce(
                camera,
                CredentialField::Username,
                "camera-user",
                [0x11; 12],
            )
            .unwrap();
        assert_eq!(
            String::from_utf8(encoded).unwrap(),
            "{\"product\":\"sentinel-monitor\",\"application_version\":\"0.2.0\",\"envelope_revision\":1,\"key_id\":\"sentinel-credentials-0.2.0-key-1\",\"nonce\":\"ERERERERERERERER\",\"ciphertext\":\"x6J1PX8jV3wvKpChAvYCo2olp7c0Ip1yAVIZ\"}"
        );
    }

    #[test]
    fn nullable_username_remains_absent_in_the_unique_target() {
        let layout = TestLayout::new();
        Connection::open(&layout.database)
            .unwrap()
            .execute("UPDATE cameras SET username=NULL WHERE id=?1", [CAMERA_ID])
            .unwrap();
        let options = layout.options("anonymous-backup");
        upgrade_sentinel(&options).unwrap();
        let username: Option<Vec<u8>> = Connection::open(&layout.database)
            .unwrap()
            .query_row(
                "SELECT username_enc FROM cameras WHERE id=?1",
                [CAMERA_ID],
                |row| row.get(0),
            )
            .unwrap();
        assert!(username.is_none());
        assert_eq!(
            inspect_current_credentials(&layout.database, &TEST_KEY)
                .unwrap()
                .len(),
            3
        );
    }

    #[test]
    fn target_verifier_rejects_wrong_key_and_cross_field_envelope_tampering() {
        let layout = TestLayout::new();
        upgrade_sentinel(&layout.options("target-tamper-backup")).unwrap();
        assert!(verify_sentinel_target_database(&layout.database, &[8; 32]).is_err());

        Connection::open(&layout.database)
            .unwrap()
            .execute(
                "UPDATE cameras SET password_enc=username_enc WHERE id=?1",
                [CAMERA_ID],
            )
            .unwrap();
        assert!(verify_sentinel_target_database(&layout.database, &TEST_KEY).is_err());
    }

    #[test]
    fn mismatched_config_and_orphan_recording_fail_closed() {
        let layout = TestLayout::new();
        let other = layout.root.path().join("other-recordings");
        fs::create_dir(&other).unwrap();
        write_config(&layout.config, &other);
        let before = source_bytes(&layout);
        let options = layout.options("config-backup");
        assert!(upgrade_sentinel(&options).is_err());
        assert_eq!(source_bytes(&layout), before);

        write_config(&layout.config, &layout.recordings);
        let orphan = layout.recordings.join("cam_deadbeef_main");
        fs::create_dir(&orphan).unwrap();
        fs::set_permissions(&orphan, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(orphan.join("segment.mp4"), b"orphan").unwrap();
        let options = layout.options("orphan-backup");
        assert!(upgrade_sentinel(&options).is_err());
        assert!(!options.backup_output.exists());
    }

    #[test]
    fn recording_for_an_unconfigured_substream_is_rejected() {
        let layout = TestLayout::new();
        let connection = Connection::open(&layout.database).unwrap();
        connection
            .execute(
                "UPDATE cameras SET sub_stream_url_enc=NULL WHERE id=?1",
                [CAMERA_ID],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE media_desired_states SET sub_path=NULL WHERE camera_id=?1",
                [CAMERA_ID],
            )
            .unwrap();
        drop(connection);
        let sub = layout.recordings.join(format!(
            "cam_{}_sub",
            Uuid::parse_str(CAMERA_ID).unwrap().simple()
        ));
        fs::create_dir(&sub).unwrap();
        fs::set_permissions(&sub, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(sub.join("segment.mp4"), b"orphan-substream").unwrap();
        let options = layout.options("substream-backup");
        assert!(upgrade_sentinel(&options).is_err());
        assert!(!options.backup_output.exists());
    }

    #[test]
    fn excessively_deep_recording_tree_is_rejected_before_backup() {
        let layout = TestLayout::new();
        let database_before = fs::read(&layout.database).unwrap();
        let mut directory = layout.recordings.join(CAMERA_PATH);
        for index in 0..MAX_RECORDING_PATH_COMPONENTS {
            directory = directory.join(format!("level-{index}"));
            fs::create_dir(&directory).unwrap();
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let options = layout.options("deep-backup");
        assert!(upgrade_sentinel(&options).is_err());
        assert!(!options.backup_output.exists());
        assert_eq!(fs::read(&layout.database).unwrap(), database_before);
    }

    #[test]
    fn held_runtime_and_mediamtx_locks_refuse_upgrade() {
        for name in ["app.lock", "mediamtx.lock"] {
            let layout = TestLayout::new();
            let path = layout.runtime.join(name);
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .open(path)
                .unwrap();
            rustix::fs::flock(&file, FlockOperation::NonBlockingLockExclusive).unwrap();
            let options = layout.options("locked-backup");
            assert!(upgrade_sentinel(&options).is_err());
            assert!(!options.backup_output.exists());
        }
    }

    #[test]
    fn composite_backup_detects_recording_and_manifest_corruption() {
        let layout = TestLayout::new();
        let options = layout.options("backup");
        upgrade_sentinel(&options).unwrap();
        let recording = options
            .backup_output
            .join(BUNDLE_RECORDINGS_DIRECTORY)
            .join(CAMERA_PATH)
            .join("2026-01-01/segment.mp4");
        let original_recording = fs::read(&recording).unwrap();
        OpenOptions::new()
            .append(true)
            .open(&recording)
            .unwrap()
            .write_all(b"corrupt")
            .unwrap();
        assert!(
            verify_sentinel_source_backup(
                Product::SentinelMonitor,
                "0.1.0",
                "0.2.0",
                &options.backup_output,
                &TEST_KEY,
            )
            .is_err()
        );

        fs::write(&recording, original_recording).unwrap();
        let manifest_path = options.backup_output.join(BUNDLE_MANIFEST_FILE);
        let original_manifest = fs::read(&manifest_path).unwrap();
        let mut manifest: serde_json::Value = serde_json::from_slice(&original_manifest).unwrap();
        manifest["credentials_key_sha256"] = serde_json::json!("0".repeat(64));
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        assert!(
            verify_sentinel_source_backup(
                Product::SentinelMonitor,
                "0.1.0",
                "0.2.0",
                &options.backup_output,
                &TEST_KEY,
            )
            .is_err()
        );

        let mut manifest: serde_json::Value = serde_json::from_slice(&original_manifest).unwrap();
        manifest["database_records"]["audit_logs"] = serde_json::json!(999);
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        assert!(
            verify_sentinel_source_backup(
                Product::SentinelMonitor,
                "0.1.0",
                "0.2.0",
                &options.backup_output,
                &TEST_KEY,
            )
            .is_err()
        );

        let mut manifest: serde_json::Value = serde_json::from_slice(&original_manifest).unwrap();
        manifest["target_source_commit"] = serde_json::json!("unofficial-target");
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        assert!(
            verify_sentinel_source_backup(
                Product::SentinelMonitor,
                "0.1.0",
                "0.2.0",
                &options.backup_output,
                &TEST_KEY,
            )
            .is_err()
        );

        let mut manifest: serde_json::Value = serde_json::from_slice(&original_manifest).unwrap();
        manifest["credential_envelope_contract"]["key-id"] =
            serde_json::json!("sentinel-credentials-unofficial");
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        assert!(
            verify_sentinel_source_backup(
                Product::SentinelMonitor,
                "0.1.0",
                "0.2.0",
                &options.backup_output,
                &TEST_KEY,
            )
            .is_err()
        );
    }

    #[test]
    fn exact_selection_rejects_wrong_product_version_and_generic_path() {
        let layout = TestLayout::new();
        let before = source_bytes(&layout);
        for (product, from_version, to_version) in [
            (Product::HostMonitoring, "0.1.0", "0.2.0"),
            (Product::SentinelMonitor, "0.0.9", "0.2.0"),
            (Product::SentinelMonitor, "0.1.0", "0.2.1"),
        ] {
            let mut options = layout.options("wrong-selection-backup");
            options.product = product;
            options.from_version = from_version.to_owned();
            options.to_version = to_version.to_owned();
            assert!(upgrade_sentinel(&options).is_err());
            assert!(!options.backup_output.exists());
        }
        assert!(
            super::super::upgrade_sqlite(
                Product::SentinelMonitor,
                "0.1.0",
                "0.2.0",
                &layout.database,
                &layout.root.path().join("generic-backup")
            )
            .is_err()
        );
        assert_eq!(source_bytes(&layout), before);
    }

    #[test]
    fn interrupted_composite_upgrade_commits_or_rolls_back_under_all_locks() {
        for interruption in [
            RestorePoint::OriginalsPreserved,
            RestorePoint::Installed,
            RestorePoint::Verified,
        ] {
            for action in [RecoveryAction::Rollback, RecoveryAction::Commit] {
                let layout = TestLayout::new();
                let before = source_bytes(&layout);
                let options = layout.options("backup");
                let result = upgrade_sentinel_with_hook(&options, |point| {
                    if std::mem::discriminant(&point) == std::mem::discriminant(&interruption) {
                        anyhow::bail!("injected Sentinel interruption")
                    }
                    Ok(())
                });
                assert!(result.is_err());
                let recovery = recovery_directory(&layout);
                let mut recovery_options = layout.recovery_options(recovery.clone(), action);
                if action == RecoveryAction::Rollback {
                    recovery_options.credentials_key = None;
                }
                recover_sentinel_upgrade(&recovery_options).unwrap();
                assert!(!recovery.exists());
                assert_eq!(source_bytes(&layout).get("config"), before.get("config"));
                assert_eq!(
                    source_bytes(&layout).get("contract"),
                    before.get("contract")
                );
                assert_eq!(
                    source_bytes(&layout).get("recording"),
                    before.get("recording")
                );
                if action == RecoveryAction::Rollback {
                    assert_eq!(fs::read(&layout.database).unwrap(), before["database"]);
                    verify_source_database(&layout.database, ADAPTER).unwrap();
                } else {
                    let identity =
                        verify_sentinel_target_database(&layout.database, &TEST_KEY).unwrap();
                    assert_eq!(identity.application_version, "0.2.0");
                }
            }
        }
    }

    #[test]
    fn interrupted_commit_rejects_wrong_key_before_mutating_recovery_state() {
        let layout = TestLayout::new();
        let options = layout.options("wrong-recovery-key-backup");
        let result = upgrade_sentinel_with_hook(&options, |point| {
            if matches!(point, RestorePoint::Installed) {
                anyhow::bail!("injected Sentinel interruption")
            }
            Ok(())
        });
        assert!(result.is_err());
        let recovery = recovery_directory(&layout);
        let destination_before = fs::read(&layout.database).unwrap();
        let mut wrong = layout.recovery_options(recovery.clone(), RecoveryAction::Commit);
        wrong.credentials_key = Some([8; 32]);
        let error = recover_sentinel_upgrade(&wrong).unwrap_err();
        assert!(recovery.exists());
        assert_eq!(fs::read(&layout.database).unwrap(), destination_before);
        assert!(!format!("{wrong:?}{error:#}").contains(&STANDARD.encode([8; 32])));

        recover_sentinel_upgrade(
            &layout.recovery_options(recovery.clone(), RecoveryAction::Commit),
        )
        .unwrap();
        assert!(!recovery.exists());
        verify_sentinel_target_database(&layout.database, &TEST_KEY).unwrap();
    }

    #[test]
    fn credential_key_file_is_bounded_and_never_returned() {
        let layout = TestLayout::new();
        let key_file = layout.root.path().join("credentials.key");
        fs::write(&key_file, STANDARD.encode(TEST_KEY)).unwrap();
        fs::set_permissions(&key_file, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(sentinel_credentials_key_from_file(&key_file).is_err());
        fs::set_permissions(&key_file, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(
            sentinel_credentials_key_from_file(&key_file).unwrap(),
            TEST_KEY
        );
        fs::write(&key_file, STANDARD.encode([9; 31])).unwrap();
        assert!(sentinel_credentials_key_from_file(&key_file).is_err());
    }
}
