//! Moving in from other launchers.
//!
//! Modrinth App shares Calcite's backend, so its whole data folder can come
//! over: instances, groups, Microsoft accounts, settings and playtime. That
//! happens in two steps because the backend keeps its database open:
//!
//! 1. **Stage** (while Calcite runs): snapshot Modrinth App's database with
//!    `VACUUM INTO`, rewrite every stored path to point at Calcite's folder,
//!    and clone the instance, game-file and cache folders into a staging
//!    folder next to Calcite's data.
//! 2. **Apply** (at the next start, before the backend opens its database):
//!    move the staged folders into place.
//!
//! Modrinth App's folder is only ever read. Other launchers go through the
//! backend's own importers instead (see the app).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use anyhow::{Context as _, bail};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection};
use sqlx::{Connection as _, Row as _};

use crate::clone::{self, CloneReport, TreeSize};

/// Folder name used for staged transfers inside Calcite's data folder.
pub const STAGING_DIR: &str = ".transfer-staging";
const MANIFEST: &str = "transfer.json";

/// The parts of Modrinth App's folder that come over. Launcher logs and
/// lock files stay behind.
const MODRINTH_FOLDERS: [&str; 5] = ["profiles", "meta", "store", "caches", "synced-options"];

/// Everything that can be moved out of the way and replaced on apply.
const REPLACED_ENTRIES: [&str; 8] = [
    "app.db",
    "app.db-wal",
    "app.db-shm",
    "profiles",
    "meta",
    "store",
    "caches",
    "synced-options",
];

