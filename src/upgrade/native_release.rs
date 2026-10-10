//! Complete immutable deployment trees. Product code defines its physical
//! layout; this module never chooses a product, runs a shell or edits a unit.
use super::{Entry, UpgradeJournal, UpgradePlan, UpgradeRelease, snapshot};
use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    path::{Component, Path, PathBuf},
};
use xcsc::state_file::{MaintenanceLock, PrivateStateDirectory};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseArtifact {
    pub protocol: String,
    pub root_layout: PathBuf,
    pub entrypoint: PathBuf,
    pub tree_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic_release_root_env: Option<String>,
}

fn validate_diagnostic_environment(name: &str) -> anyhow::Result<()> {
    ensure!(
        name.len() <= 64
            && name.ends_with("_RELEASE_ROOT")
            && name.len() > "_RELEASE_ROOT".len()
            && name.as_bytes()[0].is_ascii_uppercase()
            && name
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'),
        "unsupported release-root diagnostic environment name"
    );
    Ok(())
}

pub(super) fn diagnostic_environment(
    binary: &Path,
    artifact: Option<&ReleaseArtifact>,
) -> anyhow::Result<Option<(String, PathBuf)>> {
    let Some(artifact) = artifact else {
        return Ok(None);
    };
    let Some(name) = &artifact.diagnostic_release_root_env else {
        return Ok(None);
    };
    validate_diagnostic_environment(name)?;
    relative(&artifact.entrypoint)?;
    let mut root = binary.to_owned();
    for _ in artifact.entrypoint.components() {
        ensure!(root.pop(), "diagnostic binary lacks its physical root");
    }
    snapshot::require_path(&root)?;
    ensure!(
        root.join(&artifact.entrypoint).as_os_str() == binary.as_os_str(),
        "diagnostic entrypoint differs"
    );
    Ok(Some((name.clone(), root)))
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeReleasePlan {
    pub current_link: PathBuf,
    pub source_root: PathBuf,
    pub install_root: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NativeBackup {
    pub source_entries: Vec<Entry>,
    pub target_entries: Vec<Entry>,
    pub current_parent_device: u64,
    pub current_parent_inode: u64,
    pub source_directory_device: u64,
    pub source_directory_inode: u64,
    pub source_recovery_started: bool,
}

fn relative(path: &Path) -> anyhow::Result<()> {
    ensure!(
        !path.as_os_str().is_empty()
            && path.as_os_str().as_encoded_bytes().len() <= 4096
            && path
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
            && path.components().collect::<PathBuf>().as_os_str() == path.as_os_str(),
        "invalid immutable release relative path"
    );
    ensure!(path.to_str().is_some(), "release paths must be UTF-8");
    Ok(())
}

pub(super) fn validate_plan(plan: &UpgradePlan) -> anyhow::Result<()> {
    let Some(native) = &plan.native_release else {
        return Ok(());
    };
    for path in [&native.source_root, &native.install_root] {
        snapshot::require_path(path)?;
        ensure!(
            path != Path::new("/"),
            "release root cannot be filesystem root"
        );
    }
    let parent = native
        .current_link
        .parent()
        .context("current link has no parent")?;
    snapshot::require_path(parent)?;
    ensure!(
        native.current_link.is_absolute(),
        "current link must be absolute"
    );
    let name = native
        .current_link
        .file_name()
        .context("current link has no name")?;
    ensure!(
        name == "current",
        "the deployment selector must be named current"
    );
    ensure!(
        native.source_root.parent() == Some(parent.join("releases").as_path())
            && native.install_root.parent() == native.source_root.parent(),
        "selector may only choose its physical releases children"
    );
    ensure!(
        native.source_root != native.install_root
            && !native.source_root.starts_with(&native.install_root)
            && !native.install_root.starts_with(&native.source_root),
        "source and target immutable roots overlap"
    );
    ensure!(
        plan.installed_binary.starts_with(&native.current_link)
            && plan.installed_binary != native.current_link,
        "installed binary must use the controlled current selector"
    );
    relative(plan.installed_binary.strip_prefix(&native.current_link)?)?;
    for root in [
        &native.source_root,
        &native.install_root,
        &native.current_link,
    ] {
        for resource in &plan.resources {
            ensure!(
                !root.starts_with(&resource.path) && !resource.path.starts_with(root),
                "release and persistent state overlap"
            );
        }
        ensure!(
            !root.starts_with(&plan.work_directory) && !plan.work_directory.starts_with(root),
            "release and recovery roots overlap"
        );
    }
    Ok(())
}

pub(super) fn validate_artifact(
    plan: &UpgradePlan,
    release: &UpgradeRelease,
) -> anyhow::Result<()> {
    match (&plan.native_release, &release.artifact) {
        (None, None) => return Ok(()),
        (Some(_), Some(_)) => {}
        _ => anyhow::bail!("deployment plan and signed artifact disagree"),
    }
    let native = plan.native_release.as_ref().unwrap();
    let artifact = release.artifact.as_ref().unwrap();
    ensure!(
        native
            .source_root
            .file_name()
            .and_then(|name| name.to_str())
            == Some(release.source_identity.version.as_str()),
        "original release root does not name its signed current software version"
    );
    ensure!(
        artifact.protocol == "immutable-release-root-v1",
        "unsupported release artifact protocol"
    );
    relative(&artifact.root_layout)?;
    relative(&artifact.entrypoint)?;
    if let Some(name) = &artifact.diagnostic_release_root_env {
        validate_diagnostic_environment(name)?;
    }
    super::validate_digest(&artifact.tree_sha256)?;
    ensure!(
        artifact
            .root_layout
            .file_name()
            .and_then(|name| name.to_str())
            == Some(release.identity.version.as_str()),
        "release layout must name its exact software version"
    );
    ensure!(
        native.install_root.ends_with(&artifact.root_layout),
        "installation does not preserve the product physical layout"
    );
    ensure!(
        plan.installed_binary == native.current_link.join(&artifact.entrypoint),
        "selector entrypoint differs from signed authority"
    );
    let input = input_root(plan, artifact)?;
    ensure!(
        input.ends_with(&artifact.root_layout),
        "input does not preserve the product physical layout"
    );
    for resource in &plan.resources {
        ensure!(
            !input.starts_with(&resource.path) && !resource.path.starts_with(&input),
            "input release and persistent state overlap"
        );
    }
    ensure!(
        !input.starts_with(&plan.work_directory) && !plan.work_directory.starts_with(&input),
        "input and recovery roots overlap"
    );
    Ok(())
}

fn input_root(plan: &UpgradePlan, artifact: &ReleaseArtifact) -> anyhow::Result<PathBuf> {
    let count = artifact.entrypoint.components().count();
    let mut root = plan.target_binary.clone();
    for _ in 0..count {
        ensure!(root.pop(), "target entrypoint has no root");
    }
    ensure!(
        root.join(&artifact.entrypoint) == plan.target_binary,
        "target entrypoint differs from signed authority"
    );
    snapshot::require_path(&root)?;
    Ok(root)
}

fn immutable_entries(root: &Path, limit: u64) -> anyhow::Result<Vec<Entry>> {
    let owner_uid = fs::symlink_metadata(root)?.uid();
    let caller_uid = rustix::process::geteuid().as_raw();
    ensure!(
        owner_uid == 0 || owner_uid == caller_uid || caller_uid == 0,
        "immutable release belongs to another user"
    );
    let entries = snapshot::inventory_owned(root, true, limit, false, owner_uid)?;
    ensure!(
        entries.iter().all(|entry| entry.mode & 0o022 == 0),
        "release tree cannot be writable by its service or other users"
    );
    for entry in &entries {
        let path = if entry.path.as_os_str().is_empty() {
            root.to_path_buf()
        } else {
            root.join(&entry.path)
        };
        ensure!(
            fs::symlink_metadata(path)?.mode() & 0o7000 == 0,
            "release tree contains special permission bits"
        );
    }
    Ok(entries)
}

/// Exact preorder inventory, sorted siblings, UTF-8 paths, and explicit empty
/// root path. JSON field order is path,directory,mode,bytes,sha256. Packaging
/// implements this same byte contract, including the domain separator.
pub(super) fn tree_digest(entries: &[Entry]) -> anyhow::Result<String> {
    let mut digest = Sha256::new();
    digest.update(b"immutable-release-root-v1\n");
    digest.update(serde_json::to_vec(entries)?);
    Ok(super::digest_hex(digest.finalize()))
}

fn current_parent(plan: &UpgradePlan) -> anyhow::Result<fs::Metadata> {
    let native = plan
        .native_release
        .as_ref()
        .context("native deployment missing")?;
    let parent = native
        .current_link
        .parent()
        .context("current parent missing")?;
    snapshot::require_path(parent)?;
    let metadata = fs::symlink_metadata(parent)?;
    ensure!(
        metadata.is_dir()
            && (metadata.uid() == 0 || metadata.uid() == rustix::process::geteuid().as_raw())
            && metadata.mode() & 0o7777 == 0o755,
        "current parent must be a physical owned 0755 directory"
    );
    let releases = native
        .source_root
        .parent()
        .context("release parent missing")?;
    snapshot::require_path(releases)?;
    let release_parent = fs::symlink_metadata(releases)?;
    ensure!(
        release_parent.is_dir()
            && release_parent.uid() == metadata.uid()
            && release_parent.mode() & 0o7777 == 0o755,
        "releases parent must be physical 0755 with the selector parent's owner"
    );
    Ok(metadata)
}

fn current_root(plan: &UpgradePlan) -> anyhow::Result<PathBuf> {
    let native = plan
        .native_release
        .as_ref()
        .context("native deployment missing")?;
    let parent = current_parent(plan)?;
    let metadata = fs::symlink_metadata(&native.current_link)?;
    ensure!(
        metadata.file_type().is_symlink() && metadata.uid() == parent.uid(),
        "current must be an owned symbolic link"
    );
    let target = fs::read_link(&native.current_link)?;
    ensure!(
        target.as_os_str() == native.source_root.as_os_str()
            || target.as_os_str() == native.install_root.as_os_str(),
        "current points to an unknown release"
    );
    snapshot::require_path(&target)?;
    Ok(target)
}

pub(super) fn installed_binary(plan: &UpgradePlan) -> anyhow::Result<PathBuf> {
    let Some(native) = &plan.native_release else {
        return Ok(plan.installed_binary.clone());
    };
    let entrypoint = plan.installed_binary.strip_prefix(&native.current_link)?;
    Ok(current_root(plan)?.join(entrypoint))
}

pub(super) fn program_status(journal: &UpgradeJournal) -> anyhow::Result<&'static str> {
    verify_parent(journal)?;
    let native = journal
        .plan
        .native_release
        .as_ref()
        .context("native plan missing")?;
    let backup = journal
        .native_backup
        .as_ref()
        .context("native backup missing")?;
    let root = current_root(&journal.plan)?;
    let actual = immutable_entries(&root, journal.plan.max_backup_bytes)?;
    Ok(
        if root == native.source_root && actual == backup.source_entries {
            "original"
        } else if root == native.install_root && actual == backup.target_entries {
            "target"
        } else {
            "unknown"
        },
    )
}

pub(super) fn coordinate(plan: &UpgradePlan) -> anyhow::Result<Option<MaintenanceLock>> {
    let Some(native) = &plan.native_release else {
        return Ok(None);
    };
    current_parent(plan)?;
    let directory = PrivateStateDirectory::create(
        native
            .current_link
            .parent()
            .unwrap()
            .join(".release-upgrade"),
    )?;
    Ok(Some(directory.try_maintenance_lock()?))
}

pub(super) fn verify_input(plan: &UpgradePlan, release: &UpgradeRelease) -> anyhow::Result<()> {
    validate_artifact(plan, release)?;
    let Some(artifact) = &release.artifact else {
        return Ok(());
    };
    let entries = immutable_entries(&input_root(plan, artifact)?, plan.max_backup_bytes)?;
    ensure!(
        tree_digest(&entries)? == artifact.tree_sha256,
        "complete target release checksum differs"
    );
    Ok(())
}

pub(super) fn staged_root(journal: &UpgradeJournal) -> anyhow::Result<PathBuf> {
    Ok(super::execution::root(journal)?
        .join("target-release")
        .join(
            &journal
                .release
                .artifact
                .as_ref()
                .context("artifact missing")?
                .root_layout,
        ))
}

pub(super) fn staged_binary(journal: &UpgradeJournal) -> anyhow::Result<PathBuf> {
    if let Some(artifact) = &journal.release.artifact {
        Ok(staged_root(journal)?.join(&artifact.entrypoint))
    } else {
        Ok(super::execution::root(journal)?.join("target-binary"))
    }
}

pub(super) fn seal(journal: &mut UpgradeJournal) -> anyhow::Result<()> {
    let native = journal
        .plan
        .native_release
        .as_ref()
        .context("native plan missing")?;
    let artifact = journal
        .release
        .artifact
        .as_ref()
        .context("artifact missing")?;
    ensure!(
        current_root(&journal.plan)? == native.source_root,
        "current no longer selects original release"
    );
    let parent = current_parent(&journal.plan)?;
    let source_metadata = fs::symlink_metadata(&native.source_root)?;
    let source_entries = immutable_entries(&native.source_root, journal.plan.max_backup_bytes)?;
    let target_entries = immutable_entries(
        &input_root(&journal.plan, artifact)?,
        journal.plan.max_backup_bytes,
    )?;
    ensure!(
        tree_digest(&target_entries)? == artifact.tree_sha256,
        "target changed before sealing"
    );
    if snapshot::named_exists(&native.install_root)? {
        ensure!(
            immutable_entries(&native.install_root, journal.plan.max_backup_bytes)?
                == target_entries,
            "target installation already contains different data"
        );
    }
    let staged = staged_root(journal)?;
    create_parents(staged.parent().unwrap(), &super::execution::root(journal)?)?;
    snapshot::snapshot(
        &input_root(&journal.plan, artifact)?,
        &staged,
        &target_entries,
    )?;
    ensure!(
        immutable_entries(&staged, journal.plan.max_backup_bytes)? == target_entries,
        "sealed release changed"
    );
    journal.native_backup = Some(NativeBackup {
        source_entries,
        target_entries,
        current_parent_device: parent.dev(),
        current_parent_inode: parent.ino(),
        source_directory_device: source_metadata.dev(),
        source_directory_inode: source_metadata.ino(),
        source_recovery_started: false,
    });
    Ok(())
}

fn create_parents(path: &Path, boundary: &Path) -> anyhow::Result<()> {
    ensure!(
        path.starts_with(boundary),
        "staging parent escaped recovery root"
    );
    if path == boundary {
        return Ok(());
    }
    create_parents(path.parent().context("staging parent missing")?, boundary)?;
    if !snapshot::named_exists(path)? {
        fs::create_dir(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
        snapshot::sync_parent(path)?;
    }
    snapshot::require_path(path)
}

fn verify_parent(journal: &UpgradeJournal) -> anyhow::Result<()> {
    let backup = journal
        .native_backup
        .as_ref()
        .context("native backup missing")?;
    let parent = current_parent(&journal.plan)?;
    ensure!(
        parent.dev() == backup.current_parent_device && parent.ino() == backup.current_parent_inode,
        "deployment parent identity changed"
    );
    Ok(())
}

pub(super) fn backup_original(journal: &UpgradeJournal) -> anyhow::Result<u64> {
    let native = journal
        .plan
        .native_release
        .as_ref()
        .context("native plan missing")?;
    let backup = journal
        .native_backup
        .as_ref()
        .context("native backup missing")?;
    ensure!(
        immutable_entries(&native.source_root, journal.plan.max_backup_bytes)?
            == backup.source_entries,
        "original complete release changed"
    );
    snapshot::snapshot(
        &native.source_root,
        &journal.plan.work_directory.join("original-release"),
        &backup.source_entries,
    )?;
    let bytes = backup
        .source_entries
        .iter()
        .try_fold(0u64, |total, entry| {
            total
                .checked_add(entry.bytes)
                .context("release size overflow")
        })?;
    verify_original_backup(journal)?;
    Ok(bytes)
}

pub(super) fn verify_original_backup(journal: &UpgradeJournal) -> anyhow::Result<()> {
    let Some(backup) = &journal.native_backup else {
        return Ok(());
    };
    ensure!(
        immutable_entries(
            &journal.plan.work_directory.join("original-release"),
            journal.plan.max_backup_bytes
        )? == backup.source_entries,
        "original complete release backup changed"
    );
    Ok(())
}

pub(super) fn verify_sealed(journal: &UpgradeJournal) -> anyhow::Result<()> {
    let Some(backup) = &journal.native_backup else {
        return Ok(());
    };
    ensure!(
        immutable_entries(&staged_root(journal)?, journal.plan.max_backup_bytes)?
            == backup.target_entries,
        "sealed complete release changed"
    );
    ensure!(
        tree_digest(&backup.target_entries)?
            == journal
                .release
                .artifact
                .as_ref()
                .context("artifact missing")?
                .tree_sha256,
        "signed complete release digest changed"
    );
    Ok(())
}

fn publish_directory(stage: &Path, target: &Path) -> anyhow::Result<()> {
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        stage,
        rustix::fs::CWD,
        target,
        rustix::fs::RenameFlags::NOREPLACE,
    )?;
    snapshot::sync_parent(target)
}

pub(super) fn install_target(journal: &UpgradeJournal) -> anyhow::Result<()> {
    verify_parent(journal)?;
    verify_sealed(journal)?;
    let native = journal
        .plan
        .native_release
        .as_ref()
        .context("native plan missing")?;
    let backup = journal
        .native_backup
        .as_ref()
        .context("native backup missing")?;
    snapshot::require_path(native.install_root.parent().unwrap())?;
    if !snapshot::named_exists(&native.install_root)? {
        let stage = native
            .install_root
            .with_file_name(format!(".upgrade-release-{}", uuid::Uuid::new_v4()));
        snapshot::snapshot(&staged_root(journal)?, &stage, &backup.target_entries)?;
        ensure!(
            immutable_entries(&stage, journal.plan.max_backup_bytes)? == backup.target_entries,
            "new installation changed while copying"
        );
        publish_directory(&stage, &native.install_root)?;
    }
    ensure!(
        immutable_entries(&native.install_root, journal.plan.max_backup_bytes)?
            == backup.target_entries,
        "target complete installation differs"
    );
    switch(journal, &native.install_root)
}

fn switch(journal: &UpgradeJournal, target: &Path) -> anyhow::Result<()> {
    verify_parent(journal)?;
    current_root(&journal.plan)?;
    let native = journal
        .plan
        .native_release
        .as_ref()
        .context("native plan missing")?;
    let stage = native
        .current_link
        .with_file_name(format!(".upgrade-current-{}", uuid::Uuid::new_v4()));
    symlink(target, &stage)?;
    fs::rename(&stage, &native.current_link)?;
    snapshot::sync_parent(&native.current_link)?;
    ensure!(
        current_root(&journal.plan)? == target,
        "release selector switch failed"
    );
    Ok(())
}

pub(super) fn original_needs_repair(journal: &UpgradeJournal) -> anyhow::Result<bool> {
    let native = journal
        .plan
        .native_release
        .as_ref()
        .context("native plan missing")?;
    let backup = journal
        .native_backup
        .as_ref()
        .context("native backup missing")?;
    Ok(
        immutable_entries(&native.source_root, journal.plan.max_backup_bytes)
            .ok()
            .as_ref()
            != Some(&backup.source_entries),
    )
}

fn displaced_root(journal: &UpgradeJournal) -> anyhow::Result<PathBuf> {
    let root = &journal
        .plan
        .native_release
        .as_ref()
        .context("native plan missing")?
        .source_root;
    Ok(root.with_file_name(format!(
        ".upgrade-displaced-original-{}",
        journal.operation_id
    )))
}

pub(super) fn permitted_missing_original(journal: &UpgradeJournal) -> anyhow::Result<bool> {
    let Some(backup) = &journal.native_backup else {
        return Ok(false);
    };
    if !backup.source_recovery_started {
        return Ok(false);
    }
    let native = journal.plan.native_release.as_ref().unwrap();
    if current_root(&journal.plan)? != native.source_root
        || snapshot::named_exists(&native.source_root)?
    {
        return Ok(false);
    }
    let metadata = fs::symlink_metadata(displaced_root(journal)?)?;
    Ok(metadata.is_dir()
        && !metadata.file_type().is_symlink()
        && metadata.dev() == backup.source_directory_device
        && metadata.ino() == backup.source_directory_inode)
}

pub(super) fn restore_original(journal: &UpgradeJournal) -> anyhow::Result<()> {
    verify_parent(journal)?;
    verify_original_backup(journal)?;
    let native = journal
        .plan
        .native_release
        .as_ref()
        .context("native plan missing")?;
    let backup = journal
        .native_backup
        .as_ref()
        .context("native backup missing")?;
    if original_needs_repair(journal)? {
        ensure!(
            backup.source_recovery_started,
            "original root repair intent was not persisted"
        );
        let displaced = displaced_root(journal)?;
        if snapshot::named_exists(&native.source_root)? {
            let metadata = fs::symlink_metadata(&native.source_root)?;
            ensure!(
                metadata.is_dir()
                    && metadata.dev() == backup.source_directory_device
                    && metadata.ino() == backup.source_directory_inode,
                "original release directory was replaced; preserve unknown data"
            );
            publish_directory(&native.source_root, &displaced)?;
        } else {
            let metadata = fs::symlink_metadata(&displaced)?;
            ensure!(
                metadata.is_dir()
                    && metadata.dev() == backup.source_directory_device
                    && metadata.ino() == backup.source_directory_inode,
                "original repair evidence differs"
            );
        }
        let stage = native.source_root.with_file_name(format!(
            ".upgrade-restored-original-{}",
            uuid::Uuid::new_v4()
        ));
        snapshot::snapshot(
            &journal.plan.work_directory.join("original-release"),
            &stage,
            &backup.source_entries,
        )?;
        ensure!(
            immutable_entries(&stage, journal.plan.max_backup_bytes)? == backup.source_entries,
            "restored complete release differs"
        );
        publish_directory(&stage, &native.source_root)?;
    }
    ensure!(
        immutable_entries(&native.source_root, journal.plan.max_backup_bytes)?
            == backup.source_entries,
        "original complete release differs"
    );
    switch(journal, &native.source_root)
}
