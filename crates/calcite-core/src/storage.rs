//! Disk usage of Calcite's folder and the old files that are safe to delete.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use walkdir::WalkDir;

/// Total size of the files under `path` (links aren't followed).
pub fn dir_size(path: &Path) -> u64 {
    WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

/// Where the space goes, by kind.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Breakdown {
    /// Instance folder name and its size, biggest first.
    pub instances: Vec<(String, u64)>,
    /// Game versions, libraries, assets and Java runtimes.
    pub game_files: u64,
    /// The shared mod store.
    pub store: u64,
    /// Snapshots and world backups Calcite keeps.
    pub snapshots: u64,
    /// Icon, thumbnail and language caches, the database and the rest.
    pub other: u64,
}

impl Breakdown {
    pub fn instances_total(&self) -> u64 {
        self.instances.iter().map(|(_, b)| b).sum()
    }
    pub fn total(&self) -> u64 {
        self.instances_total() + self.game_files + self.store + self.snapshots + self.other
    }
}

pub fn breakdown(data_dir: &Path) -> Breakdown {
    let mut out = Breakdown::default();
    let Ok(entries) = std::fs::read_dir(data_dir) else {
        return out;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let size = || {
            if path.is_dir() {
                dir_size(&path)
            } else {
                entry.metadata().map(|m| m.len()).unwrap_or(0)
            }
        };
        match name.as_str() {
            "profiles" => {
                if let Ok(instances) = std::fs::read_dir(&path) {
                    for inst in instances.filter_map(Result::ok) {
                        if inst.path().is_dir() {
                            let name = inst.file_name().to_string_lossy().into_owned();
                            out.instances.push((name, dir_size(&inst.path())));
                        }
                    }
                }
            }
            "meta" => out.game_files += size(),
            "store" => out.store += size(),
            "calcite-snapshots" => out.snapshots += size(),
            _ => out.other += size(),
        }
    }
    out.instances
        .sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out
}

/// Files that can go without losing anything that matters.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OldFiles {
    pub files: Vec<PathBuf>,
    pub bytes: u64,
}

/// Logs older than two weeks (never `latest.log`) and crash reports older
/// than a month, across every instance in `profiles_dir`.
pub fn old_logs(profiles_dir: &Path, now: SystemTime) -> OldFiles {
    const LOG_AGE: Duration = Duration::from_secs(14 * 24 * 3600);
    const CRASH_AGE: Duration = Duration::from_secs(30 * 24 * 3600);
    let mut out = OldFiles::default();
    let Ok(instances) = std::fs::read_dir(profiles_dir) else {
        return out;
    };
    for inst in instances.filter_map(Result::ok) {
        for (folder, age) in [("logs", LOG_AGE), ("crash-reports", CRASH_AGE)] {
            let dir = inst.path().join(folder);
            for entry in WalkDir::new(&dir)
                .max_depth(1)
                .into_iter()
                .filter_map(Result::ok)
            {
                if !entry.file_type().is_file() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy();
                let is_log =
                    name.ends_with(".log") || name.ends_with(".log.gz") || name.ends_with(".txt");
                if !is_log || name == "latest.log" {
                    continue;
                }
                let Ok(meta) = entry.metadata() else { continue };
                let old = meta
                    .modified()
                    .ok()
                    .and_then(|m| now.duration_since(m).ok())
                    .is_some_and(|d| d > age);
                if old {
                    out.bytes += meta.len();
                    out.files.push(entry.into_path());
                }
            }
        }
    }
    out
}

/// Delete `files`; returns the bytes freed.
pub fn remove(files: &[PathBuf]) -> u64 {
    files
        .iter()
        .filter_map(|f| {
            let len = std::fs::metadata(f).ok()?.len();
            std::fs::remove_file(f).ok().map(|_| len)
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn age(path: &Path, days: u64) {
        let t = SystemTime::now() - Duration::from_secs(days * 24 * 3600);
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(t)
            .unwrap();
    }

    #[test]
    fn finds_only_old_logs_and_crash_reports() {
        let root = tempfile::tempdir().unwrap();
        let profiles = root.path().join("profiles");
        let logs = profiles.join("A").join("logs");
        let crashes = profiles.join("A").join("crash-reports");
        fs::create_dir_all(&logs).unwrap();
        fs::create_dir_all(&crashes).unwrap();
        for (path, days) in [
            (logs.join("latest.log"), 100),
            (logs.join("2024-01-01-1.log.gz"), 100),
            (logs.join("2026-11-14-1.log.gz"), 1),
            (crashes.join("crash-old.txt"), 40),
            (crashes.join("crash-new.txt"), 20),
        ] {
            fs::write(&path, b"0123456789").unwrap();
            age(&path, days);
        }
        let mut found = old_logs(&profiles, SystemTime::now());
        found.files.sort();
        let names: Vec<String> = found
            .files
            .iter()
            .map(|f| f.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["crash-old.txt", "2024-01-01-1.log.gz"]);
        assert_eq!(found.bytes, 20);
        assert_eq!(remove(&found.files), 20);
        assert!(logs.join("latest.log").exists());
    }

    #[test]
    fn breaks_usage_down_by_kind() {
        let root = tempfile::tempdir().unwrap();
        let d = root.path();
        fs::create_dir_all(d.join("profiles/Big/mods")).unwrap();
        fs::create_dir_all(d.join("profiles/Small")).unwrap();
        fs::create_dir_all(d.join("meta/versions")).unwrap();
        fs::write(d.join("profiles/Big/mods/a.jar"), vec![0u8; 300]).unwrap();
        fs::write(d.join("profiles/Small/options.txt"), vec![0u8; 10]).unwrap();
        fs::write(d.join("meta/versions/1.jar"), vec![0u8; 50]).unwrap();
        fs::write(d.join("app.db"), vec![0u8; 7]).unwrap();
        let b = breakdown(d);
        assert_eq!(b.instances, vec![("Big".into(), 300), ("Small".into(), 10)]);
        assert_eq!((b.game_files, b.store, b.other), (50, 0, 7));
        assert_eq!(b.total(), 367);
    }
}
