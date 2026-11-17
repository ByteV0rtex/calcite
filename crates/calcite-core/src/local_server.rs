//! Minecraft servers that run on this computer: make one (Vanilla, Fabric
//! or Paper), keep its settings, and accept Mojang's EULA when the player
//! says so. Starting and stopping the process is the app's job.
//!
//! Each server is a folder under `<data>/servers/` with the server jar,
//! the world, and `calcite-server.json`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context as _, anyhow, bail};
use serde::{Deserialize, Serialize};

const MOJANG_MANIFEST: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const FABRIC_META: &str = "https://meta.fabricmc.net/v2";
const PAPER_FILL: &str = "https://fill.papermc.io/v3/projects/paper";
const CONFIG: &str = "calcite-server.json";
/// The jar every server starts from, whatever made it.
pub const JAR: &str = "server.jar";
pub const EULA_URL: &str = "https://aka.ms/MinecraftEULA";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Vanilla,
    Fabric,
    Paper,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Vanilla, Kind::Fabric, Kind::Paper];

    pub fn label(self) -> &'static str {
        match self {
            Kind::Vanilla => "Vanilla",
            Kind::Fabric => "Fabric",
            Kind::Paper => "Paper",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            Kind::Vanilla => "Plain Minecraft, exactly as Mojang ships it.",
            Kind::Fabric => "Runs Fabric mods, like the ones in your instances.",
            Kind::Paper => "Faster, and runs Bukkit and Paper plugins.",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub name: String,
    pub kind: Kind,
    pub game_version: String,
    #[serde(default)]
    pub loader_version: Option<String>,
    /// Java major version the game needs.
    pub java: u32,
    pub memory_mb: u32,
    pub port: u16,
    pub created: i64,
}

/// A server folder and what's in its config.
#[derive(Clone, Debug, PartialEq)]
pub struct LocalServer {
    pub dir: PathBuf,
    pub config: Config,
}

impl LocalServer {
    pub fn eula_accepted(&self) -> bool {
        std::fs::read_to_string(self.dir.join("eula.txt"))
            .map(|text| {
                text.lines()
                    .any(|l| l.trim().eq_ignore_ascii_case("eula=true"))
            })
            .unwrap_or(false)
    }

    /// Only ever called after the player ticked "I agree" in Calcite.
    pub fn accept_eula(&self) -> anyhow::Result<()> {
        std::fs::write(
            self.dir.join("eula.txt"),
            format!(
                "# Accepted in Calcite on {}. See {EULA_URL}\neula=true\n",
                chrono::Utc::now().format("%Y-%m-%d")
            ),
        )?;
        Ok(())
    }

    pub fn save(&self) -> anyhow::Result<()> {
        write_config(&self.dir, &self.config)
    }

    /// The address to join it from this computer.
    pub fn address(&self) -> String {
        if self.config.port == 25565 {
            "localhost".into()
        } else {
            format!("localhost:{}", self.config.port)
        }
    }

    /// Java arguments to start it (after the java binary itself).
    pub fn java_args(&self) -> Vec<String> {
        vec![
            format!("-Xms{}M", (self.config.memory_mb / 2).max(512)),
            format!("-Xmx{}M", self.config.memory_mb),
            "-jar".into(),
            JAR.into(),
            "nogui".into(),
        ]
    }

    /// Set `server-port` (and keep everything else in server.properties).
    pub fn set_port(&mut self, port: u16) -> anyhow::Result<()> {
        self.config.port = port;
        set_property(&self.dir, "server-port", &port.to_string())?;
        self.save()
    }
}

pub fn root(data_dir: &Path) -> PathBuf {
    data_dir.join("servers")
}

/// Every server under `root`, newest first. Folders without a readable
/// config are skipped.
pub fn list(root: &Path) -> Vec<LocalServer> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut servers: Vec<LocalServer> = entries
        .flatten()
        .filter_map(|e| {
            let dir = e.path();
            let text = std::fs::read_to_string(dir.join(CONFIG)).ok()?;
            let config = serde_json::from_str(&text).ok()?;
            Some(LocalServer { dir, config })
        })
        .collect();
    servers.sort_by_key(|s| std::cmp::Reverse(s.config.created));
    servers
}

/// Release versions, newest first.
pub async fn game_versions(http: &reqwest::Client) -> anyhow::Result<Vec<String>> {
    let manifest: Manifest = http
        .get(MOJANG_MANIFEST)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(manifest
        .versions
        .into_iter()
        .filter(|v| v.kind == "release")
        .map(|v| v.id)
        .collect())
}

/// What to make.
#[derive(Clone, Debug)]
pub struct NewServer {
    pub name: String,
    pub kind: Kind,
    pub game_version: String,
    pub memory_mb: u32,
}

