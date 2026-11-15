//! Copying folders without using disk space where the filesystem allows it.
//!
//! On APFS (macOS), btrfs/XFS (Linux) and ReFS (Windows) files are cloned:
//! the copy shares blocks with the original until either one changes, so a
//! 10 GB instance folder costs almost nothing. Elsewhere files are copied
//! normally, after checking there is room. The source is only ever read.

use std::fs;
#[cfg(windows)]
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context as _, bail};
use walkdir::WalkDir;

/// What a folder holds, measured before copying.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TreeSize {
    pub files: u64,
    pub bytes: u64,
}

impl std::ops::AddAssign for TreeSize {
    fn add_assign(&mut self, rhs: Self) {
        self.files += rhs.files;
        self.bytes += rhs.bytes;
    }
}

/// Count files and bytes under `root` (symlinks are not followed).
pub fn measure(root: &Path) -> TreeSize {
    let mut size = TreeSize::default();
    for entry in WalkDir::new(root).follow_links(false).into_iter().flatten() {
        if entry.file_type().is_file() {
            size.files += 1;
            size.bytes += entry.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    size
}

/// How the files were transferred.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CloneReport {
    pub files: u64,
    pub bytes: u64,
    /// Bytes that were cloned (shared with the source, no new disk space).
    pub cloned_bytes: u64,
}

impl std::ops::AddAssign for CloneReport {
    fn add_assign(&mut self, rhs: Self) {
        self.files += rhs.files;
        self.bytes += rhs.bytes;
        self.cloned_bytes += rhs.cloned_bytes;
    }
}

/// True if files can be cloned from `sample` (an existing file in the
/// source) into `dest_dir`. Checked by actually cloning it, which is the only
/// reliable test. Nothing is written next to the source.
pub fn supports_cloning(sample: &Path, dest_dir: &Path) -> bool {
    if fs::create_dir_all(dest_dir).is_err() {
        return false;
    }
    let probe = dest_dir.join(".calcite-clone-probe");
    let _ = fs::remove_file(&probe);
    let ok = reflink_copy::reflink(sample, &probe).is_ok();
    let _ = fs::remove_file(&probe);
    ok
}

/// Free bytes on the volume holding `path` (or its nearest existing parent).
pub fn free_space(path: &Path) -> Option<u64> {
    let mut probe = path.to_path_buf();
    while !probe.exists() {
        probe = probe.parent()?.to_path_buf();
    }
    fs4::available_space(&probe).ok()
}

/// Copy the tree at `src` to `dst` (which must not exist yet), cloning each
/// file where possible. `on_file` is called after every file with the bytes
/// handled so far; return early by setting `cancel`.
pub fn clone_tree(
    src: &Path,
    dst: &Path,
    cancel: &AtomicBool,
    mut on_file: impl FnMut(&CloneReport),
) -> anyhow::Result<CloneReport> {
    if !src.is_dir() {
        bail!("{} is not a folder", src.display());
    }
    if dst.exists() {
        bail!("{} already exists", dst.display());
    }
    let mut report = CloneReport::default();
    for entry in WalkDir::new(src).follow_links(false) {
        if cancel.load(Ordering::Relaxed) {
            bail!("cancelled");
        }
        let entry = entry.with_context(|| format!("reading {}", src.display()))?;
        let relative = entry
            .path()
            .strip_prefix(src)
            .expect("walkdir yields children");
        let target = dst.join(relative);
        let kind = entry.file_type();
        if kind.is_dir() {
            fs::create_dir_all(&target)
                .with_context(|| format!("creating {}", target.display()))?;
        } else if kind.is_symlink() {
            copy_symlink(entry.path(), &target)?;
        } else if kind.is_file() {
            let len = entry.metadata().map(|m| m.len()).unwrap_or(0);
            let cloned = clone_file(entry.path(), &target)?;
            report.files += 1;
            report.bytes += len;
            if cloned {
                report.cloned_bytes += len;
            }
            on_file(&report);
        }
    }
    Ok(report)
}

/// Clone one file, falling back to a normal copy. Returns true if cloned.
fn clone_file(src: &Path, dst: &Path) -> anyhow::Result<bool> {
    match reflink_copy::reflink(src, dst) {
        Ok(()) => Ok(true),
        Err(_) => {
            fs::copy(src, dst)
                .with_context(|| format!("copying {} to {}", src.display(), dst.display()))?;
            Ok(false)
        }
    }
}

fn copy_symlink(src: &Path, dst: &Path) -> anyhow::Result<()> {
    let target: PathBuf = fs::read_link(src)?;
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&target, dst)
            .with_context(|| format!("linking {}", dst.display()))?;
    }
    #[cfg(windows)]
    {
        let result = if src.is_dir() {
            std::os::windows::fs::symlink_dir(&target, dst)
        } else {
            std::os::windows::fs::symlink_file(&target, dst)
        };
        if let Err(error) = result {
            // Symlinks need extra rights on Windows; fall back to a copy.
            if error.kind() == io::ErrorKind::PermissionDenied && src.is_file() {
                fs::copy(src, dst)?;
            } else {
                return Err(error.into());
            }
        }
    }
    Ok(())
}

