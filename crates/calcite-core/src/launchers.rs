//! The official Minecraft Launcher: finding it and reading its installations.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The official launcher's game folder on this computer.
pub fn minecraft_dir() -> Option<PathBuf> {
    let dir = if cfg!(target_os = "macos") {
        dirs::data_dir()?.join("minecraft")
    } else if cfg!(windows) {
        dirs::data_dir()?.join(".minecraft")
    } else {
        dirs::home_dir()?.join(".minecraft")
    };
    dir.join("launcher_profiles.json").is_file().then_some(dir)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Loader {
    Vanilla,
    Fabric,
    Quilt,
    Forge,
    NeoForge,
}

/// What an installation runs, worked out from its version id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedVersion {
    pub loader: Loader,
    /// A concrete game version, or "latest-release" / "latest-snapshot".
    pub game_version: String,
    pub loader_version: Option<String>,
}

/// Understand the version ids the official launcher and loader installers use.
pub fn parse_version_id(id: &str) -> Option<ParsedVersion> {
    let id = id.trim();
    if id.is_empty() {
        return None;
    }
    let vanilla = |game: &str| ParsedVersion {
        loader: Loader::Vanilla,
        game_version: game.to_string(),
        loader_version: None,
    };
    if id == "latest-release" || id == "latest-snapshot" {
        return Some(vanilla(id));
    }
    // fabric-loader-0.16.5-1.21.1, quilt-loader-0.26.0-1.21.1
    for (prefix, loader) in [
        ("fabric-loader-", Loader::Fabric),
        ("quilt-loader-", Loader::Quilt),
    ] {
        if let Some(rest) = id.strip_prefix(prefix) {
            let (loader_version, game) = rest.split_once('-')?;
            return Some(ParsedVersion {
                loader,
                game_version: game.to_string(),
                loader_version: Some(loader_version.to_string()),
            });
        }
    }
    // 1.20.1-forge-47.3.0
    if let Some((game, forge)) = id.split_once("-forge-") {
        return Some(ParsedVersion {
            loader: Loader::Forge,
            game_version: game.to_string(),
            loader_version: Some(forge.to_string()),
        });
    }
    // neoforge-21.1.77 → Minecraft 1.21.1; neoforge-26.1.0.12 → Minecraft 26.1
    if let Some(neo) = id.strip_prefix("neoforge-") {
        let parts: Vec<&str> = neo.split('.').collect();
        let (major, minor) = (parts.first()?, parts.get(1)?);
        let major_n: u32 = major.parse().ok()?;
        let game = if major_n < 26 {
            if *minor == "0" {
                format!("1.{major}")
            } else {
                format!("1.{major}.{minor}")
            }
        } else if *minor == "0" {
            major.to_string()
        } else {
            format!("{major}.{minor}")
        };
        return Some(ParsedVersion {
            loader: Loader::NeoForge,
            game_version: game,
            loader_version: Some(neo.to_string()),
        });
    }
    // Plain release or snapshot ids.
    if id.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        return Some(vanilla(id));
    }
    None
}