/// Download everything and set the folder up. `progress` hears short
/// status lines. The EULA is left for the player to accept.
pub async fn create(
    http: &reqwest::Client,
    root: &Path,
    new: NewServer,
    progress: impl Fn(&str),
) -> anyhow::Result<LocalServer> {
    let name = new.name.trim();
    if name.is_empty() {
        bail!("Give the server a name.");
    }
    progress("Looking up the game version");
    let version = mojang_version(http, &new.game_version).await?;
    let java = version.java_version.map(|j| j.major_version).unwrap_or(8);

    std::fs::create_dir_all(root)?;
    let dir = free_dir(root, name);
    std::fs::create_dir_all(&dir)?;
    let result = async {
        let jar = dir.join(JAR);
        let loader_version = match new.kind {
            Kind::Vanilla => {
                let server = version
                    .downloads
                    .server
                    .ok_or_else(|| anyhow!("Mojang has no server for {}.", new.game_version))?;
                progress("Downloading the server");
                let bytes = download(http, &server.url).await?;
                check_sha1(&bytes, &server.sha1)?;
                std::fs::write(&jar, bytes)?;
                None
            }
            Kind::Fabric => {
                progress("Finding the newest Fabric");
                let loader = fabric_loader(http, &new.game_version).await?;
                let installer = fabric_installer(http).await?;
                progress("Downloading Fabric");
                let url = format!(
                    "{FABRIC_META}/versions/loader/{}/{loader}/{installer}/server/jar",
                    new.game_version
                );
                let bytes = download(http, &url).await?;
                if !bytes.starts_with(b"PK") {
                    bail!("Fabric sent something that isn't a jar.");
                }
                std::fs::write(&jar, bytes)?;
                Some(loader)
            }
            Kind::Paper => {
                progress("Finding the newest Paper build");
                let build: PaperBuild = http
                    .get(format!(
                        "{PAPER_FILL}/versions/{}/builds/latest",
                        new.game_version
                    ))
                    .send()
                    .await?
                    .error_for_status()
                    .map_err(|_| anyhow!("Paper isn't out for {} yet.", new.game_version))?
                    .json()
                    .await?;
                let file = build
                    .downloads
                    .get("server:default")
                    .ok_or_else(|| anyhow!("Paper's build {} has no server jar.", build.id))?;
                progress("Downloading Paper");
                let bytes = download(http, &file.url).await?;
                check_sha256(&bytes, &file.checksums.sha256)?;
                std::fs::write(&jar, bytes)?;
                Some(build.id.to_string())
            }
        };
        let port = 25565;
        std::fs::write(
            dir.join("server.properties"),
            format!(
                "server-port={port}\nmotd={}\nmax-players=10\nspawn-protection=0\n",
                name.replace(['\n', '\r'], " ")
            ),
        )?;
        let server = LocalServer {
            dir: dir.clone(),
            config: Config {
                name: name.to_string(),
                kind: new.kind,
                game_version: new.game_version.clone(),
                loader_version,
                java,
                memory_mb: new.memory_mb.max(512),
                port,
                created: chrono::Utc::now().timestamp(),
            },
        };
        server.save()?;
        anyhow::Ok(server)
    }
    .await;
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    result
}

/// Change one `key=value` line in server.properties, adding it if missing.
pub fn set_property(dir: &Path, key: &str, value: &str) -> anyhow::Result<()> {
    let path = dir.join("server.properties");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let mut found = false;
    let mut lines: Vec<String> = text
        .lines()
        .map(|line| match line.split_once('=') {
            Some((k, _)) if k.trim() == key => {
                found = true;
                format!("{key}={value}")
            }
            _ => line.to_string(),
        })
        .collect();
    if !found {
        lines.push(format!("{key}={value}"));
    }
    std::fs::write(path, lines.join("\n") + "\n")?;
    Ok(())
}

fn write_config(dir: &Path, config: &Config) -> anyhow::Result<()> {
    std::fs::write(dir.join(CONFIG), serde_json::to_string_pretty(config)?)?;
    Ok(())
}

/// `root/name`, or `root/name (2)` and so on if that's taken. Characters
/// that can't be in a folder name become `-`.
fn free_dir(root: &Path, name: &str) -> PathBuf {
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_control() || r#"/\:*?"<>|"#.contains(c) {
                '-'
            } else {
                c
            }
        })
        .collect();
    let safe = safe.trim_matches(['.', ' ']).to_string();
    let safe = if safe.is_empty() {
        "Server".into()
    } else {
        safe
    };
    let mut dir = root.join(&safe);
    let mut n = 2;
    while dir.exists() {
        dir = root.join(format!("{safe} ({n})"));
        n += 1;
    }
    dir
}

async fn download(http: &reqwest::Client, url: &str) -> anyhow::Result<Vec<u8>> {
    let response = http
        .get(url)
        .timeout(Duration::from_secs(600))
        .send()
        .await?
        .error_for_status()?;
    Ok(response.bytes().await?.to_vec())
}

fn check_sha1(bytes: &[u8], expected: &str) -> anyhow::Result<()> {
    use sha1::Digest as _;
    let got = hex(&sha1::Sha1::digest(bytes));
    if !got.eq_ignore_ascii_case(expected) {
        bail!("The download was damaged (checksum mismatch). Try again.");
    }
    Ok(())
}

