//! 9/IB SMP: one button that gets the server's modpack ready and joins it.
//!
//! The server publishes a small manifest: which pack version it runs, where
//! the `.mrpack` is, and its address. Calcite keeps one instance for it. The
//! first press makes that instance from the pack; when the manifest names a
//! newer pack, the instance is updated in place (worlds, settings, keybinds
//! and the minimap stay). Then the game starts straight into the server.
//!
//! Normal Calcite can switch into this mode from the sidebar. The 9/IB
//! edition is Calcite built with the `ibsmp` feature: it always shows this
//! screen and has its own name and icon, but shares Calcite's data folder,
//! so the 9/IB instance is found by both and never downloaded twice.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail};
use serde::Deserialize;

/// Built as the 9/IB edition (`--features ibsmp`).
pub const EDITION: bool = cfg!(feature = "ibsmp");

/// The window shows the 9/IB screen: always in the edition, and in normal
/// Calcite after "Switch to 9/IB mode".
pub fn active(cx: &gpui_kit::App) -> bool {
    EDITION || cx.global::<crate::prefs::Prefs>().ibsmp_mode
}

/// What the server publishes for the launcher.
pub const MANIFEST_URL: &str = "https://status.bytev0rtex.com/s15/launcher.json";

/// Written into the 9/IB instance's folder after each install (the pack
/// version), so the instance is recognised even if Calcite's own record of
/// it is lost.
pub const MARKER: &str = ".9ib-pack";

/// The pack version recorded in an instance folder, if it is the 9/IB one.
pub async fn read_marker(instance_dir: &Path) -> Option<String> {
    let text = tokio::fs::read_to_string(instance_dir.join(MARKER))
        .await
        .ok()?;
    let version = text.trim();
    (!version.is_empty()).then(|| version.to_string())
}

pub async fn write_marker(instance_dir: &Path, version: &str) -> std::io::Result<()> {
    tokio::fs::write(instance_dir.join(MARKER), format!("{version}\n")).await
}

/// Shown (and pinged) before the manifest has arrived.
pub const DEFAULT_NAME: &str = "9/IB SMP";
pub const DEFAULT_SEASON: &str = "Sezon 1.5";
pub const DEFAULT_ADDRESS: &str = "smp.bytev0rtex.com:25570";

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Manifest {
    /// "9/IB SMP"
    pub name: String,
    /// "Sezon 1.5"
    pub season: String,
    /// Where the game connects, `host[:port]`.
    pub address: String,
    pub pack: Pack,
    /// One line for the launcher ("Cumartesi 21:00: SON SAVAŞ").
    #[serde(default)]
    pub news: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Pack {
    /// Changes whenever the pack does; the instance is updated when it differs.
    pub version: String,
    /// The `.mrpack` file.
    pub url: String,
    /// SHA-1 of the `.mrpack`, so a broken download is never installed.
    pub sha1: String,
}

impl Manifest {
    /// "9/IB SMP Sezon 1.5": the instance's name.
    pub fn title(&self) -> String {
        format!("{} {}", self.name, self.season)
    }
}

fn http() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(calcite_core::user_agent())
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(60))
        .build()?)
}

/// Ask the server which pack it runs. Needs the backend's tokio runtime.
pub async fn fetch_manifest() -> anyhow::Result<Manifest> {
    let response = http()?
        .get(MANIFEST_URL)
        .send()
        .await
        .context("reaching the 9/IB SMP website")?;
    if !response.status().is_success() {
        bail!("the 9/IB SMP website answered {}", response.status());
    }
    let bytes = response.bytes().await?;
    serde_json::from_slice(&bytes).context("reading what the 9/IB SMP website sent")
}

/// Download the pack into `dir` (once per version) and check its SHA-1.
pub async fn download_pack(pack: &Pack, dir: &Path) -> anyhow::Result<PathBuf> {
    let path = dir.join(format!("9ib-{}.mrpack", file_safe(&pack.version)));
    let wanted = pack.sha1.trim().to_ascii_lowercase();
    if let Ok(bytes) = tokio::fs::read(&path).await
        && sha1_hex(&bytes) == wanted
    {
        return Ok(path);
    }
    let bytes = http()?
        .get(&pack.url)
        .send()
        .await
        .context("downloading the 9/IB SMP modpack")?
        .error_for_status()
        .context("downloading the 9/IB SMP modpack")?
        .bytes()
        .await
        .context("downloading the 9/IB SMP modpack")?;
    if sha1_hex(&bytes) != wanted {
        bail!("the 9/IB SMP modpack didn't download correctly (its checksum is wrong); try again");
    }
    tokio::fs::create_dir_all(dir).await?;
    tokio::fs::write(&path, &bytes).await?;
    Ok(path)
}

/// How much memory the 9/IB pack gets: half the computer's, between 3 and
/// 6 GB. The launcher's default (2 GB) is too little for Create, the guns
/// and 40-odd other mods, and the game stutters or runs out of memory.
pub fn recommended_memory_mb(system_mb: u64) -> u32 {
    let half = (system_mb / 2 / 512 * 512) as u32;
    half.clamp(3072, 6144)
}

/// Give the 9/IB instance enough memory, unless someone already chose a
/// value for it in its settings. Needs the backend's tokio runtime.
pub async fn ensure_memory(instance_id: &str) -> anyhow::Result<()> {
    let Some(meta) = theseus::instance::get(instance_id).await? else {
        return Ok(());
    };
    if meta.launch_overrides.memory.is_some() {
        return Ok(());
    }
    let system_mb = theseus::jre::get_max_memory().await.unwrap_or(0) / 1024;
    let maximum = recommended_memory_mb(system_mb);
    let patch = theseus::data::EditInstance {
        launch_overrides: Some(theseus::data::InstanceLaunchOverridesPatch {
            memory: Some(Some(theseus::data::MemorySettings { maximum })),
            ..Default::default()
        }),
        ..Default::default()
    };
    theseus::instance::edit(instance_id, patch).await?;
    tracing::info!("9/IB instance memory set to {maximum} MB");
    Ok(())
}

fn file_safe(version: &str) -> String {
    version
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn sha1_hex(bytes: &[u8]) -> String {
    use sha1::Digest as _;
    sha1::Sha1::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_reads_with_and_without_news() {
        let m: Manifest = serde_json::from_str(
            r#"{"name":"9/IB SMP","season":"Sezon 1.5","address":"smp.bytev0rtex.com:25570",
                "pack":{"version":"0.3-test","url":"https://x/p.mrpack","sha1":"ABC"}}"#,
        )
        .unwrap();
        assert_eq!(m.title(), "9/IB SMP Sezon 1.5");
        assert_eq!(m.news, None);
        assert_eq!(m.pack.version, "0.3-test");
    }

    #[test]
    fn memory_is_half_the_computer_within_limits() {
        assert_eq!(recommended_memory_mb(4096), 3072);
        assert_eq!(recommended_memory_mb(8192), 4096);
        assert_eq!(recommended_memory_mb(8000), 3584);
        assert_eq!(recommended_memory_mb(16384), 6144);
        assert_eq!(recommended_memory_mb(0), 3072);
    }

    #[test]
    fn sha1_and_file_names() {
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(file_safe("0.3 test/../x"), "0.3_test_.._x");
    }
}