/// Where Modrinth App keeps its data on this computer, if it's installed.
pub fn modrinth_app_dir() -> Option<PathBuf> {
    let base = dirs::data_dir()?;
    ["ModrinthApp", "com.modrinth.theseus"]
        .into_iter()
        .map(|name| base.join(name))
        .find(|dir| dir.join("app.db").is_file())
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModrinthPreview {
    pub dir: PathBuf,
    /// Where its instances and game files actually live (it can be moved).
    pub content_dir: PathBuf,
    pub instances: u32,
    pub accounts: u32,
    pub schema_version: i64,
    pub size: TreeSize,
}

impl ModrinthPreview {
    /// The database is from a Modrinth App version Calcite understands.
    pub fn schema_supported(&self, calcite_schema: i64) -> bool {
        self.schema_version <= calcite_schema
    }
}

async fn open_read_only(db: &Path) -> anyhow::Result<SqliteConnection> {
    let options = SqliteConnectOptions::new()
        .filename(db)
        .read_only(true)
        .immutable(false);
    SqliteConnection::connect_with(&options)
        .await
        .with_context(|| format!("opening {}", db.display()))
}

/// Look at Modrinth App's data without changing anything.
pub async fn preview_modrinth(dir: &Path) -> anyhow::Result<ModrinthPreview> {
    let mut db = open_read_only(&dir.join("app.db")).await?;
    let schema_version: i64 = sqlx::query("SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations")
        .fetch_one(&mut db)
        .await?
        .get(0);
    let instances: i64 = sqlx::query("SELECT COUNT(*) FROM instances")
        .fetch_one(&mut db)
        .await?
        .get(0);
    let accounts: i64 = sqlx::query("SELECT COUNT(*) FROM minecraft_users")
        .fetch_one(&mut db)
        .await
        .map(|r| r.get(0))
        .unwrap_or(0);
    let custom_dir: Option<String> = sqlx::query("SELECT custom_dir FROM settings LIMIT 1")
        .fetch_optional(&mut db)
        .await
        .ok()
        .flatten()
        .and_then(|r| r.get(0));
    db.close().await.ok();

    let content_dir = custom_dir
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(|| dir.to_path_buf());
    let dir_owned = content_dir.clone();
    let size = tokio::task::spawn_blocking(move || {
        let mut total = TreeSize::default();
        for folder in MODRINTH_FOLDERS {
            total += clone::measure(&dir_owned.join(folder));
        }
        total
    })
    .await?;

    Ok(ModrinthPreview {
        dir: dir.to_path_buf(),
        content_dir,
        instances: instances as u32,
        accounts: accounts as u32,
        schema_version,
        size,
    })
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub source: String,
    pub source_dir: PathBuf,
    pub created: chrono::DateTime<chrono::Utc>,
    pub instances: u32,
    pub accounts: u32,
    pub files: u64,
    pub bytes: u64,
    pub cloned_bytes: u64,
}

/// Progress updates while staging.
#[derive(Clone, Debug, PartialEq)]
pub enum Progress {
    Snapshotting,
    Copying { done: CloneReport, total: TreeSize },
    Verifying,
}

/// Stage a full transfer from Modrinth App into `calcite_dir/.transfer-staging`.
pub async fn stage_modrinth(
    preview: &ModrinthPreview,
    calcite_dir: &Path,
    calcite_schema: i64,
    cancel: Arc<AtomicBool>,
    progress: impl Fn(Progress) + Send + Sync + 'static,
) -> anyhow::Result<Manifest> {
    if !preview.schema_supported(calcite_schema) {
        bail!(
            "Modrinth App has been updated past what this Calcite understands. Update Calcite, then try again."
        );
    }
    let staging = calcite_dir.join(STAGING_DIR);
    if staging.exists() {
        tokio::fs::remove_dir_all(&staging).await?;
    }
    tokio::fs::create_dir_all(&staging).await?;

    // Room check: cloning needs almost nothing; a real copy needs it all.
    let sample = preview.dir.join("app.db");
    let staging_for_probe = staging.clone();
    let can_clone =
        tokio::task::spawn_blocking(move || clone::supports_cloning(&sample, &staging_for_probe))
            .await?;
    if !can_clone {
        let free = clone::free_space(&staging).unwrap_or(u64::MAX);
        let needed = preview.size.bytes + preview.size.bytes / 20;
        if free < needed {
            tokio::fs::remove_dir_all(&staging).await.ok();
            bail!(
                "This drive can't share files between folders, so moving in would need {} free and only {} is available.",
                human_bytes(needed),
                human_bytes(free)
            );
        }
    }

    // 1. Consistent snapshot of the database, even if Modrinth App is open.
    progress(Progress::Snapshotting);
    let snapshot = staging.join("app.db");
    {
        let mut db = open_read_only(&preview.dir.join("app.db")).await?;
        sqlx::query("VACUUM INTO ?")
            .bind(snapshot.to_string_lossy().to_string())
            .execute(&mut db)
            .await
            .context("snapshotting Modrinth App's database")?;
        db.close().await.ok();
    }
    rewrite_paths(&snapshot, &preview.content_dir, calcite_dir).await?;

    // 2. Clone the folders.
    let total = preview.size;
    let content_dir = preview.content_dir.clone();
    let staging_dir = staging.clone();
    let progress = Arc::new(progress);
    let progress_for_copy = progress.clone();
    let report = tokio::task::spawn_blocking(move || -> anyhow::Result<CloneReport> {
        let mut report = CloneReport::default();
        let mut last_emit = std::time::Instant::now();
        for folder in MODRINTH_FOLDERS {
            let src = content_dir.join(folder);
            if !src.is_dir() {
                continue;
            }
            let base = report;
            let part = clone::clone_tree(&src, &staging_dir.join(folder), &cancel, |done| {
                if last_emit.elapsed().as_millis() >= 100 {
                    last_emit = std::time::Instant::now();
                    let mut sum = base;
                    sum += *done;
                    progress_for_copy(Progress::Copying { done: sum, total });
                }
            })?;
            report += part;
        }
        // The store's scratch area is temporary; don't bring it.
        let _ = std::fs::remove_dir_all(staging_dir.join("store").join("staging"));
        Ok(report)
    })
    .await??;

    progress(Progress::Verifying);
    let content_dir = preview.content_dir.clone();
    let staging_dir = staging.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        for folder in ["profiles", "meta"] {
            let src = content_dir.join(folder);
            if src.is_dir() {
                clone::verify(&src, &staging_dir.join(folder))?;
            }
        }
        Ok(())
    })
    .await??;

    let manifest = Manifest {
        source: "Modrinth App".into(),
        source_dir: preview.dir.clone(),
        created: chrono::Utc::now(),
        instances: preview.instances,
        accounts: preview.accounts,
        files: report.files,
        bytes: report.bytes,
        cloned_bytes: report.cloned_bytes,
    };
    tokio::fs::write(
        staging.join(MANIFEST),
        serde_json::to_vec_pretty(&manifest)?,
    )
    .await?;
    Ok(manifest)
}

