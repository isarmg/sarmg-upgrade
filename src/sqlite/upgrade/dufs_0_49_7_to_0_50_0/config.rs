use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, ensure};
use argon2::{Params, password_hash::PasswordHash};
use rustix::{
    fs::{AtFlags, FileType, Mode, OFlags, fgetxattr, fstat, openat2, statat},
    io::Errno,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::tree::RootIdentity;
use super::{DufsConfigMetadata, MAX_CONFIG_BYTES};
use crate::sqlite::{SecureDirectory, absolute_path, secure_resolve_flags};

const MAX_ACCOUNTS: usize = 1024;
const MAX_USERNAME_BYTES: usize = 128;
const ARGON2_VERSION: u32 = 19;
const ARGON2_MEMORY_KIB: u32 = 19 * 1024;
const ARGON2_ITERATIONS: u32 = 2;
const ARGON2_PARALLELISM: u32 = 1;
const ARGON2_OUTPUT_BYTES: usize = 32;
const ARGON2_SALT_BYTES: usize = 16;
const CONFIG_POSIX_ACL_XATTR: &str = "system.posix_acl_access";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ConfigSnapshot {
    device: u64,
    inode: u64,
    mode: u32,
    links: u64,
    uid: u32,
    gid: u32,
    size: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
    changed_seconds: i64,
    changed_nanoseconds: i64,
}

impl ConfigSnapshot {
    fn from_stat(stat: &rustix::fs::Stat) -> anyhow::Result<Self> {
        ensure!(stat.st_size >= 0, "Dufs config has a negative size");
        Ok(Self {
            device: stat.st_dev,
            inode: stat.st_ino,
            mode: stat.st_mode,
            links: stat.st_nlink,
            uid: stat.st_uid,
            gid: stat.st_gid,
            size: stat.st_size as u64,
            modified_seconds: stat.st_mtime,
            modified_nanoseconds: i64::try_from(stat.st_mtime_nsec)?,
            changed_seconds: stat.st_ctime,
            changed_nanoseconds: i64::try_from(stat.st_ctime_nsec)?,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct RawDufsConfig {
    serve_path: Option<PathBuf>,
    state_dir: Option<PathBuf>,
    auth: Option<Vec<String>>,
    #[serde(rename = "bind")]
    _bind: Option<serde_yaml::Value>,
    #[serde(rename = "trusted-proxies")]
    _trusted_proxies: Option<serde_yaml::Value>,
    #[serde(rename = "port")]
    _port: Option<serde_yaml::Value>,
    #[serde(rename = "log-format")]
    _log_format: Option<serde_yaml::Value>,
    #[serde(rename = "log-file")]
    _log_file: Option<serde_yaml::Value>,
    #[serde(rename = "max-upload-size")]
    _max_upload_size: Option<serde_yaml::Value>,
    #[serde(rename = "upload-idle-timeout")]
    _upload_idle_timeout: Option<serde_yaml::Value>,
    #[serde(rename = "upload-total-timeout")]
    _upload_total_timeout: Option<serde_yaml::Value>,
    #[serde(rename = "max-concurrent-uploads")]
    _max_concurrent_uploads: Option<serde_yaml::Value>,
    #[serde(rename = "min-free-space")]
    _min_free_space: Option<serde_yaml::Value>,
    #[serde(rename = "max-connections")]
    _max_connections: Option<serde_yaml::Value>,
    #[serde(rename = "max-search-entries")]
    _max_search_entries: Option<serde_yaml::Value>,
    #[serde(rename = "max-concurrent-searches")]
    _max_concurrent_searches: Option<serde_yaml::Value>,
    #[serde(rename = "request-timeout")]
    _request_timeout: Option<serde_yaml::Value>,
}

pub(super) struct ParsedDufsConfig {
    pub shared_root: PathBuf,
    pub state_dir: PathBuf,
    pub usernames: Vec<String>,
}

pub(super) struct ConfigAnchor {
    parent: SecureDirectory,
    name: OsString,
    file: File,
    snapshot: ConfigSnapshot,
    bytes: Vec<u8>,
    sha256: String,
    configured_path: PathBuf,
    pub parsed: ParsedDufsConfig,
}

impl ConfigAnchor {
    pub(super) fn open(path: &Path, service_uid: u32, service_gid: u32) -> anyhow::Result<Self> {
        let configured_path = absolute_path(path)?;
        let parent = SecureDirectory::open(
            configured_path
                .parent()
                .context("Dufs config must have a parent directory")?,
            "Dufs config parent",
        )?;
        let name = configured_path
            .file_name()
            .context("Dufs config must name a file")?
            .to_os_string();
        let fd = openat2(
            &parent.file,
            &name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
            secure_resolve_flags(),
        )
        .context("open protected Dufs config")?;
        let mut file = File::from(fd);
        let before = ConfigSnapshot::from_stat(&fstat(&file)?)?;
        validate_security(&file, before, service_uid, service_gid)?;
        ensure!(before.size <= MAX_CONFIG_BYTES, "Dufs config is too large");

        let mut bytes = Vec::with_capacity(before.size as usize);
        Read::by_ref(&mut file)
            .take(MAX_CONFIG_BYTES + 1)
            .read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= MAX_CONFIG_BYTES,
            "Dufs config is too large"
        );
        let after = ConfigSnapshot::from_stat(&fstat(&file)?)?;
        ensure!(
            before == after && bytes.len() as u64 == before.size,
            "Dufs config changed while it was read"
        );
        validate_named_identity(&parent, &name, after)?;

        let parsed = parse_bytes(&bytes)?;
        let sha256 = lower_hex(&Sha256::digest(&bytes));

        Ok(Self {
            parent,
            name,
            file,
            snapshot: after,
            bytes,
            sha256,
            configured_path,
            parsed,
        })
    }

    pub(super) fn ensure_unchanged(&mut self) -> anyhow::Result<()> {
        self.file.seek(SeekFrom::Start(0))?;
        let before = ConfigSnapshot::from_stat(&fstat(&self.file)?)?;
        ensure!(
            before == self.snapshot,
            "Dufs config metadata changed during upgrade"
        );
        let mut bytes = Vec::with_capacity(self.snapshot.size as usize);
        Read::by_ref(&mut self.file)
            .take(MAX_CONFIG_BYTES + 1)
            .read_to_end(&mut bytes)?;
        let after = ConfigSnapshot::from_stat(&fstat(&self.file)?)?;
        ensure!(
            before == after && bytes == self.bytes,
            "Dufs config changed during upgrade"
        );
        validate_named_identity(&self.parent, &self.name, after)
    }

    pub(super) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(super) fn sha256(&self) -> &str {
        &self.sha256
    }

    pub(super) fn configured_path(&self) -> &Path {
        &self.configured_path
    }

    pub(super) fn identity(&self) -> RootIdentity {
        RootIdentity {
            device: self.snapshot.device,
            inode: self.snapshot.inode,
        }
    }

    pub(super) fn metadata(&self) -> DufsConfigMetadata {
        DufsConfigMetadata {
            bytes: self.snapshot.size,
            sha256: self.sha256.clone(),
            uid: self.snapshot.uid,
            gid: self.snapshot.gid,
            mode: self.snapshot.mode & 0o7777,
            sensitive: true,
        }
    }
}

pub(super) fn parse_bytes(bytes: &[u8]) -> anyhow::Result<ParsedDufsConfig> {
    ensure!(
        bytes.len() as u64 <= MAX_CONFIG_BYTES,
        "Dufs config is too large"
    );
    let text = std::str::from_utf8(bytes).context("Dufs config is not UTF-8")?;
    let raw: RawDufsConfig = serde_yaml::from_str(text)
        .context("Dufs config does not match the exact protected YAML contract")?;
    let shared_root = require_absolute_normal_path(
        raw.serve_path
            .context("Dufs config must explicitly set serve-path")?,
        "serve-path",
    )?;
    let state_dir = require_absolute_normal_path(
        raw.state_dir
            .context("Dufs config must explicitly set state-dir")?,
        "state-dir",
    )?;
    let accounts = raw
        .auth
        .context("Dufs config must explicitly contain auth accounts")?;
    let usernames = parse_auth_accounts(&accounts)?;
    Ok(ParsedDufsConfig {
        shared_root,
        state_dir,
        usernames,
    })
}

fn validate_security(
    file: &File,
    snapshot: ConfigSnapshot,
    service_uid: u32,
    service_gid: u32,
) -> anyhow::Result<()> {
    ensure!(
        FileType::from_raw_mode(snapshot.mode) == FileType::RegularFile && snapshot.links == 1,
        "Dufs config must be one regular file"
    );
    ensure!(
        snapshot.uid == 0 || snapshot.uid == service_uid,
        "Dufs config owner is neither root nor the explicit service uid"
    );
    let permissions = snapshot.mode & 0o7777;
    ensure!(
        matches!(permissions, 0o400 | 0o440 | 0o600 | 0o640),
        "Dufs config permissions must be 0400, 0440, 0600, or 0640"
    );
    ensure!(
        permissions & 0o040 == 0 || snapshot.gid == service_gid,
        "group-readable Dufs config does not belong to the explicit service gid"
    );
    let mut empty = [0_u8; 0];
    match fgetxattr(file, CONFIG_POSIX_ACL_XATTR, &mut empty) {
        Ok(_) => anyhow::bail!("Dufs config must not have an extended POSIX access ACL"),
        Err(Errno::NODATA | Errno::NOTSUP) => {}
        Err(error) => return Err(std::io::Error::from(error)).context("inspect Dufs config ACL"),
    }
    Ok(())
}

fn validate_named_identity(
    parent: &SecureDirectory,
    name: &OsStr,
    opened: ConfigSnapshot,
) -> anyhow::Result<()> {
    let named = ConfigSnapshot::from_stat(&statat(&parent.file, name, AtFlags::SYMLINK_NOFOLLOW)?)?;
    ensure!(
        named == opened,
        "Dufs config path changed while it was anchored"
    );
    Ok(())
}

fn parse_auth_accounts(accounts: &[String]) -> anyhow::Result<Vec<String>> {
    ensure!(
        !accounts.is_empty() && accounts.len() <= MAX_ACCOUNTS,
        "Dufs auth account count is outside the exact 1..={MAX_ACCOUNTS} contract"
    );
    let mut usernames = Vec::with_capacity(accounts.len());
    for (index, account) in accounts.iter().enumerate() {
        let (username, password_hash) = account
            .split_once(':')
            .with_context(|| format!("Dufs auth account #{} is malformed", index + 1))?;
        ensure!(
            !username.is_empty() && username.len() <= MAX_USERNAME_BYTES,
            "Dufs auth account #{} has an invalid username length",
            index + 1
        );
        ensure!(
            !usernames.iter().any(|existing| existing == username),
            "Dufs auth account #{} duplicates a username",
            index + 1
        );
        validate_argon2id_hash(password_hash).with_context(|| {
            format!(
                "Dufs auth account #{} has an invalid password contract",
                index + 1
            )
        })?;
        usernames.push(username.to_owned());
    }
    Ok(usernames)
}

fn validate_argon2id_hash(value: &str) -> anyhow::Result<()> {
    let parsed = PasswordHash::new(value).map_err(|_| anyhow::anyhow!("invalid PHC string"))?;
    ensure!(
        parsed.algorithm.as_str() == "argon2id",
        "password hash algorithm is not argon2id"
    );
    ensure!(
        parsed.version == Some(ARGON2_VERSION),
        "password hash version is not 19"
    );
    let salt = parsed.salt.context("password hash has no salt")?;
    let output = parsed.hash.context("password hash has no output")?;
    let mut decoded_salt = [0_u8; 64];
    let decoded_salt = salt
        .decode_b64(&mut decoded_salt)
        .map_err(|_| anyhow::anyhow!("invalid password salt"))?;
    ensure!(
        decoded_salt.len() == ARGON2_SALT_BYTES,
        "password salt length is not exact"
    );
    ensure!(
        output.len() == ARGON2_OUTPUT_BYTES,
        "password output length is not exact"
    );
    ensure!(
        parsed.params.iter().count() == 3
            && parsed.params.get_decimal("m") == Some(ARGON2_MEMORY_KIB)
            && parsed.params.get_decimal("t") == Some(ARGON2_ITERATIONS)
            && parsed.params.get_decimal("p") == Some(ARGON2_PARALLELISM),
        "password cost parameters are not exact"
    );
    let params =
        Params::try_from(&parsed).map_err(|_| anyhow::anyhow!("invalid password parameters"))?;
    ensure!(
        params.m_cost() == ARGON2_MEMORY_KIB
            && params.t_cost() == ARGON2_ITERATIONS
            && params.p_cost() == ARGON2_PARALLELISM
            && params.output_len() == Some(ARGON2_OUTPUT_BYTES)
            && params.keyid().is_empty()
            && params.data().is_empty(),
        "password parameters do not match the exact Dufs policy"
    );
    Ok(())
}

fn require_absolute_normal_path(path: PathBuf, field: &str) -> anyhow::Result<PathBuf> {
    ensure!(path.is_absolute(), "Dufs config {field} must be absolute");
    ensure!(
        path.components()
            .all(|component| matches!(component, Component::RootDir | Component::Normal(_))),
        "Dufs config {field} must contain only normal absolute components"
    );
    Ok(path)
}

fn lower_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}