#[derive(Debug, Deserialize)]
struct ProfilesFile {
    #[serde(default)]
    profiles: std::collections::HashMap<String, RawProfile>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawProfile {
    #[serde(default)]
    name: String,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    last_version_id: String,
    game_dir: Option<PathBuf>,
    last_used: Option<String>,
}

/// Something that can be recreated as an instance: an official-launcher
/// installation, or a Lunar / Feather profile.
#[derive(Clone, Debug, PartialEq)]
pub struct Installation {
    pub name: String,
    pub version: ParsedVersion,
    /// Game folder with worlds, packs and settings to carry over.
    pub game_dir: PathBuf,
    /// A separate folder of mod jars (Lunar and Feather keep their own).
    pub mods_dir: Option<PathBuf>,
    pub mod_count: usize,
    /// Whether it has anything worth bringing (worlds, packs, settings).
    pub has_content: bool,
}

/// Files and folders that carry over from an installation's game folder.
pub const CARRIED_OVER: [&str; 7] = [
    "saves",
    "resourcepacks",
    "shaderpacks",
    "screenshots",
    "options.txt",
    "servers.dat",
    "mods",
];

/// Read the official launcher's installations.
pub fn installations(minecraft_dir: &Path) -> anyhow::Result<Vec<Installation>> {
    let raw = std::fs::read(minecraft_dir.join("launcher_profiles.json"))?;
    let file: ProfilesFile = serde_json::from_slice(&raw)?;
    let mut list: Vec<(Option<String>, Installation)> = file
        .profiles
        .into_values()
        .filter_map(|p| {
            let version = parse_version_id(&p.last_version_id)?;
            let game_dir = p.game_dir.unwrap_or_else(|| minecraft_dir.to_path_buf());
            let name = if !p.name.trim().is_empty() {
                p.name.trim().to_string()
            } else if p.kind == "latest-snapshot" {
                "Latest snapshot".to_string()
            } else {
                "Minecraft".to_string()
            };
            let has_content = has_game_content(&game_dir);
            Some((
                p.last_used,
                Installation {
                    name,
                    version,
                    game_dir,
                    mods_dir: None,
                    mod_count: 0,
                    has_content,
                },
            ))
        })
        .collect();
    list.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(list.into_iter().map(|(_, i)| i).collect())
}

fn has_game_content(game_dir: &Path) -> bool {
    CARRIED_OVER.iter().any(|f| {
        let path = game_dir.join(f);
        path.is_file()
            || std::fs::read_dir(&path)
                .map(|mut d| d.next().is_some())
                .unwrap_or(false)
    })
}

/// The first release-style version ("1.21.4", "26.3") in a folder name like
/// "1.21.4-fabric" or "fabric-1.20.1".
pub fn version_in_name(name: &str) -> Option<String> {
    name.split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .find(|part| {
            let pieces: Vec<&str> = part.split('.').collect();
            (2..=3).contains(&pieces.len())
                && pieces
                    .iter()
                    .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
                && (pieces[0] == "1" || pieces[0].parse::<u32>().is_ok_and(|n| n >= 26))
        })
        .map(str::to_string)
}

/// Folders of `.jar` files under `root` (a few levels deep), each with the
/// number of jars it holds.
fn jar_folders(root: &Path, max_depth: usize) -> Vec<(PathBuf, usize)> {
    let mut found = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .max_depth(max_depth)
        .follow_links(false)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_dir())
    {
        let jars = std::fs::read_dir(entry.path())
            .map(|d| {
                d.flatten()
                    .filter(|f| f.path().extension().is_some_and(|x| x == "jar"))
                    .count()
            })
            .unwrap_or(0);
        if jars > 0 {
            found.push((entry.path().to_path_buf(), jars));
        }
    }
    found
}

/// Turn a client's per-version mod folders into installations. The version
/// comes from the nearest folder name that contains one; folders without a
/// recognisable version are skipped because there'd be no way to know which
/// Minecraft to install.
fn client_installations(client: &str, roots: &[PathBuf], game_dir: &Path) -> Vec<Installation> {
    let mut list: Vec<Installation> = Vec::new();
    for root in roots.iter().filter(|r| r.is_dir()) {
        for (folder, jars) in jar_folders(root, 5) {
            let version = folder
                .ancestors()
                .take_while(|p| p.starts_with(root))
                .filter_map(|p| p.file_name()?.to_str().and_then(version_in_name))
                .next();
            let Some(game_version) = version else {
                continue;
            };
            if list
                .iter()
                .any(|i| i.mods_dir.as_deref() == Some(folder.as_path()))
            {
                continue;
            }
            list.push(Installation {
                name: format!("{client} {game_version}"),
                version: ParsedVersion {
                    loader: Loader::Fabric,
                    game_version,
                    loader_version: None,
                },
                game_dir: game_dir.to_path_buf(),
                mods_dir: Some(folder),
                mod_count: jars,
                has_content: has_game_content(game_dir),
            });
        }
    }
    list
}

/// Lunar Client keeps its own files in `~/.lunarclient`; added Fabric mods
/// live in per-version profile folders there. Worlds and packs stay in the
/// regular game folder.
pub fn lunar_installations(home: &Path, minecraft_dir: &Path) -> Vec<Installation> {
    let base = home.join(".lunarclient");
    client_installations("Lunar Client", &[base.join("profiles")], minecraft_dir)
}

/// Feather Client keeps added mods in its own folder (location varies by
/// version), with worlds and packs in the regular game folder.
pub fn feather_installations(
    home: &Path,
    data_dir: &Path,
    minecraft_dir: &Path,
) -> Vec<Installation> {
    let roots = [
        minecraft_dir.join("feather"),
        data_dir.join(".feather"),
        data_dir.join("feather"),
        home.join(".feather"),
    ];
    client_installations("Feather Client", &roots, minecraft_dir)
}

