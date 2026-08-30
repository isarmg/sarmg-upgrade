use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs::{self, File, OpenOptions},
    io::Read,
    os::{
        fd::AsRawFd,
        unix::{
            ffi::OsStrExt,
            fs::{FileExt, MetadataExt, OpenOptionsExt, PermissionsExt},
        },
    },
    path::{Path, PathBuf},
};

use anyhow::{Context, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD_NO_PAD};
use rustix::{
    fs::{
        AtFlags, CWD, FileType, FlockOperation, Mode, OFlags, RenameFlags, SeekFrom, Timestamps,
        XattrFlags, chownat, fchown, flock, fstat, futimens, lgetxattr, llistxattr, lsetxattr,
        open, openat2, renameat_with, seek, statat,
    },
    io::Errno,
    process::{Gid, Uid},
    time::Timespec,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{
    DufsTreeBudget, NEW_STAGE_DIRECTORY, OLD_STAGE_DIRECTORY, READINESS_PREFIX, UPLOAD_PREFIX,
    UPLOAD_SUFFIX, decode_path, encode_path,
};
use crate::sqlite::{SecureDirectory, absolute_path, secure_resolve_flags};

const MAX_TREE_DEPTH: usize = 2048;
const MAX_XATTR_LIST_BYTES: usize = 64 * 1024;
const MAX_XATTR_COUNT: usize = 1024;
const MAX_XATTR_VALUE_BYTES: usize = 64 * 1024;
const MAX_XATTR_BYTES_PER_ENTRY: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct RootIdentity {
    pub device: u64,
    pub inode: u64,
}

impl RootIdentity {
    pub(super) fn sha256(self) -> String {
        let mut digest = Sha256::new();
        digest.update(b"dufs-root-binding-v1\0");
        digest.update(self.device.to_be_bytes());
        digest.update(self.inode.to_be_bytes());
        super::lower_hex(&digest.finalize())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TreeInventory {
    pub entries: u64,
    pub directories: u64,
    pub regular_files: u64,
    pub symbolic_links: u64,
    pub logical_bytes: u64,
    pub allocated_bytes: u64,
    pub sha256: String,
    pub records: Vec<TreeEntry>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TreeEntry {
    pub path_base64: String,
    pub kind: TreeEntryKind,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub logical_bytes: u64,
    pub allocated_bytes: u64,
    pub content_sha256: String,
    pub symlink_target_base64: Option<String>,
    pub hardlink_to_base64: Option<String>,
    pub xattrs: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TreeEntryKind {
    Directory,
    RegularFile,
    SymbolicLink,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StageFileExpectation {
    pub relative_path_base64: String,
    pub device: Option<u64>,
    pub inode: Option<u64>,
}

impl StageFileExpectation {
    pub(super) fn new(path: Vec<u8>, identity: Option<(u64, u64)>) -> Self {
        Self {
            relative_path_base64: encode_path(&path),
            device: identity.map(|value| value.0),
            inode: identity.map(|value| value.1),
        }
    }

    fn path(&self) -> anyhow::Result<Vec<u8>> {
        decode_path(&self.relative_path_base64)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StageMove {
    pub old_relative_base64: String,
    pub new_relative_base64: String,
    pub old_device: Option<u64>,
    pub old_inode: Option<u64>,
    pub files: Vec<StageFileExpectation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PurgeExpectation {
    pub target: Vec<u8>,
    pub trash: Vec<u8>,
    pub source_device: u64,
    pub source_inode: u64,
    pub is_directory: bool,
    pub state: i64,
}

impl StageMove {
    pub(super) fn new(old: Vec<u8>, new: Vec<u8>) -> Self {
        Self {
            old_relative_base64: encode_path(&old),
            new_relative_base64: encode_path(&new),
            old_device: None,
            old_inode: None,
            files: Vec::new(),
        }
    }

    pub(super) fn add_file(&mut self, file: StageFileExpectation) {
        self.files.push(file);
        self.files
            .sort_by(|left, right| left.relative_path_base64.cmp(&right.relative_path_base64));
    }

    pub(super) fn old(&self) -> anyhow::Result<Vec<u8>> {
        decode_path(&self.old_relative_base64)
    }

    pub(super) fn new_path(&self) -> anyhow::Result<Vec<u8>> {
        decode_path(&self.new_relative_base64)
    }

    fn parent(&self) -> anyhow::Result<Vec<u8>> {
        let old = self.old()?;
        Ok(old
            .iter()
            .rposition(|byte| *byte == b'/')
            .map_or_else(Vec::new, |index| old[..index].to_vec()))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RootSnapshot {
    device: u64,
    inode: u64,
    mode: u32,
    uid: u32,
    gid: u32,
}

pub(super) struct RootAnchor {
    configured_path: PathBuf,
    directory: SecureDirectory,
    snapshot: RootSnapshot,
    _locked: bool,
}

impl RootAnchor {
    pub(super) fn lock(path: &Path, service_uid: u32, service_gid: u32) -> anyhow::Result<Self> {
        let anchor = Self::open(path, service_uid, service_gid, true)?;
        match flock(
            &anchor.directory.file,
            FlockOperation::NonBlockingLockExclusive,
        ) {
            Ok(()) => Ok(Self {
                _locked: true,
                ..anchor
            }),
            Err(Errno::WOULDBLOCK) => anyhow::bail!("Dufs service is active on the shared root"),
            Err(error) => {
                Err(std::io::Error::from(error)).context("acquire exclusive Dufs shared-root lock")
            }
        }
    }

    pub(super) fn open_unlocked(
        path: &Path,
        service_uid: u32,
        service_gid: u32,
    ) -> anyhow::Result<Self> {
        Self::open(path, service_uid, service_gid, false)
    }

    fn open(path: &Path, service_uid: u32, service_gid: u32, locked: bool) -> anyhow::Result<Self> {
        let configured_path = absolute_path(path)?;
        let directory = SecureDirectory::open(&configured_path, "Dufs shared root")?;
        let stat = fstat(&directory.file)?;
        ensure!(
            stat.st_uid == service_uid && stat.st_gid == service_gid,
            "Dufs shared root must belong to the explicit service uid/gid"
        );
        Ok(Self {
            configured_path,
            directory,
            snapshot: RootSnapshot {
                device: stat.st_dev,
                inode: stat.st_ino,
                mode: stat.st_mode,
                uid: stat.st_uid,
                gid: stat.st_gid,
            },
            _locked: locked,
        })
    }

    pub(super) fn identity(&self) -> RootIdentity {
        RootIdentity {
            device: self.snapshot.device,
            inode: self.snapshot.inode,
        }
    }

    pub(super) fn owner(&self) -> (u32, u32) {
        (self.snapshot.uid, self.snapshot.gid)
    }

    pub(super) fn ensure_unchanged(&self) -> anyhow::Result<()> {
        let stat = fstat(&self.directory.file)?;
        ensure!(
            RootSnapshot {
                device: stat.st_dev,
                inode: stat.st_ino,
                mode: stat.st_mode,
                uid: stat.st_uid,
                gid: stat.st_gid,
            } == self.snapshot,
            "Dufs shared root identity or ownership changed"
        );
        let reopened = SecureDirectory::open(&self.configured_path, "Dufs shared root")?;
        let named = fstat(&reopened.file)?;
        ensure!(
            named.st_dev == self.snapshot.device && named.st_ino == self.snapshot.inode,
            "Dufs shared-root path changed while anchored"
        );
        Ok(())
    }

    pub(super) fn validate_stage_plan(
        &self,
        moves: &mut [StageMove],
        service_uid: u32,
        budget: DufsTreeBudget,
        protected: &[(&str, RootIdentity)],
    ) -> anyhow::Result<()> {
        for (label, identity) in protected {
            ensure!(
                self.identity() != *identity,
                "Dufs shared root aliases the protected {label} object"
            );
        }
        let discovered = scan_namespaces(&self.path(), budget, protected)?;
        let planned = moves
            .iter()
            .map(StageMove::old)
            .collect::<Result<Vec<_>, _>>()?;
        ensure!(
            discovered.old_stage_directories == planned,
            "Dufs old private stage-directory set does not exactly match active database state (tree={}, database={})",
            discovered.old_stage_directories.len(),
            planned.len()
        );
        for movement in moves {
            let old = movement.old()?;
            let new = movement.new_path()?;
            let old_stat = self.stat_relative(&old)?;
            ensure!(
                FileType::from_raw_mode(old_stat.st_mode) == FileType::Directory
                    && old_stat.st_uid == service_uid
                    && old_stat.st_mode & 0o7777 == 0o700,
                "Dufs old stage directory must be a service-owned 0700 real directory"
            );
            let parent = movement.parent()?;
            let parent_stat = if parent.is_empty() {
                fstat(&self.directory.file)?
            } else {
                self.stat_relative(&parent)?
            };
            ensure!(
                old_stat.st_dev == parent_stat.st_dev,
                "Dufs old stage directory crosses its parent filesystem"
            );
            ensure!(
                self.try_stat_relative(&new)?.is_none(),
                "Dufs new stage namespace already exists"
            );
            movement.old_device = Some(old_stat.st_dev);
            movement.old_inode = Some(old_stat.st_ino);
            for expected in &movement.files {
                let path = expected.path()?;
                let stat = self.stat_relative(&path)?;
                ensure!(
                    FileType::from_raw_mode(stat.st_mode) == FileType::RegularFile
                        && stat.st_nlink == 1
                        && stat.st_uid == service_uid
                        && stat.st_mode & 0o7777 == 0o600,
                    "Dufs active stage must be one service-owned 0600 regular file"
                );
                if let (Some(device), Some(inode)) = (expected.device, expected.inode) {
                    ensure!(
                        stat.st_dev == device && stat.st_ino == inode,
                        "Dufs active stage identity does not match SQLite"
                    );
                }
            }
        }
        self.ensure_unchanged()
    }

    pub(super) fn validate_purges(&self, purges: &[PurgeExpectation]) -> anyhow::Result<()> {
        for purge in purges {
            let (present, absent) = if purge.state == 0 {
                (&purge.target, &purge.trash)
            } else {
                (&purge.trash, &purge.target)
            };
            let stat = self.stat_relative(present)?;
            let file_type = FileType::from_raw_mode(stat.st_mode);
            ensure!(
                stat.st_dev == purge.source_device
                    && stat.st_ino == purge.source_inode
                    && if purge.is_directory {
                        file_type == FileType::Directory
                    } else {
                        file_type == FileType::RegularFile
                    },
                "Dufs purge external resource does not match its persisted source identity"
            );
            if purge.state == 0 {
                ensure!(
                    self.try_stat_relative(absent)?.is_none(),
                    "prepared Dufs purge already has an ambiguous trash occupant"
                );
            }
        }
        self.ensure_unchanged()
    }

    pub(super) fn ensure_no_protected_aliases(
        &self,
        budget: DufsTreeBudget,
        protected: &[(&str, RootIdentity)],
    ) -> anyhow::Result<()> {
        budget.validate()?;
        for (label, identity) in protected {
            ensure!(
                self.identity() != *identity,
                "Dufs shared root aliases the protected {label} object"
            );
        }
        let root = self.path();
        let mut stack = vec![(PathBuf::new(), 0_usize)];
        let mut total_entries = 1_u64;
        while let Some((relative, depth)) = stack.pop() {
            ensure!(
                depth <= MAX_TREE_DEPTH,
                "Dufs alias scan exceeds the maximum depth"
            );
            for entry in read_directory_bounded(&root.join(&relative), budget)? {
                total_entries = total_entries
                    .checked_add(1)
                    .context("Dufs alias scan entry count overflow")?;
                ensure!(
                    total_entries <= budget.max_entries,
                    "Dufs alias scan entry budget exceeded"
                );
                let child = relative.join(entry.file_name());
                let metadata = fs::symlink_metadata(entry.path())?;
                if let Some((label, _)) = protected.iter().find(|(_, identity)| {
                    metadata.dev() == identity.device && metadata.ino() == identity.inode
                }) {
                    anyhow::bail!("Dufs shared tree aliases the protected {label} object");
                }
                if metadata.is_dir() && !metadata.file_type().is_symlink() {
                    stack.push((child, depth + 1));
                }
            }
        }
        self.ensure_unchanged()
    }

    pub(super) fn backup_tree(
        &self,
        destination: &Path,
        budget: DufsTreeBudget,
    ) -> anyhow::Result<TreeInventory> {
        ensure!(
            !destination.exists(),
            "Dufs tree backup destination already exists"
        );
        fs::create_dir(destination)?;
        let before = inventory_path(&self.path(), budget)?;
        copy_tree(&self.path(), destination, budget)?;
        sync_tree(destination)?;
        let copied = inventory_path(destination, budget)?;
        ensure!(
            before == copied,
            "Dufs shared-tree backup does not reproduce the source inventory"
        );
        let after = inventory_path(&self.path(), budget)?;
        ensure!(
            before == after,
            "Dufs shared tree changed while it was backed up"
        );
        Ok(copied)
    }

    pub(super) fn move_stage(&self, movement: &StageMove, forward: bool) -> anyhow::Result<()> {
        let old = movement.old()?;
        let new = movement.new_path()?;
        let parent = movement.parent()?;
        let (source, destination, expected_dev, expected_ino) = if forward {
            (old, new, movement.old_device, movement.old_inode)
        } else {
            (new, old, movement.old_device, movement.old_inode)
        };
        let parent_fd = self.open_relative_directory(&parent)?;
        let source_name = source
            .rsplit(|byte| *byte == b'/')
            .next()
            .context("Dufs stage move source has no name")?;
        let destination_name = destination
            .rsplit(|byte| *byte == b'/')
            .next()
            .context("Dufs stage move destination has no name")?;
        let source_os = OsStr::from_bytes(source_name);
        let destination_os = OsStr::from_bytes(destination_name);
        let source_stat = statat(&parent_fd, source_os, AtFlags::SYMLINK_NOFOLLOW)?;
        ensure!(
            Some(source_stat.st_dev) == expected_dev && Some(source_stat.st_ino) == expected_ino,
            "Dufs stage-directory identity changed before rename"
        );
        match statat(&parent_fd, destination_os, AtFlags::SYMLINK_NOFOLLOW) {
            Err(Errno::NOENT) => {}
            Ok(_) => anyhow::bail!("Dufs stage rename destination unexpectedly exists"),
            Err(error) => {
                return Err(std::io::Error::from(error))
                    .context("inspect Dufs stage rename destination");
            }
        }
        renameat_with(
            &parent_fd,
            source_os,
            &parent_fd,
            destination_os,
            RenameFlags::NOREPLACE,
        )?;
        parent_fd.sync_all()?;
        Ok(())
    }

    pub(super) fn classify_stage(&self, movement: &StageMove) -> anyhow::Result<StagePosition> {
        let old_path = movement.old()?;
        let new_path = movement.new_path()?;
        let old = self.try_stat_relative(&old_path)?;
        let new = self.try_stat_relative(&new_path)?;
        let matches = |stat: &rustix::fs::Stat| {
            Some(stat.st_dev) == movement.old_device && Some(stat.st_ino) == movement.old_inode
        };
        match (old.as_ref().map(matches), new.as_ref().map(matches)) {
            (Some(true), None) => Ok(StagePosition::Old),
            (None, Some(true)) => Ok(StagePosition::New),
            _ => Ok(StagePosition::Ambiguous),
        }
    }

    pub(super) fn path(&self) -> PathBuf {
        PathBuf::from(format!("/proc/self/fd/{}", self.directory.file.as_raw_fd()))
    }

    pub(super) fn configured_path(&self) -> &Path {
        &self.configured_path
    }

    fn open_relative_directory(&self, relative: &[u8]) -> anyhow::Result<File> {
        let path = if relative.is_empty() {
            OsStr::new(".")
        } else {
            OsStr::from_bytes(relative)
        };
        let fd = openat2(
            &self.directory.file,
            path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
            secure_resolve_flags(),
        )?;
        Ok(File::from(fd))
    }

    fn stat_relative(&self, relative: &[u8]) -> anyhow::Result<rustix::fs::Stat> {
        let path = if relative.is_empty() {
            OsStr::new(".")
        } else {
            OsStr::from_bytes(relative)
        };
        let fd = openat2(
            &self.directory.file,
            path,
            OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
            secure_resolve_flags(),
        )?;
        fstat(&fd).map_err(Into::into)
    }

    fn try_stat_relative(&self, relative: &[u8]) -> anyhow::Result<Option<rustix::fs::Stat>> {
        let path = if relative.is_empty() {
            OsStr::new(".")
        } else {
            OsStr::from_bytes(relative)
        };
        match openat2(
            &self.directory.file,
            path,
            OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
            secure_resolve_flags(),
        ) {
            Ok(fd) => Ok(Some(fstat(&fd)?)),
            Err(Errno::NOENT) => Ok(None),
            Err(error) => {
                Err(std::io::Error::from(error)).context("inspect Dufs root-relative entry")
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StagePosition {
    Old,
    New,
    Ambiguous,
}

#[derive(Default)]
struct NamespaceScan {
    old_stage_directories: Vec<Vec<u8>>,
}

fn read_directory_bounded(
    path: &Path,
    budget: DufsTreeBudget,
) -> anyhow::Result<Vec<fs::DirEntry>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(path)? {
        ensure!(
            (entries.len() as u64) < budget.max_entries_per_directory,
            "Dufs per-directory entry budget exceeded"
        );
        entries.push(entry?);
    }
    entries.sort_by_key(fs::DirEntry::file_name);
    Ok(entries)
}

fn scan_namespaces(
    root: &Path,
    budget: DufsTreeBudget,
    protected: &[(&str, RootIdentity)],
) -> anyhow::Result<NamespaceScan> {
    budget.validate()?;
    let mut scan = NamespaceScan::default();
    let mut stack = vec![(PathBuf::new(), 0_usize, false)];
    let mut total_entries = 1_u64;
    ensure!(
        total_entries <= budget.max_entries,
        "Dufs namespace scan entry budget exceeded"
    );
    while let Some((relative, depth, inside_old_stage)) = stack.pop() {
        ensure!(
            depth <= MAX_TREE_DEPTH,
            "Dufs shared tree exceeds the maximum depth"
        );
        let entries = read_directory_bounded(&root.join(&relative), budget)?;
        for entry in entries {
            total_entries = total_entries
                .checked_add(1)
                .context("Dufs namespace entry count overflow")?;
            ensure!(
                total_entries <= budget.max_entries,
                "Dufs namespace scan entry budget exceeded"
            );
            let name = entry.file_name();
            let name_bytes = name.as_bytes();
            let child = relative.join(&name);
            let child_bytes = child.as_os_str().as_bytes().to_vec();
            let metadata = fs::symlink_metadata(entry.path())?;
            if let Some((label, _)) = protected.iter().find(|(_, identity)| {
                metadata.dev() == identity.device && metadata.ino() == identity.inode
            }) {
                anyhow::bail!("Dufs shared tree aliases the protected {label} object");
            }
            if name_bytes == NEW_STAGE_DIRECTORY {
                anyhow::bail!("Dufs current-only .dufs-upload-stages namespace already exists");
            }
            if is_readiness_name(name_bytes) {
                anyhow::bail!("Dufs current-only readiness namespace collides with old user data");
            }
            if !inside_old_stage && is_legacy_stage_residue(name_bytes) {
                anyhow::bail!(
                    "Dufs legacy root-level upload residue requires v0.49.7 reconciliation"
                );
            }
            if name_bytes == OLD_STAGE_DIRECTORY {
                ensure!(
                    metadata.is_dir() && !metadata.file_type().is_symlink(),
                    "Dufs old private stage namespace is not a real directory"
                );
                scan.old_stage_directories.push(child_bytes.clone());
            }
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                stack.push((
                    child,
                    depth + 1,
                    inside_old_stage || name_bytes == OLD_STAGE_DIRECTORY,
                ));
            }
        }
    }
    scan.old_stage_directories.sort();
    Ok(scan)
}

fn is_readiness_name(name: &[u8]) -> bool {
    let Some(value) = name
        .strip_prefix(READINESS_PREFIX)
        .and_then(|value| value.strip_suffix(b".probe"))
    else {
        return false;
    };
    std::str::from_utf8(value).ok().is_some_and(|value| {
        Uuid::parse_str(value).is_ok_and(|uuid| uuid.hyphenated().to_string() == value)
    })
}

fn is_legacy_stage_residue(name: &[u8]) -> bool {
    let is_stage = name.starts_with(UPLOAD_PREFIX) && name.ends_with(UPLOAD_SUFFIX);
    let is_state = name.starts_with(UPLOAD_PREFIX) && name.ends_with(b".part.state");
    let is_state_temp = name.starts_with(UPLOAD_PREFIX)
        && name
            .windows(b".part.state-".len())
            .any(|window| window == b".part.state-")
        && name.ends_with(b".tmp");
    is_stage || is_state || is_state_temp
}

pub(super) fn validate_state_directory(
    path: &Path,
    uid: u32,
    gid: u32,
) -> anyhow::Result<RootIdentity> {
    let directory = SecureDirectory::open(path, "Dufs state directory")?;
    let stat = fstat(&directory.file)?;
    ensure!(
        stat.st_uid == uid && stat.st_gid == gid && stat.st_mode & 0o7777 == 0o700,
        "Dufs state directory must be service-owned mode 0700"
    );
    Ok(RootIdentity {
        device: stat.st_dev,
        inode: stat.st_ino,
    })
}

pub(super) fn validate_disjoint_paths(
    root: &Path,
    state: &Path,
    config: &Path,
    output: &Path,
) -> anyhow::Result<RootIdentity> {
    let root = absolute_path(root)?;
    let state = absolute_path(state)?;
    let config = absolute_path(config)?;
    let output = absolute_path(output)?;
    let output_parent = output
        .parent()
        .context("Dufs backup output must have a parent")?;
    let output_parent_anchor = SecureDirectory::open(output_parent, "Dufs backup output parent")?;
    let output_parent_stat = fstat(&output_parent_anchor.file)?;
    let output_candidate = output_parent.join(
        output
            .file_name()
            .context("Dufs backup output must name a directory")?,
    );
    validate_core_path_relationships(&root, &state, &config)?;
    for protected in [&root, &state, &config] {
        ensure!(
            !output_candidate.starts_with(protected) && !protected.starts_with(&output_candidate),
            "Dufs backup output overlaps a protected product resource"
        );
    }
    Ok(RootIdentity {
        device: output_parent_stat.st_dev,
        inode: output_parent_stat.st_ino,
    })
}

pub(super) fn validate_core_path_relationships(
    root: &Path,
    state: &Path,
    config: &Path,
) -> anyhow::Result<()> {
    let root = absolute_path(root)?;
    let state = absolute_path(state)?;
    let config = absolute_path(config)?;
    ensure!(
        root != state && !root.starts_with(&state) && !state.starts_with(&root),
        "Dufs shared root and state directory overlap"
    );
    ensure!(
        !config.starts_with(&root)
            && !config.starts_with(&state)
            && !root.starts_with(&config)
            && !state.starts_with(&config),
        "Dufs protected config overlaps the shared root or state directory"
    );
    Ok(())
}

pub(super) fn inventory_path(root: &Path, budget: DufsTreeBudget) -> anyhow::Result<TreeInventory> {
    budget.validate()?;
    let root = root
        .canonicalize()
        .context("canonicalize Dufs inventory root")?;
    let root_metadata = fs::symlink_metadata(&root)?;
    ensure!(
        root_metadata.is_dir() && !root_metadata.file_type().is_symlink(),
        "Dufs inventory root is not a real directory"
    );
    let mut builder = InventoryBuilder::new(budget);
    builder.add(&root, Path::new(""), &root_metadata)?;
    let mut stack = vec![(PathBuf::new(), 0_usize)];
    while let Some((relative, depth)) = stack.pop() {
        ensure!(
            depth <= MAX_TREE_DEPTH,
            "Dufs tree depth exceeds {MAX_TREE_DEPTH}"
        );
        let entries = read_directory_bounded(&root.join(&relative), budget)?;
        for entry in entries.into_iter().rev() {
            let child = relative.join(entry.file_name());
            let metadata = fs::symlink_metadata(root.join(&child))?;
            builder.add(&root, &child, &metadata)?;
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                stack.push((child, depth + 1));
            }
        }
    }
    builder.finish()
}

struct InventoryBuilder {
    budget: DufsTreeBudget,
    entries: Vec<TreeEntry>,
    logical_bytes: u64,
    allocated_bytes: u64,
    directories: u64,
    regular_files: u64,
    symbolic_links: u64,
    hardlinks: BTreeMap<(u64, u64), (String, u64, u64)>,
}

impl InventoryBuilder {
    fn new(budget: DufsTreeBudget) -> Self {
        Self {
            budget,
            entries: Vec::new(),
            logical_bytes: 0,
            allocated_bytes: 0,
            directories: 0,
            regular_files: 0,
            symbolic_links: 0,
            hardlinks: BTreeMap::new(),
        }
    }

    fn add(&mut self, root: &Path, relative: &Path, metadata: &fs::Metadata) -> anyhow::Result<()> {
        ensure!(
            (self.entries.len() as u64) < self.budget.max_entries,
            "Dufs total tree entry budget exceeded"
        );
        let path_base64 = STANDARD_NO_PAD.encode(relative.as_os_str().as_bytes());
        let mode = metadata.mode() & 0o7777;
        let uid = metadata.uid();
        let gid = metadata.gid();
        let full = root.join(relative);
        let xattrs = read_xattrs(&full)?;
        let (
            kind,
            logical_bytes,
            allocated_bytes,
            content_sha256,
            symlink_target_base64,
            hardlink_to_base64,
        ) = if metadata.is_dir() && !metadata.file_type().is_symlink() {
            self.directories += 1;
            (
                TreeEntryKind::Directory,
                0,
                0,
                super::lower_hex(&Sha256::digest([])),
                None,
                None,
            )
        } else if metadata.is_file() && !metadata.file_type().is_symlink() {
            self.regular_files += 1;
            let key = (metadata.dev(), metadata.ino());
            let hardlink = if metadata.nlink() > 1 {
                if let Some((first, seen, expected)) = self.hardlinks.get_mut(&key) {
                    *seen += 1;
                    ensure!(
                        *expected == metadata.nlink(),
                        "Dufs hard-link count changed during inventory"
                    );
                    Some(first.clone())
                } else {
                    self.hardlinks
                        .insert(key, (path_base64.clone(), 1, metadata.nlink()));
                    None
                }
            } else {
                None
            };
            let digest = hash_file(&full)?;
            (
                TreeEntryKind::RegularFile,
                metadata.size(),
                metadata.blocks().saturating_mul(512),
                digest,
                None,
                hardlink,
            )
        } else if metadata.file_type().is_symlink() {
            self.symbolic_links += 1;
            let target = fs::read_link(&full)?;
            let target_bytes = target.as_os_str().as_bytes();
            (
                TreeEntryKind::SymbolicLink,
                target_bytes.len() as u64,
                0,
                super::lower_hex(&Sha256::digest(target_bytes)),
                Some(STANDARD_NO_PAD.encode(target_bytes)),
                None,
            )
        } else {
            anyhow::bail!("Dufs tree contains an unsupported special filesystem entry")
        };
        self.logical_bytes = self
            .logical_bytes
            .checked_add(logical_bytes)
            .context("Dufs logical-byte overflow")?;
        self.allocated_bytes = self
            .allocated_bytes
            .checked_add(allocated_bytes)
            .context("Dufs allocated-byte overflow")?;
        ensure!(
            self.logical_bytes <= self.budget.max_logical_bytes,
            "Dufs logical-byte budget exceeded"
        );
        ensure!(
            self.allocated_bytes <= self.budget.max_backup_bytes,
            "Dufs backup-byte budget exceeded"
        );
        self.entries.push(TreeEntry {
            path_base64,
            kind,
            mode,
            uid,
            gid,
            logical_bytes,
            allocated_bytes,
            content_sha256,
            symlink_target_base64,
            hardlink_to_base64,
            xattrs,
        });
        Ok(())
    }

    fn finish(mut self) -> anyhow::Result<TreeInventory> {
        for (_, (path, seen, expected)) in self.hardlinks {
            ensure!(
                seen == expected,
                "Dufs hard link at {path} has links outside the shared root"
            );
        }
        self.entries
            .sort_by(|left, right| left.path_base64.cmp(&right.path_base64));
        let bytes = serde_json::to_vec(&self.entries)?;
        Ok(TreeInventory {
            entries: self.entries.len() as u64,
            directories: self.directories,
            regular_files: self.regular_files,
            symbolic_links: self.symbolic_links,
            logical_bytes: self.logical_bytes,
            allocated_bytes: self.allocated_bytes,
            sha256: super::lower_hex(&Sha256::digest(bytes)),
            records: self.entries,
        })
    }
}

fn read_xattrs(path: &Path) -> anyhow::Result<BTreeMap<String, String>> {
    let names = match list_xattrs(path) {
        Ok(names) => names,
        Err(Errno::NOTSUP) => return Ok(BTreeMap::new()),
        Err(error) => return Err(std::io::Error::from(error)).context("list Dufs tree xattrs"),
    };
    ensure!(
        names.len() <= MAX_XATTR_LIST_BYTES,
        "Dufs xattr name-list budget exceeded"
    );
    let split = names
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();
    ensure!(
        split.len() <= MAX_XATTR_COUNT,
        "Dufs xattr count budget exceeded"
    );
    let mut total = 0_usize;
    let mut result = BTreeMap::new();
    for name in split {
        let name = OsStr::from_bytes(name);
        let value = get_xattr(path, name)?;
        ensure!(
            value.len() <= MAX_XATTR_VALUE_BYTES,
            "Dufs xattr value budget exceeded"
        );
        total = total
            .checked_add(value.len())
            .context("Dufs xattr size overflow")?;
        ensure!(
            total <= MAX_XATTR_BYTES_PER_ENTRY,
            "Dufs per-entry xattr budget exceeded"
        );
        result.insert(
            STANDARD_NO_PAD.encode(name.as_bytes()),
            super::lower_hex(&Sha256::digest(&value)),
        );
    }
    Ok(result)
}

fn copy_xattrs(source: &Path, destination: &Path) -> anyhow::Result<()> {
    let names = match list_xattrs(source) {
        Ok(names) => names,
        Err(Errno::NOTSUP) => return Ok(()),
        Err(error) => return Err(std::io::Error::from(error)).context("list source Dufs xattrs"),
    };
    for name in names
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
    {
        let name = OsStr::from_bytes(name);
        let value = get_xattr(source, name)?;
        lsetxattr(destination, name, &value, XattrFlags::empty())?;
    }
    Ok(())
}

fn list_xattrs(path: &Path) -> Result<Vec<u8>, Errno> {
    let mut buffer = vec![0_u8; MAX_XATTR_LIST_BYTES];
    let length = llistxattr(path, &mut buffer)?;
    buffer.truncate(length);
    Ok(buffer)
}

fn get_xattr(path: &Path, name: &OsStr) -> Result<Vec<u8>, Errno> {
    let mut buffer = vec![0_u8; MAX_XATTR_VALUE_BYTES];
    let length = lgetxattr(path, name, &mut buffer)?;
    buffer.truncate(length);
    Ok(buffer)
}

fn hash_file(path: &Path) -> anyhow::Result<String> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(super::lower_hex(&digest.finalize()))
}

fn copy_tree(source: &Path, destination: &Path, budget: DufsTreeBudget) -> anyhow::Result<()> {
    let source = source.canonicalize()?;
    let source_root_metadata = fs::symlink_metadata(&source)?;
    let mut hardlinks = BTreeMap::<(u64, u64), PathBuf>::new();
    let mut directory_metadata = Vec::new();
    let mut stack = vec![(PathBuf::new(), 0_usize)];
    let mut copied_entries = 0_u64;
    while let Some((relative, depth)) = stack.pop() {
        ensure!(depth <= MAX_TREE_DEPTH, "Dufs copy depth budget exceeded");
        let entries = read_directory_bounded(&source.join(&relative), budget)?;
        for entry in entries.into_iter().rev() {
            copied_entries = copied_entries
                .checked_add(1)
                .context("Dufs copy entry overflow")?;
            ensure!(
                copied_entries <= budget.max_entries,
                "Dufs copy entry budget exceeded"
            );
            let child = relative.join(entry.file_name());
            let source_path = source.join(&child);
            let destination_path = destination.join(&child);
            let metadata = fs::symlink_metadata(&source_path)?;
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                fs::create_dir(&destination_path)?;
                fs::set_permissions(&destination_path, fs::Permissions::from_mode(0o700))?;
                directory_metadata.push((
                    source_path.clone(),
                    destination_path.clone(),
                    metadata.clone(),
                ));
                stack.push((child, depth + 1));
            } else if metadata.is_file() && !metadata.file_type().is_symlink() {
                let key = (metadata.dev(), metadata.ino());
                if metadata.nlink() > 1 {
                    if let Some(first) = hardlinks.get(&key) {
                        fs::hard_link(first, &destination_path)?;
                    } else {
                        copy_sparse_file(&source_path, &destination_path, &metadata)?;
                        hardlinks.insert(key, destination_path.clone());
                    }
                } else {
                    copy_sparse_file(&source_path, &destination_path, &metadata)?;
                }
                apply_regular_metadata(&source_path, &destination_path, &metadata)?;
            } else if metadata.file_type().is_symlink() {
                let target = fs::read_link(&source_path)?;
                std::os::unix::fs::symlink(target, &destination_path)?;
                apply_symlink_ownership(&destination_path, &metadata)?;
                copy_xattrs(&source_path, &destination_path)?;
            } else {
                anyhow::bail!("Dufs tree contains an unsupported special entry")
            }
        }
    }
    for (source_path, destination_path, metadata) in directory_metadata.into_iter().rev() {
        apply_ownership(&destination_path, &metadata)?;
        copy_xattrs(&source_path, &destination_path)?;
        fs::set_permissions(
            &destination_path,
            fs::Permissions::from_mode(metadata.mode() & 0o7777),
        )?;
        apply_times(&destination_path, &metadata)?;
        File::open(&destination_path)?.sync_all()?;
    }
    apply_ownership(destination, &source_root_metadata)?;
    copy_xattrs(&source, destination)?;
    fs::set_permissions(
        destination,
        fs::Permissions::from_mode(source_root_metadata.mode() & 0o7777),
    )?;
    apply_times(destination, &source_root_metadata)?;
    File::open(destination)?.sync_all()?;
    Ok(())
}

fn copy_sparse_file(
    source_path: &Path,
    destination_path: &Path,
    metadata: &fs::Metadata,
) -> anyhow::Result<()> {
    let source_fd = open(
        source_path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    let source = File::from(source_fd);
    let destination = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(destination_path)?;
    destination.set_len(metadata.size())?;
    let mut offset = 0_u64;
    let mut buffer = vec![0_u8; 1024 * 1024];
    let sparse_supported = match seek(&source, SeekFrom::Data(0)) {
        Ok(_) | Err(Errno::NXIO) => true,
        Err(Errno::INVAL) | Err(Errno::NOTSUP) => false,
        Err(error) => {
            return Err(std::io::Error::from(error)).context("inspect Dufs sparse extents");
        }
    };
    if sparse_supported {
        while offset < metadata.size() {
            let data = match seek(&source, SeekFrom::Data(offset)) {
                Ok(value) => value,
                Err(Errno::NXIO) => break,
                Err(error) => {
                    return Err(std::io::Error::from(error)).context("find Dufs data extent");
                }
            };
            let hole = seek(&source, SeekFrom::Hole(data))?.min(metadata.size());
            copy_extent(&source, &destination, data, hole, &mut buffer)?;
            offset = hole;
        }
    } else {
        copy_extent(&source, &destination, 0, metadata.size(), &mut buffer)?;
    }
    destination.sync_all()?;
    Ok(())
}

fn copy_extent(
    source: &File,
    destination: &File,
    start: u64,
    end: u64,
    buffer: &mut [u8],
) -> anyhow::Result<()> {
    let mut offset = start;
    while offset < end {
        let wanted = usize::try_from((end - offset).min(buffer.len() as u64))?;
        let read = source.read_at(&mut buffer[..wanted], offset)?;
        ensure!(read > 0, "Dufs source file shortened during sparse copy");
        destination.write_all_at(&buffer[..read], offset)?;
        offset = offset
            .checked_add(read as u64)
            .context("Dufs sparse copy offset overflow")?;
    }
    Ok(())
}

fn apply_regular_metadata(
    source: &Path,
    destination: &Path,
    metadata: &fs::Metadata,
) -> anyhow::Result<()> {
    apply_ownership(destination, metadata)?;
    copy_xattrs(source, destination)?;
    fs::set_permissions(
        destination,
        fs::Permissions::from_mode(metadata.mode() & 0o7777),
    )?;
    apply_times(destination, metadata)?;
    Ok(())
}

fn apply_ownership(path: &Path, metadata: &fs::Metadata) -> anyhow::Result<()> {
    let file = OpenOptions::new().read(true).open(path)?;
    let current = fstat(&file)?;
    if current.st_uid != metadata.uid() || current.st_gid != metadata.gid() {
        fchown(
            &file,
            Some(Uid::from_raw(metadata.uid())),
            Some(Gid::from_raw(metadata.gid())),
        )?;
    }
    Ok(())
}

fn apply_symlink_ownership(path: &Path, metadata: &fs::Metadata) -> anyhow::Result<()> {
    let current = fs::symlink_metadata(path)?;
    if current.uid() != metadata.uid() || current.gid() != metadata.gid() {
        chownat(
            CWD,
            path,
            Some(Uid::from_raw(metadata.uid())),
            Some(Gid::from_raw(metadata.gid())),
            AtFlags::SYMLINK_NOFOLLOW,
        )?;
    }
    Ok(())
}

fn apply_times(path: &Path, metadata: &fs::Metadata) -> anyhow::Result<()> {
    let file = OpenOptions::new().read(true).open(path)?;
    futimens(
        &file,
        &Timestamps {
            last_access: Timespec {
                tv_sec: metadata.atime(),
                tv_nsec: metadata.atime_nsec(),
            },
            last_modification: Timespec {
                tv_sec: metadata.mtime(),
                tv_nsec: metadata.mtime_nsec(),
            },
        },
    )?;
    Ok(())
}

fn sync_tree(root: &Path) -> anyhow::Result<()> {
    let mut directories = vec![root.to_path_buf()];
    let mut index = 0;
    while index < directories.len() {
        for entry in fs::read_dir(&directories[index])? {
            let path = entry?.path();
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                directories.push(path);
            } else if metadata.is_file() {
                File::open(path)?.sync_all()?;
            }
        }
        index += 1;
    }
    for directory in directories.into_iter().rev() {
        File::open(directory)?.sync_all()?;
    }
    Ok(())
}
