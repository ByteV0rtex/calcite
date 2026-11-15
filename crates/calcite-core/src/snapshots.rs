//! Instance snapshots: a saved copy of an instance's mods, configs and packs
//! that can be restored in one step. Copies are clones where the filesystem
//! allows it, so a snapshot costs almost no disk space.
//!
//! Worlds are not part of a snapshot (they have their own backups), and
//! neither are logs, crash reports or screenshots.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use anyhow::{Context as _, bail};
use serde::{Deserialize, Serialize};

use crate::clone;

/// The parts of an instance a snapshot covers.
pub const COVERED: [&str; 9] = [
    "mods",
    "config",
    "defaultconfigs",
    "kubejs",
    "resourcepacks",
    "shaderpacks",
    "options.txt",
    "servers.dat",
    "datapacks",
];

const META: &str = "snapshot.json";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Snapshot {
    /// Folder name, also the id.
    pub id: String,
    pub label: String,
    pub created: chrono::DateTime<chrono::Utc>,
    pub files: u64,
    pub bytes: u64,
    /// True if taken automatically (before an update, a restore, …).
    pub automatic: bool,
}

/// Where an instance's snapshots live.
pub fn dir_for(snapshots_root: &Path, instance_id: &str) -> PathBuf {
    let safe: String = instance_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    snapshots_root.join(safe)
}

/// Save the covered parts of `instance_dir`.
pub fn take(
    instance_dir: &Path,
    instance_snapshots: &Path,
    label: &str,
    automatic: bool,
) -> anyhow::Result<Snapshot> {
    let now = chrono::Utc::now();
    let mut id = now.format("%Y%m%d-%H%M%S").to_string();
    let mut n = 1;
    while instance_snapshots.join(&id).exists() {
        n += 1;
        id = format!("{}-{n}", now.format("%Y%m%d-%H%M%S"));
    }
    let target = instance_snapshots.join(&id);
    std::fs::create_dir_all(&target)?;
    let cancel = AtomicBool::new(false);
    let mut files = 0;
    let mut bytes = 0;
    for item in COVERED {
        let src = instance_dir.join(item);
        let dst = target.join(item);
        if src.is_dir() {
            let report = clone::clone_tree(&src, &dst, &cancel, |_| {})?;
            files += report.files;
            bytes += report.bytes;
        } else if src.is_file() {
            reflink_copy::reflink_or_copy(&src, &dst)
                .with_context(|| format!("saving {}", src.display()))?;
            files += 1;
            bytes += std::fs::metadata(&src).map(|m| m.len()).unwrap_or(0);
        }
    }
    let snapshot = Snapshot {
        id,
        label: label.to_string(),
        created: now,
        files,
        bytes,
        automatic,
    };
    std::fs::write(target.join(META), serde_json::to_vec_pretty(&snapshot)?)?;
    Ok(snapshot)
}

/// All snapshots of an instance, newest first.
pub fn list(instance_snapshots: &Path) -> Vec<Snapshot> {
    let mut list: Vec<Snapshot> = std::fs::read_dir(instance_snapshots)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| std::fs::read(e.path().join(META)).ok())
        .filter_map(|b| serde_json::from_slice(&b).ok())
        .collect();
    list.sort_by_key(|s| std::cmp::Reverse(s.created));
    list
}

/// Put a snapshot back. What's there now is snapshotted first ("Before
/// restoring …"), so a restore can itself be undone.
pub fn restore(
    instance_dir: &Path,
    instance_snapshots: &Path,
    id: &str,
) -> anyhow::Result<Snapshot> {
    let source = instance_snapshots.join(id);
    let meta: Snapshot = serde_json::from_slice(
        &std::fs::read(source.join(META)).context("that snapshot is missing")?,
    )?;
    let safety = take(
        instance_dir,
        instance_snapshots,
        &format!("Before restoring \u{201c}{}\u{201d}", meta.label),
        true,
    )?;
    let cancel = AtomicBool::new(false);
    for item in COVERED {
        let current = instance_dir.join(item);
        let saved = source.join(item);
        if current.is_dir() {
            std::fs::remove_dir_all(&current)?;
        } else if current.is_file() {
            std::fs::remove_file(&current)?;
        }
        if saved.is_dir() {
            clone::clone_tree(&saved, &current, &cancel, |_| {})?;
        } else if saved.is_file() {
            reflink_copy::reflink_or_copy(&saved, &current)?;
        }
    }
    Ok(safety)
}