/// Lunar or Feather is installed (even if there's nothing to bring yet).
pub fn client_present(home: &Path, data_dir: &Path, minecraft_dir: &Path) -> (bool, bool) {
    let lunar = home.join(".lunarclient").is_dir();
    let feather = [
        minecraft_dir.join("feather"),
        data_dir.join(".feather"),
        data_dir.join("feather"),
        home.join(".feather"),
    ]
    .iter()
    .any(|p| p.is_dir());
    (lunar, feather)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_launcher_version_ids() {
        assert_eq!(
            parse_version_id("fabric-loader-0.16.5-1.21.1"),
            Some(ParsedVersion {
                loader: Loader::Fabric,
                game_version: "1.21.1".into(),
                loader_version: Some("0.16.5".into()),
            })
        );
        assert_eq!(
            parse_version_id("1.20.1-forge-47.3.0")
                .unwrap()
                .loader_version
                .as_deref(),
            Some("47.3.0")
        );
        assert_eq!(
            parse_version_id("neoforge-21.1.77").unwrap().game_version,
            "1.21.1"
        );
        assert_eq!(
            parse_version_id("neoforge-20.4.237").unwrap().game_version,
            "1.20.4"
        );
        assert_eq!(
            parse_version_id("neoforge-26.1.0.12").unwrap().game_version,
            "26.1"
        );
        assert_eq!(
            parse_version_id("quilt-loader-0.26.0-1.20.1")
                .unwrap()
                .loader,
            Loader::Quilt
        );
        assert_eq!(parse_version_id("26.3").unwrap().loader, Loader::Vanilla);
        assert_eq!(
            parse_version_id("latest-release").unwrap().game_version,
            "latest-release"
        );
        assert_eq!(parse_version_id("OptiFine_1.20.1_HD_U_I6"), None);
    }

    #[test]
    fn finds_versions_in_folder_names() {
        assert_eq!(version_in_name("1.21.4-fabric").as_deref(), Some("1.21.4"));
        assert_eq!(version_in_name("fabric-1.20.1").as_deref(), Some("1.20.1"));
        assert_eq!(version_in_name("26.3").as_deref(), Some("26.3"));
        assert_eq!(version_in_name("mods"), None);
        assert_eq!(version_in_name("v2.5"), None);
    }

    #[test]
    fn finds_lunar_and_feather_mods() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let data = tmp.path().join("data");
        let mc = data.join("minecraft");
        std::fs::create_dir_all(mc.join("saves/World")).unwrap();
        let lunar_mods = home.join(".lunarclient/profiles/lunar/1.21.4/mods");
        std::fs::create_dir_all(&lunar_mods).unwrap();
        std::fs::write(lunar_mods.join("sodium.jar"), b"x").unwrap();
        std::fs::write(lunar_mods.join("iris.jar"), b"x").unwrap();
        let feather_mods = data.join(".feather/user-mods/1.20.1-fabric");
        std::fs::create_dir_all(&feather_mods).unwrap();
        std::fs::write(feather_mods.join("modmenu.jar"), b"x").unwrap();
        // A folder without a version is ignored.
        std::fs::create_dir_all(home.join(".lunarclient/profiles/misc")).unwrap();
        std::fs::write(home.join(".lunarclient/profiles/misc/x.jar"), b"x").unwrap();

        let lunar = lunar_installations(&home, &mc);
        assert_eq!(lunar.len(), 1);
        assert_eq!(lunar[0].name, "Lunar Client 1.21.4");
        assert_eq!(lunar[0].mod_count, 2);
        assert!(lunar[0].has_content);

        let feather = feather_installations(&home, &data, &mc);
        assert_eq!(feather.len(), 1);
        assert_eq!(feather[0].version.game_version, "1.20.1");
        assert_eq!(feather[0].version.loader, Loader::Fabric);
        assert_eq!(client_present(&home, &data, &mc), (true, true));
    }

    #[test]
    fn reads_installations() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("saves/World")).unwrap();
        std::fs::write(
            root.join("launcher_profiles.json"),
            r#"{"profiles":{
                "a":{"type":"latest-release","lastVersionId":"latest-release","lastUsed":"2026-01-01T00:00:00Z"},
                "b":{"type":"custom","name":"Fabric","lastVersionId":"fabric-loader-0.16.5-1.21.1","gameDir":"/nowhere","lastUsed":"2026-03-01T00:00:00Z"},
                "c":{"type":"custom","name":"Odd","lastVersionId":"OptiFine_1.20.1_HD"}
            }}"#,
        )
        .unwrap();
        let list = installations(root).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "Fabric");
        assert!(!list[0].has_content);
        assert_eq!(list[1].name, "Minecraft");
        assert!(list[1].has_content);
    }
}