fn check_sha256(bytes: &[u8], expected: &str) -> anyhow::Result<()> {
    use sha2::Digest as _;
    let got = hex(&sha2::Sha256::digest(bytes));
    if !got.eq_ignore_ascii_case(expected) {
        bail!("The download was damaged (checksum mismatch). Try again.");
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

async fn mojang_version(http: &reqwest::Client, id: &str) -> anyhow::Result<VersionJson> {
    let manifest: Manifest = http
        .get(MOJANG_MANIFEST)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let entry = manifest
        .versions
        .into_iter()
        .find(|v| v.id == id)
        .ok_or_else(|| anyhow!("Minecraft {id} doesn't exist."))?;
    http.get(&entry.url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
        .context("reading the version details")
}

async fn fabric_loader(http: &reqwest::Client, game: &str) -> anyhow::Result<String> {
    let loaders: Vec<FabricLoaderEntry> = http
        .get(format!("{FABRIC_META}/versions/loader/{game}"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    loaders
        .iter()
        .find(|l| l.loader.stable)
        .or(loaders.first())
        .map(|l| l.loader.version.clone())
        .ok_or_else(|| anyhow!("Fabric doesn't support {game} yet."))
}

async fn fabric_installer(http: &reqwest::Client) -> anyhow::Result<String> {
    let installers: Vec<FabricVersion> = http
        .get(format!("{FABRIC_META}/versions/installer"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    installers
        .iter()
        .find(|i| i.stable)
        .or(installers.first())
        .map(|i| i.version.clone())
        .ok_or_else(|| anyhow!("Couldn't find Fabric's installer."))
}

#[derive(Deserialize)]
struct Manifest {
    versions: Vec<ManifestEntry>,
}

#[derive(Deserialize)]
struct ManifestEntry {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    url: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VersionJson {
    downloads: VersionDownloads,
    java_version: Option<JavaVersion>,
}

#[derive(Deserialize)]
struct VersionDownloads {
    server: Option<Download>,
}

#[derive(Deserialize)]
struct Download {
    sha1: String,
    url: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JavaVersion {
    major_version: u32,
}

#[derive(Deserialize)]
struct FabricLoaderEntry {
    loader: FabricVersion,
}

#[derive(Deserialize)]
struct FabricVersion {
    version: String,
    stable: bool,
}

#[derive(Deserialize)]
struct PaperBuild {
    id: u64,
    downloads: std::collections::HashMap<String, PaperFile>,
}

#[derive(Deserialize)]
struct PaperFile {
    url: String,
    checksums: PaperChecksums,
}

#[derive(Deserialize)]
struct PaperChecksums {
    sha256: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(dir: &Path) -> LocalServer {
        LocalServer {
            dir: dir.to_path_buf(),
            config: Config {
                name: "Test".into(),
                kind: Kind::Vanilla,
                game_version: "1.21.4".into(),
                loader_version: None,
                java: 21,
                memory_mb: 2048,
                port: 25565,
                created: 1,
            },
        }
    }

    #[test]
    fn eula_is_off_until_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let s = server(dir.path());
        assert!(!s.eula_accepted());
        std::fs::write(dir.path().join("eula.txt"), "eula=false\n").unwrap();
        assert!(!s.eula_accepted());
        s.accept_eula().unwrap();
        assert!(s.eula_accepted());
    }

    #[test]
    fn properties_change_in_place() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("server.properties"),
            "#comment\nserver-port=25565\nmotd=Hi\n",
        )
        .unwrap();
        let mut s = server(dir.path());
        s.set_port(25570).unwrap();
        set_property(dir.path(), "pvp", "false").unwrap();
        let text = std::fs::read_to_string(dir.path().join("server.properties")).unwrap();
        assert_eq!(text, "#comment\nserver-port=25570\nmotd=Hi\npvp=false\n");
        assert_eq!(s.address(), "localhost:25570");
    }

    #[test]
    fn folders_are_listed_newest_first_and_names_stay_safe() {
        let root = tempfile::tempdir().unwrap();
        for (name, created) in [("Old", 1), ("New", 5)] {
            let dir = free_dir(root.path(), name);
            std::fs::create_dir_all(&dir).unwrap();
            let mut s = server(&dir);
            s.config.name = name.into();
            s.config.created = created;
            s.save().unwrap();
        }
        let names: Vec<_> = list(root.path())
            .into_iter()
            .map(|s| s.config.name)
            .collect();
        assert_eq!(names, ["New", "Old"]);
        assert_eq!(free_dir(root.path(), "New").file_name().unwrap(), "New (2)");
        assert_eq!(free_dir(root.path(), "a/b:c").file_name().unwrap(), "a-b-c");
        assert_eq!(free_dir(root.path(), "..").file_name().unwrap(), "Server");
    }

    #[test]
    fn checksums_are_checked() {
        assert!(check_sha1(b"abc", "a9993e364706816aba3e25717850c26c9cd0d89d").is_ok());
        assert!(check_sha1(b"abd", "a9993e364706816aba3e25717850c26c9cd0d89d").is_err());
        assert!(
            check_sha256(
                b"abc",
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
            )
            .is_ok()
        );
    }
}