/// Point every stored path at Calcite's folder, and drop records that only
/// make sense inside Modrinth App (running processes, its install history).
async fn rewrite_paths(db: &Path, old_root: &Path, new_root: &Path) -> anyhow::Result<()> {
    let options = SqliteConnectOptions::new().filename(db);
    let mut conn = SqliteConnection::connect_with(&options).await?;
    let old = old_root
        .to_string_lossy()
        .trim_end_matches(['/', '\\'])
        .to_string();
    let new = new_root
        .to_string_lossy()
        .trim_end_matches(['/', '\\'])
        .to_string();
    // JSON escapes backslashes, so Windows paths appear doubled inside JSON.
    let old_json = old.replace('\\', "\\\\");
    let new_json = new.replace('\\', "\\\\");

    // The folder is Calcite's own now; no custom location.
    sqlx::query("UPDATE settings SET custom_dir = NULL, prev_custom_dir = NULL")
        .execute(&mut conn)
        .await?;
    for table in ["processes", "install_jobs", "install_job_requests"] {
        // Fixed table names from this file, not user input.
        let _ = sqlx::query(sqlx::AssertSqlSafe(format!("DELETE FROM \"{table}\"")))
            .execute(&mut conn)
            .await;
    }

    let tables: Vec<String> = sqlx::query(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name != '_sqlx_migrations'",
    )
    .fetch_all(&mut conn)
    .await?
    .into_iter()
    .map(|r| r.get(0))
    .collect();

    let pattern = format!("%{old}%");
    let pattern_json = format!("%{old_json}%");
    for table in tables {
        // Names come from the database's own schema; skip anything that
        // couldn't be quoted safely.
        if !is_plain_identifier(&table) {
            continue;
        }
        let columns: Vec<(String, String)> = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT name, type FROM pragma_table_info('{table}')"
        )))
        .fetch_all(&mut conn)
        .await?
        .into_iter()
        .map(|r| (r.get(0), r.get::<String, _>(1).to_ascii_uppercase()))
        .collect();
        for (column, kind) in columns {
            if !is_plain_identifier(&column) {
                continue;
            }
            let (sql, from, to, like) = match kind.as_str() {
                "TEXT" | "" => (
                    format!(
                        "UPDATE \"{table}\" SET \"{column}\" = replace(\"{column}\", ?1, ?2) WHERE \"{column}\" LIKE ?3"
                    ),
                    &old,
                    &new,
                    &pattern,
                ),
                "JSONB" | "JSON" => (
                    format!(
                        "UPDATE \"{table}\" SET \"{column}\" = jsonb(replace(json(\"{column}\"), ?1, ?2)) WHERE json(\"{column}\") LIKE ?3"
                    ),
                    &old_json,
                    &new_json,
                    &pattern_json,
                ),
                _ => continue,
            };
            sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(from)
                .bind(to)
                .bind(like)
                .execute(&mut conn)
                .await
                .with_context(|| format!("updating paths in {table}.{column}"))?;
        }
    }
    conn.close().await.ok();
    Ok(())
}

