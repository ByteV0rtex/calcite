//! Keeping world backups tidy. Backups are zip files named
//! `<time>_<world>.zip` (Minecraft's own naming), newest sorting last.

use std::path::Path;

/// Delete all but the newest `keep` backups of `world` in `backups_dir`.
/// Returns how many were removed. Backups of other worlds are untouched.
pub fn prune_world_backups(backups_dir: &Path, world: &str, keep: usize) -> usize {
    let suffix = format!("_{world}");
    let mut ours: Vec<std::path::PathBuf> = std::fs::read_dir(backups_dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "zip"))
        .filter(|p| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .map(|stem| {
                    // "<time>_<world>" or "<time>_<world> (2)"
                    let base = stem.rsplit_once(" (").map_or(stem, |(b, _)| b);
                    base.ends_with(&suffix)
                })
                .unwrap_or(false)
        })
        .collect();
    ours.sort();
    let excess = ours.len().saturating_sub(keep);
    ours.into_iter()
        .take(excess)
        .filter(|p| std::fs::remove_file(p).is_ok())
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_newest_backups_of_one_world() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        for name in [
            "2026-11-01_10-00-00_Survival.zip",
            "2026-11-02_10-00-00_Survival.zip",
            "2026-11-03_10-00-00_Survival.zip",
            "2026-11-03_10-00-00_Survival (2).zip",
            "2026-11-01_10-00-00_Creative.zip",
            "notes.txt",
        ] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        assert_eq!(prune_world_backups(dir, "Survival", 2), 2);
        let mut left: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        left.sort();
        assert_eq!(
            left,
            vec![
                "2026-11-01_10-00-00_Creative.zip",
                "2026-11-03_10-00-00_Survival (2).zip",
                "2026-11-03_10-00-00_Survival.zip",
                "notes.txt",
            ]
        );
    }
}