pub fn delete(instance_snapshots: &Path, id: &str) -> anyhow::Result<()> {
    if id.contains(['/', '\\']) || id.starts_with('.') {
        bail!("invalid snapshot id");
    }
    std::fs::remove_dir_all(instance_snapshots.join(id))?;
    Ok(())
}

/// Keep at most `keep` automatic snapshots (manual ones are never pruned).
pub fn prune_automatic(instance_snapshots: &Path, keep: usize) {
    let automatic: Vec<Snapshot> = list(instance_snapshots)
        .into_iter()
        .filter(|s| s.automatic)
        .collect();
    for old in automatic.into_iter().skip(keep) {
        let _ = delete(instance_snapshots, &old.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn instance(root: &Path) {
        fs::create_dir_all(root.join("mods")).unwrap();
        fs::write(root.join("mods/sodium-1.jar"), b"v1").unwrap();
        fs::create_dir_all(root.join("config")).unwrap();
        fs::write(root.join("config/sodium.json"), b"{\"a\":1}").unwrap();
        fs::write(root.join("options.txt"), b"fov:70").unwrap();
        fs::create_dir_all(root.join("saves/World")).unwrap();
        fs::write(root.join("saves/World/level.dat"), b"world").unwrap();
    }

    #[test]
    fn take_change_restore() {
        let tmp = tempfile::tempdir().unwrap();
        let inst = tmp.path().join("inst");
        let snaps = dir_for(&tmp.path().join("snapshots"), "local:abc/def");
        instance(&inst);

        let snap = take(&inst, &snaps, "Before update", false).unwrap();
        assert_eq!(snap.files, 3);
        assert!(!snaps.join(&snap.id).join("saves").exists());

        // An update swaps the mod and changes a setting; the world moves on.
        fs::remove_file(inst.join("mods/sodium-1.jar")).unwrap();
        fs::write(inst.join("mods/sodium-2.jar"), b"v2").unwrap();
        fs::write(inst.join("options.txt"), b"fov:90").unwrap();
        fs::write(inst.join("saves/World/level.dat"), b"world-later").unwrap();

        let safety = restore(&inst, &snaps, &snap.id).unwrap();
        assert!(safety.automatic);
        assert!(inst.join("mods/sodium-1.jar").exists());
        assert!(!inst.join("mods/sodium-2.jar").exists());
        assert_eq!(fs::read(inst.join("options.txt")).unwrap(), b"fov:70");
        // Worlds are never rolled back.
        assert_eq!(
            fs::read(inst.join("saves/World/level.dat")).unwrap(),
            b"world-later"
        );

        let all = list(&snaps);
        assert_eq!(all.len(), 2);
        assert!(all[0].label.starts_with("Before restoring"));

        // And the restore can be undone.
        restore(&inst, &snaps, &safety.id).unwrap();
        assert!(inst.join("mods/sodium-2.jar").exists());
    }

    #[test]
    fn pruning_keeps_manual_snapshots() {
        let tmp = tempfile::tempdir().unwrap();
        let inst = tmp.path().join("inst");
        let snaps = tmp.path().join("snaps");
        instance(&inst);
        let manual = take(&inst, &snaps, "Mine", false).unwrap();
        for i in 0..4 {
            take(&inst, &snaps, &format!("auto {i}"), true).unwrap();
        }
        prune_automatic(&snaps, 2);
        let left = list(&snaps);
        assert_eq!(left.iter().filter(|s| s.automatic).count(), 2);
        assert!(left.iter().any(|s| s.id == manual.id));
    }

    #[test]
    fn delete_rejects_paths() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(delete(tmp.path(), "../x").is_err());
    }
}