fn is_plain_identifier(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A staged transfer waiting to be applied, if any.
pub fn staged(calcite_dir: &Path) -> Option<Manifest> {
    let bytes = std::fs::read(calcite_dir.join(STAGING_DIR).join(MANIFEST)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Move a staged transfer into place. Must run before the backend starts.
/// Whatever Calcite had before is kept in `.before-transfer-<time>`.
pub fn apply_staged(calcite_dir: &Path) -> anyhow::Result<Option<Manifest>> {
    let staging = calcite_dir.join(STAGING_DIR);
    let Some(manifest) = staged(calcite_dir) else {
        // A staging folder without a manifest is an unfinished attempt.
        if staging.exists() {
            std::fs::remove_dir_all(&staging).ok();
        }
        return Ok(None);
    };
    let backup = calcite_dir.join(format!(
        ".before-transfer-{}",
        chrono::Utc::now().format("%Y%m%d-%H%M%S")
    ));
    for entry in REPLACED_ENTRIES {
        let current = calcite_dir.join(entry);
        let incoming = staging.join(entry);
        if !incoming.exists() && !entry.starts_with("app.db") {
            continue;
        }
        if current.exists() {
            std::fs::create_dir_all(&backup)?;
            std::fs::rename(&current, backup.join(entry))
                .with_context(|| format!("setting aside {}", current.display()))?;
        }
        if incoming.exists() {
            std::fs::rename(&incoming, &current)
                .with_context(|| format!("moving {} into place", incoming.display()))?;
        }
    }
    std::fs::remove_dir_all(&staging).ok();
    Ok(Some(manifest))
}

pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Build a miniature Modrinth App folder with a real SQLite database.
    async fn fake_modrinth(root: &Path, schema: i64) {
        fs::create_dir_all(root.join("profiles/Survival/mods")).unwrap();
        fs::write(root.join("profiles/Survival/mods/sodium.jar"), b"jar").unwrap();
        fs::create_dir_all(root.join("meta/versions")).unwrap();
        fs::write(root.join("meta/versions/26.3.json"), b"{}").unwrap();
        fs::create_dir_all(root.join("caches/icons")).unwrap();
        fs::write(root.join("caches/icons/a.png"), b"png").unwrap();
        fs::create_dir_all(root.join("launcher_logs")).unwrap();
        fs::write(root.join("launcher_logs/big.log"), b"noise").unwrap();

        let options = SqliteConnectOptions::new()
            .filename(root.join("app.db"))
            .create_if_missing(true);
        let mut db = SqliteConnection::connect_with(&options).await.unwrap();
        let root_s = root.to_string_lossy().to_string();
        for sql in [
            "CREATE TABLE _sqlx_migrations (version BIGINT PRIMARY KEY)".to_string(),
            format!("INSERT INTO _sqlx_migrations VALUES ({schema})"),
            "CREATE TABLE settings (custom_dir TEXT, prev_custom_dir TEXT)".into(),
            format!("INSERT INTO settings VALUES ('{root_s}', '{root_s}')"),
            "CREATE TABLE instances (id TEXT, path TEXT, icon_path TEXT)".into(),
            format!(
                "INSERT INTO instances VALUES ('legacy:Survival', 'Survival', '{root_s}/caches/icons/a.png')"
            ),
            "CREATE TABLE minecraft_users (uuid TEXT)".into(),
            "INSERT INTO minecraft_users VALUES ('u1')".into(),
            "CREATE TABLE cache (data JSONB)".into(),
            format!(
                "INSERT INTO cache VALUES (jsonb('{{\"icon_path\":\"{root_s}/caches/icons/a.png\"}}'))"
            ),
            "CREATE TABLE processes (pid INTEGER)".into(),
            "INSERT INTO processes VALUES (42)".into(),
        ] {
            sqlx::query(sqlx::AssertSqlSafe(sql))
                .execute(&mut db)
                .await
                .unwrap();
        }
        db.close().await.unwrap();
    }

    #[tokio::test]
    async fn full_transfer_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("ModrinthApp");
        let calcite = tmp.path().join("Calcite");
        fs::create_dir_all(&calcite).unwrap();
        fake_modrinth(&source, 20261022120000).await;

        let preview = preview_modrinth(&source).await.unwrap();
        assert_eq!(preview.instances, 1);
        assert_eq!(preview.accounts, 1);
        assert!(preview.schema_supported(20261102120000));

        let cancel = Arc::new(AtomicBool::new(false));
        let manifest = stage_modrinth(&preview, &calcite, 20261102120000, cancel, |_| {})
            .await
            .unwrap();
        assert_eq!(manifest.instances, 1);
        assert!(staged(&calcite).is_some());

        // Paths now point at Calcite; Modrinth-only records are gone.
        let mut db = SqliteConnection::connect_with(
            &SqliteConnectOptions::new().filename(calcite.join(STAGING_DIR).join("app.db")),
        )
        .await
        .unwrap();
        let icon: String = sqlx::query("SELECT icon_path FROM instances")
            .fetch_one(&mut db)
            .await
            .unwrap()
            .get(0);
        assert!(
            icon.starts_with(&calcite.to_string_lossy().to_string()),
            "{icon}"
        );
        let custom: Option<String> = sqlx::query("SELECT custom_dir FROM settings")
            .fetch_one(&mut db)
            .await
            .unwrap()
            .get(0);
        assert_eq!(custom, None);
        let cached: String = sqlx::query("SELECT json(data) FROM cache")
            .fetch_one(&mut db)
            .await
            .unwrap()
            .get(0);
        assert!(
            cached.contains(&calcite.to_string_lossy().to_string()),
            "{cached}"
        );
        let processes: i64 = sqlx::query("SELECT COUNT(*) FROM processes")
            .fetch_one(&mut db)
            .await
            .unwrap()
            .get(0);
        assert_eq!(processes, 0);
        db.close().await.unwrap();

        // Apply moves everything into place and keeps what was there before.
        fs::write(calcite.join("app.db"), b"old").unwrap();
        let applied = apply_staged(&calcite).unwrap().unwrap();
        assert_eq!(applied.source, "Modrinth App");
        assert!(calcite.join("profiles/Survival/mods/sodium.jar").is_file());
        assert!(!calcite.join("launcher_logs").exists());
        assert!(!calcite.join(STAGING_DIR).exists());
        let backups: Vec<_> = fs::read_dir(&calcite)
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(".before-transfer-")
            })
            .collect();
        assert_eq!(backups.len(), 1);

        // The source is untouched.
        assert!(source.join("profiles/Survival/mods/sodium.jar").is_file());
        assert!(source.join("launcher_logs/big.log").is_file());
        let mut src_db = open_read_only(&source.join("app.db")).await.unwrap();
        let src_custom: Option<String> = sqlx::query("SELECT custom_dir FROM settings")
            .fetch_one(&mut src_db)
            .await
            .unwrap()
            .get(0);
        assert_eq!(
            src_custom.as_deref(),
            Some(source.to_string_lossy().as_ref())
        );
    }

    #[tokio::test]
    async fn refuses_newer_modrinth_databases() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("ModrinthApp");
        fake_modrinth(&source, 20300101000000).await;
        let preview = preview_modrinth(&source).await.unwrap();
        assert!(!preview.schema_supported(20261102120000));
        let cancel = Arc::new(AtomicBool::new(false));
        let err = stage_modrinth(
            &preview,
            &tmp.path().join("Calcite"),
            20261102120000,
            cancel,
            |_| {},
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("Update Calcite"));
    }

    #[test]
    fn bytes_read_naturally() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(10 * 1024 * 1024 * 1024), "10.0 GB");
    }
}