/// Compare two trees by file count and total size.
pub fn verify(src: &Path, dst: &Path) -> anyhow::Result<()> {
    let (a, b) = (measure(src), measure(dst));
    if a != b {
        bail!(
            "copy is incomplete: {} files / {} bytes expected, found {} files / {} bytes",
            a.files,
            a.bytes,
            b.files,
            b.bytes
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(root: &Path) {
        fs::create_dir_all(root.join("profiles/Survival/mods")).unwrap();
        fs::write(root.join("profiles/Survival/mods/a.jar"), vec![7u8; 4096]).unwrap();
        fs::write(root.join("profiles/Survival/options.txt"), b"fov:90").unwrap();
        fs::create_dir_all(root.join("empty")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("options.txt", root.join("profiles/Survival/link")).unwrap();
    }

    #[test]
    fn clones_a_tree_and_verifies() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");
        tree(&src);
        let cancel = AtomicBool::new(false);
        let mut calls = 0;
        let report = clone_tree(&src, &dst, &cancel, |_| calls += 1).unwrap();
        assert_eq!(report.files, 2);
        assert_eq!(report.bytes, 4096 + 6);
        assert_eq!(calls, 2);
        assert!(dst.join("empty").is_dir());
        assert_eq!(
            fs::read(dst.join("profiles/Survival/options.txt")).unwrap(),
            b"fov:90"
        );
        #[cfg(unix)]
        assert_eq!(
            fs::read_link(dst.join("profiles/Survival/link")).unwrap(),
            PathBuf::from("options.txt")
        );
        verify(&src, &dst).unwrap();
        // Editing the copy never touches the source.
        fs::write(dst.join("profiles/Survival/options.txt"), b"fov:110").unwrap();
        assert_eq!(
            fs::read(src.join("profiles/Survival/options.txt")).unwrap(),
            b"fov:90"
        );
    }

    #[test]
    fn refuses_to_overwrite() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        tree(&src);
        let dst = tmp.path().join("dst");
        fs::create_dir_all(&dst).unwrap();
        let cancel = AtomicBool::new(false);
        assert!(clone_tree(&src, &dst, &cancel, |_| {}).is_err());
    }

    #[test]
    fn cancel_stops_early() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        tree(&src);
        let cancel = AtomicBool::new(true);
        let err = clone_tree(&src, &tmp.path().join("dst"), &cancel, |_| {}).unwrap_err();
        assert_eq!(err.to_string(), "cancelled");
    }

    #[test]
    fn free_space_is_reported() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(free_space(&tmp.path().join("not/yet/created")).unwrap_or(0) > 0);
    }

    #[test]
    fn clone_probe_leaves_nothing_behind() {
        let tmp = tempfile::tempdir().unwrap();
        let sample = tmp.path().join("sample.bin");
        fs::write(&sample, b"x").unwrap();
        let dest = tmp.path().join("dest");
        let _ = supports_cloning(&sample, &dest);
        assert_eq!(fs::read_dir(&dest).unwrap().count(), 0);
        assert!(sample.exists());
    }
}
