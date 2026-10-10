//! Administrator controlled executable staging and service identity delegation.
use super::{UpgradeJournal, UpgradePlan, snapshot};
use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};
use xcsc::state_file::PrivateStateDirectory;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ExecutionSeal {
    pub root: PathBuf,
    pub parent_device: u64,
    pub parent_inode: u64,
    pub directory_device: u64,
    pub directory_inode: u64,
}

pub(super) fn identity(plan: &UpgradePlan) -> anyhow::Result<(u32, u32)> {
    let directory = PrivateStateDirectory::open_for_administration(&plan.data_dir)?;
    directory.verify_identity()?;
    Ok((directory.owner_uid(), directory.owner_gid()))
}

pub(super) fn verify_service(plan: &UpgradePlan) -> anyhow::Result<()> {
    let bytes = super::run_bounded(
        "/usr/bin/systemctl",
        &[
            "show".into(),
            plan.service.clone(),
            "--property=User".into(),
            "--property=Group".into(),
            "--property=DynamicUser".into(),
        ],
        plan.timeout_seconds,
    )?;
    let value = std::str::from_utf8(&bytes)?;
    let user = value
        .lines()
        .find_map(|line| line.strip_prefix("User="))
        .context("service User authority missing")?;
    let group = value
        .lines()
        .find_map(|line| line.strip_prefix("Group="))
        .context("service Group authority missing")?;
    ensure!(
        !value.lines().any(|line| line == "DynamicUser=yes"),
        "dynamic service identities are not supported"
    );
    let (uid, default_gid) = account(user)?;
    let gid = if group.is_empty() {
        default_gid
    } else {
        group_id(group)?
    };
    ensure!(
        (uid, gid) == identity(plan)?,
        "systemd service identity differs from the private data owner"
    );
    Ok(())
}

#[allow(unsafe_code)] // Reentrant NSS lookup; see docs/unsafe-audit.md.
fn account(name: &str) -> anyhow::Result<(u32, u32)> {
    if name.is_empty() {
        return Ok((0, 0));
    }
    let mut storage = vec![0u8; 65536];
    let mut entry = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut pointer = std::ptr::null_mut();
    let cname = std::ffi::CString::new(name)?;
    // SAFETY: CString and both output buffers live through the reentrant NSS
    // call. The initialized passwd result is read only after success, and NSS
    // must return the exact output pointer supplied to it.
    let result = unsafe {
        if let Ok(uid) = name.parse::<u32>() {
            libc::getpwuid_r(
                uid,
                entry.as_mut_ptr(),
                storage.as_mut_ptr().cast(),
                storage.len(),
                &mut pointer,
            )
        } else {
            libc::getpwnam_r(
                cname.as_ptr(),
                entry.as_mut_ptr(),
                storage.as_mut_ptr().cast(),
                storage.len(),
                &mut pointer,
            )
        }
    };
    ensure!(
        result == 0 && pointer == entry.as_mut_ptr(),
        "service account cannot be resolved"
    );
    // SAFETY: successful NSS lookup above initialized this exact output value.
    let entry = unsafe { entry.assume_init() };
    Ok((entry.pw_uid, entry.pw_gid))
}

#[allow(unsafe_code)] // Reentrant NSS lookup; see docs/unsafe-audit.md.
fn group_id(name: &str) -> anyhow::Result<u32> {
    if let Ok(gid) = name.parse::<u32>() {
        return Ok(gid);
    }
    let mut storage = vec![0u8; 65536];
    let mut entry = std::mem::MaybeUninit::<libc::group>::uninit();
    let mut pointer = std::ptr::null_mut();
    let cname = std::ffi::CString::new(name)?;
    // SAFETY: the NUL-terminated input and bounded scratch/output storage remain
    // valid for the reentrant NSS call; its result pointer is checked below.
    let result = unsafe {
        libc::getgrnam_r(
            cname.as_ptr(),
            entry.as_mut_ptr(),
            storage.as_mut_ptr().cast(),
            storage.len(),
            &mut pointer,
        )
    };
    ensure!(
        result == 0 && pointer == entry.as_mut_ptr(),
        "service group cannot be resolved"
    );
    // SAFETY: successful NSS lookup above initialized this exact output value.
    Ok(unsafe { entry.assume_init() }.gr_gid)
}

fn parent(journal: &UpgradeJournal) -> anyhow::Result<&Path> {
    if let Some(native) = &journal.plan.native_release {
        Ok(native
            .current_link
            .parent()
            .context("selector parent missing")?)
    } else {
        Ok(journal
            .plan
            .installed_binary
            .parent()
            .context("program parent missing")?)
    }
}

pub(super) fn require_trusted(path: &Path) -> anyhow::Result<()> {
    snapshot::require_path(path)?;
    let mut prefix = PathBuf::new();
    for component in path.components() {
        prefix.push(component);
        let metadata = fs::symlink_metadata(&prefix)?;
        // A root-owned sticky temporary ancestor protects root-owned children;
        // the execution root and its descendants themselves never permit 022.
        ensure!(
            metadata.is_dir()
                && metadata.uid() == 0
                && (metadata.mode() & 0o022 == 0
                    || (metadata.mode() & 0o1000 != 0 && prefix == Path::new("/tmp")))
                && metadata.mode() & 0o005 == 0o005,
            "execution ancestry must be traversable and administrator controlled"
        );
    }
    Ok(())
}

pub(super) fn prepare(journal: &mut UpgradeJournal) -> anyhow::Result<()> {
    let (uid, _) = identity(&journal.plan)?;
    if uid == rustix::process::geteuid().as_raw() {
        return Ok(());
    }
    ensure!(
        rustix::process::geteuid().as_raw() == 0,
        "only the administrator may delegate identity"
    );
    let parent = parent(journal)?;
    require_trusted(parent)?;
    let metadata = fs::symlink_metadata(parent)?;
    let root = parent.join(format!(".xssc-execution-{}", journal.operation_id));
    fs::create_dir(&root)?;
    fs::set_permissions(&root, fs::Permissions::from_mode(0o755))?;
    snapshot::sync_parent(&root)?;
    let directory = fs::symlink_metadata(&root)?;
    journal.execution_seal = Some(ExecutionSeal {
        root,
        parent_device: metadata.dev(),
        parent_inode: metadata.ino(),
        directory_device: directory.dev(),
        directory_inode: directory.ino(),
    });
    Ok(())
}

pub(super) fn verify(journal: &UpgradeJournal) -> anyhow::Result<()> {
    if let Some(seal) = &journal.execution_seal {
        ensure!(
            seal.root.as_os_str()
                == parent(journal)?
                    .join(format!(".xssc-execution-{}", journal.operation_id))
                    .as_os_str(),
            "execution root mapping differs"
        );
        require_trusted(&seal.root)?;
        let parent = fs::symlink_metadata(parent(journal)?)?;
        let directory = fs::symlink_metadata(&seal.root)?;
        ensure!(
            parent.dev() == seal.parent_device
                && parent.ino() == seal.parent_inode
                && directory.dev() == seal.directory_device
                && directory.ino() == seal.directory_inode,
            "execution root identity changed"
        );
    }
    Ok(())
}

pub(super) fn root(journal: &UpgradeJournal) -> anyhow::Result<PathBuf> {
    verify(journal)?;
    Ok(journal.execution_seal.as_ref().map_or_else(
        || journal.plan.work_directory.clone(),
        |seal| seal.root.clone(),
    ))
}
