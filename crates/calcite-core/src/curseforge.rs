//! CurseForge's API: search mods, modpacks, resource packs and shaders,
//! and find the right file for an instance.
//!
//! It needs an API key. Builds get one from `CALCITE_CURSEFORGE_KEY` at
//! compile time (a CI secret, never in the source); without one, CurseForge
//! is simply not offered.

use serde::Deserialize;

const API: &str = "https://api.curseforge.com/v1";
/// Minecraft on CurseForge.
const GAME: u32 = 432;

/// The key this build was made with, if any.
pub fn api_key() -> Option<&'static str> {
    option_env!("CALCITE_CURSEFORGE_KEY").filter(|k| !k.trim().is_empty())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Mods,
    Modpacks,
    ResourcePacks,
    Shaders,
}

impl Class {
    pub const ALL: [Class; 4] = [
        Class::Mods,
        Class::Modpacks,
        Class::ResourcePacks,
        Class::Shaders,
    ];

    pub fn id(self) -> u32 {
        match self {
            Class::Mods => 6,
            Class::Modpacks => 4471,
            Class::ResourcePacks => 12,
            Class::Shaders => 6552,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Class::Mods => "Mods",
            Class::Modpacks => "Modpacks",
            Class::ResourcePacks => "Resource packs",
            Class::Shaders => "Shaders",
        }
    }
}

/// CurseForge's numbers for mod loaders.
pub fn loader_type(loader: &str) -> Option<u32> {
    match loader.to_ascii_lowercase().as_str() {
        "forge" => Some(1),
        "fabric" => Some(4),
        "quilt" => Some(5),
        "neoforge" => Some(6),
        _ => None,
    }
}

#[derive(Clone, Debug, Default)]
pub struct Query {
    pub text: String,
    pub class: Option<Class>,
    pub game_version: Option<String>,
    pub loader: Option<String>,
    pub offset: u32,
}

impl Query {
    fn params(&self) -> Vec<(&'static str, String)> {
        let mut p = vec![
            ("gameId", GAME.to_string()),
            (
                "classId",
                self.class.unwrap_or(Class::Mods).id().to_string(),
            ),
            // Popularity, most first.
            ("sortField", "2".into()),
            ("sortOrder", "desc".into()),
            ("pageSize", "20".into()),
            ("index", self.offset.to_string()),
        ];
        if !self.text.trim().is_empty() {
            p.push(("searchFilter", self.text.trim().to_string()));
        }
        if let Some(v) = &self.game_version {
            p.push(("gameVersion", v.clone()));
        }
        if let Some(l) = self.loader.as_deref().and_then(loader_type) {
            p.push(("modLoaderType", l.to_string()));
        }
        p
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: u64,
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub download_count: f64,
    pub logo: Option<Logo>,
    #[serde(default)]
    pub authors: Vec<Author>,
    pub links: Option<Links>,
    pub class_id: Option<u32>,
}

impl Project {
    pub fn author(&self) -> &str {
        self.authors.first().map(|a| a.name.as_str()).unwrap_or("")
    }

    pub fn icon_url(&self) -> Option<&str> {
        self.logo.as_ref().map(|l| l.thumbnail_url.as_str())
    }

    pub fn page_url(&self) -> String {
        self.links
            .as_ref()
            .and_then(|l| l.website_url.clone())
            .unwrap_or_else(|| {
                format!(
                    "https://www.curseforge.com/minecraft/search?search={}",
                    self.slug
                )
            })
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Logo {
    pub thumbnail_url: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Author {
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Links {
    pub website_url: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct File {
    pub id: u64,
    #[serde(default)]
    pub mod_id: u64,
    pub display_name: String,
    pub file_name: String,
    /// Missing when the author doesn't allow other apps to download it;
    /// then the player has to get it from the website.
    pub download_url: Option<String>,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub hashes: Vec<Hash>,
    pub file_length: Option<u64>,
}

impl File {
    pub fn sha1(&self) -> Option<&str> {
        // algo 1 is SHA-1, 2 is MD5.
        self.hashes
            .iter()
            .find(|h| h.algo == 1)
            .map(|h| h.value.as_str())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Hash {
    pub value: String,
    pub algo: u32,
}

#[derive(Deserialize)]
struct Page<T> {
    data: Vec<T>,
    pagination: Option<Pagination>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pagination {
    total_count: u64,
}

pub struct Results {
    pub projects: Vec<Project>,
    pub total: u64,
}

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    key: String,
}

impl Client {
    /// A client, if this build has a key.
    pub fn new(http: reqwest::Client) -> Option<Client> {
        Some(Client {
            http,
            key: api_key()?.to_string(),
        })
    }

    async fn get<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> anyhow::Result<T> {
        Ok(self
            .http
            .get(format!("{API}{path}"))
            .header("x-api-key", &self.key)
            .header("Accept", "application/json")
            .query(query)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub async fn search(&self, query: &Query) -> anyhow::Result<Results> {
        let page: Page<Project> = self.get("/mods/search", &query.params()).await?;
        Ok(Results {
            total: page
                .pagination
                .map(|p| p.total_count)
                .unwrap_or(page.data.len() as u64),
            projects: page.data,
        })
    }

    async fn post<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        body: serde_json::Value,
    ) -> anyhow::Result<T> {
        Ok(self
            .http
            .post(format!("{API}{path}"))
            .header("x-api-key", &self.key)
            .header("Accept", "application/json")
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    /// Many files at once, by id.
    pub async fn files_by_id(&self, ids: &[u64]) -> anyhow::Result<Vec<File>> {
        let mut all = Vec::new();
        for chunk in ids.chunks(500) {
            let page: Page<File> = self
                .post("/mods/files", serde_json::json!({ "fileIds": chunk }))
                .await?;
            all.extend(page.data);
        }
        Ok(all)
    }

    /// Many projects at once, by id (for what kind each one is).
    pub async fn projects_by_id(&self, ids: &[u64]) -> anyhow::Result<Vec<Project>> {
        let mut all = Vec::new();
        for chunk in ids.chunks(500) {
            let page: Page<Project> = self
                .post("/mods", serde_json::json!({ "modIds": chunk }))
                .await?;
            all.extend(page.data);
        }
        Ok(all)
    }

    /// Files for a project that fit the game version and loader, newest
    /// first.
    pub async fn files(
        &self,
        project: u64,
        game_version: Option<&str>,
        loader: Option<&str>,
    ) -> anyhow::Result<Vec<File>> {
        let mut query = vec![("pageSize", "50".to_string())];
        if let Some(v) = game_version {
            query.push(("gameVersion", v.to_string()));
        }
        if let Some(l) = loader.and_then(loader_type) {
            query.push(("modLoaderType", l.to_string()));
        }
        let page: Page<File> = self.get(&format!("/mods/{project}/files"), &query).await?;
        Ok(page.data)
    }
}

// ---- CurseForge modpacks -------------------------------------------------
//
// A CurseForge pack is a zip with `manifest.json` (game version, loader, and
// the project and file id of every mod) and an overrides folder. Calcite
// downloads the mods itself (checked against CurseForge's SHA-1) and builds a
// Modrinth pack (.mrpack) with them inside, so the loader install and the new
// instance go through the same path as any other pack. (The backend wants a
// SHA-512 for anything it downloads, which CurseForge doesn't give.)

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub minecraft: ManifestGame,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub files: Vec<ManifestFile>,
    #[serde(default = "default_overrides")]
    pub overrides: String,
}

fn default_overrides() -> String {
    "overrides".into()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestGame {
    pub version: String,
    #[serde(default)]
    pub mod_loaders: Vec<ManifestLoader>,
}

#[derive(Debug, Deserialize)]
pub struct ManifestLoader {
    pub id: String,
    #[serde(default)]
    pub primary: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestFile {
    #[serde(rename = "projectID")]
    pub project_id: u64,
    #[serde(rename = "fileID")]
    pub file_id: u64,
    #[serde(default = "yes")]
    pub required: bool,
}

fn yes() -> bool {
    true
}

impl Manifest {
    /// `("forge", "47.2.0")` from `forge-47.2.0`, as a Modrinth pack names it.
    pub fn loader(&self) -> Option<(&'static str, String)> {
        let loader = self
            .minecraft
            .mod_loaders
            .iter()
            .find(|l| l.primary)
            .or(self.minecraft.mod_loaders.first())?;
        let (kind, version) = loader.id.split_once('-')?;
        let key = match kind.to_ascii_lowercase().as_str() {
            "forge" => "forge",
            "neoforge" => "neoforge",
            "fabric" => "fabric-loader",
            "quilt" => "quilt-loader",
            _ => return None,
        };
        Some((key, version.to_string()))
    }
}

pub fn read_manifest(zip: &std::path::Path) -> anyhow::Result<Manifest> {
    use std::io::Read as _;
    let mut archive = zip::ZipArchive::new(std::fs::File::open(zip)?)?;
    let mut text = String::new();
    archive
        .by_name("manifest.json")
        .map_err(|_| anyhow::anyhow!("That isn't a CurseForge modpack (no manifest.json)."))?
        .read_to_string(&mut text)?;
    Ok(serde_json::from_str(&text)?)
}

/// Where a file goes in the instance, by its project's CurseForge section.
fn folder_for(class_id: Option<u32>) -> &'static str {
    match class_id {
        Some(12) => "resourcepacks",
        Some(6552) => "shaderpacks",
        Some(6945) => "datapacks",
        _ => "mods",
    }
}

/// A file of the pack to download into the instance.
#[derive(Clone, Debug, PartialEq)]
pub struct Planned {
    /// Path in the instance, like `mods/jei.jar`.
    pub path: String,
    pub url: String,
    pub sha1: String,
}

/// The Modrinth pack index for a manifest and the files to put in it, given
/// what CurseForge says about the files and projects. Also returns the
/// files that can only be downloaded from the website.
pub fn build_index(
    manifest: &Manifest,
    files: &[File],
    projects: &[Project],
) -> anyhow::Result<(serde_json::Value, Vec<Planned>, Vec<String>)> {
    let mut entries = Vec::new();
    let mut manual = Vec::new();
    for wanted in manifest.files.iter().filter(|f| f.required) {
        let project = projects.iter().find(|p| p.id == wanted.project_id);
        let Some(file) = files.iter().find(|f| f.id == wanted.file_id) else {
            manual.push(
                project
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| format!("project {}", wanted.project_id)),
            );
            continue;
        };
        let (Some(url), Some(sha1)) = (file.download_url.as_ref(), file.sha1()) else {
            manual.push(
                project
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| file.display_name.clone()),
            );
            continue;
        };
        // A plain file name only: a server never gets to choose a path.
        let name = std::path::Path::new(&file.file_name)
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| !n.is_empty() && !n.starts_with('.'))
            .ok_or_else(|| anyhow::anyhow!("odd file name {:?}", file.file_name))?;
        entries.push(Planned {
            path: format!("{}/{name}", folder_for(project.and_then(|p| p.class_id))),
            url: url.clone(),
            sha1: sha1.to_string(),
        });
    }
    let mut dependencies = serde_json::Map::new();
    dependencies.insert(
        "minecraft".into(),
        manifest.minecraft.version.clone().into(),
    );
    if let Some((key, version)) = manifest.loader() {
        dependencies.insert(key.into(), version.into());
    }
    let name = if manifest.name.trim().is_empty() {
        "CurseForge modpack".to_string()
    } else {
        manifest.name.trim().to_string()
    };
    Ok((
        serde_json::json!({
            "formatVersion": 1,
            "game": "minecraft",
            "versionId": if manifest.version.is_empty() { "1" } else { manifest.version.as_str() },
            "name": name,
            "summary": "Converted from CurseForge by Calcite",
            "files": [],
            "dependencies": dependencies,
        }),
        entries,
        manual,
    ))
}

/// Write a .mrpack: the index, the downloaded files (as `(path in the
/// instance, file on disk)`), and the CurseForge pack's overrides.
pub fn write_mrpack(
    cf_zip: &std::path::Path,
    overrides: &str,
    index: &serde_json::Value,
    downloaded: &[(String, std::path::PathBuf)],
    out: &std::path::Path,
) -> anyhow::Result<()> {
    use std::io::{Read as _, Write as _};
    let mut source = zip::ZipArchive::new(std::fs::File::open(cf_zip)?)?;
    let mut pack = zip::ZipWriter::new(std::fs::File::create(out)?);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    pack.start_file("modrinth.index.json", options)?;
    pack.write_all(serde_json::to_string_pretty(index)?.as_bytes())?;
    // Jars and zips are compressed already.
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (path, file) in downloaded {
        pack.start_file(format!("overrides/{path}"), stored)?;
        std::io::copy(&mut std::fs::File::open(file)?, &mut pack)?;
    }
    let prefix = format!("{}/", overrides.trim_matches('/'));
    for ix in 0..source.len() {
        let mut entry = source.by_index(ix)?;
        // enclosed_name refuses "../" tricks.
        let Some(inner) = entry.enclosed_name() else {
            continue;
        };
        let inner = inner.to_string_lossy().replace('\\', "/");
        let Some(rest) = inner.strip_prefix(&prefix) else {
            continue;
        };
        if rest.is_empty() || entry.is_dir() {
            continue;
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        pack.start_file(format!("overrides/{rest}"), options)?;
        pack.write_all(&bytes)?;
    }
    pack.finish()?;
    Ok(())
}

/// Download a CurseForge modpack's newest file and turn it into a .mrpack
/// in `out_dir`. Returns the pack's path, its name, and the mods that have
/// to be fetched by hand.
pub async fn prepare_pack(
    client: &Client,
    project: u64,
    out_dir: &std::path::Path,
) -> anyhow::Result<(std::path::PathBuf, String, Vec<String>)> {
    let files = client.files(project, None, None).await?;
    let file = files
        .iter()
        .find(|f| f.download_url.is_some())
        .ok_or_else(|| anyhow::anyhow!("This pack can only be downloaded from curseforge.com."))?;
    let bytes = client
        .http
        .get(file.download_url.as_deref().unwrap_or_default())
        .timeout(std::time::Duration::from_secs(600))
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    std::fs::create_dir_all(out_dir)?;
    let cf_zip = out_dir.join(format!("curseforge-{}.zip", file.id));
    std::fs::write(&cf_zip, &bytes)?;
    convert_pack_file(client, &cf_zip, out_dir).await
}

/// Turn a CurseForge modpack zip on disk into a .mrpack in `out_dir`.
pub async fn convert_pack_file(
    client: &Client,
    cf_zip: &std::path::Path,
    out_dir: &std::path::Path,
) -> anyhow::Result<(std::path::PathBuf, String, Vec<String>)> {
    let manifest = read_manifest(cf_zip)?;
    let file_ids: Vec<u64> = manifest.files.iter().map(|f| f.file_id).collect();
    let project_ids: Vec<u64> = manifest.files.iter().map(|f| f.project_id).collect();
    let files = client.files_by_id(&file_ids).await?;
    let projects = client.projects_by_id(&project_ids).await?;
    let (index, planned, manual) = build_index(&manifest, &files, &projects)?;
    let name = index["name"]
        .as_str()
        .unwrap_or("CurseForge modpack")
        .to_string();
    std::fs::create_dir_all(out_dir)?;
    let downloaded = download_all(client, &planned, &out_dir.join("files")).await?;
    let out = out_dir.join(format!("{}.mrpack", uuid_like(&name)));
    write_mrpack(cf_zip, &manifest.overrides, &index, &downloaded, &out)?;
    let _ = std::fs::remove_dir_all(out_dir.join("files"));
    Ok((out, name, manual))
}

/// Download the planned files, eight at a time, each checked against its
/// SHA-1. Returns where each one was saved.
async fn download_all(
    client: &Client,
    planned: &[Planned],
    dir: &std::path::Path,
) -> anyhow::Result<Vec<(String, std::path::PathBuf)>> {
    use futures::StreamExt as _;
    use sha1::Digest as _;
    std::fs::create_dir_all(dir)?;
    // Each download owns what it needs, so the whole thing can run on any
    // thread of the backend.
    let downloads = planned.iter().cloned().enumerate().map(|(ix, file)| {
        let http = client.http.clone();
        let saved = dir.join(format!("{ix}.part"));
        async move {
            let bytes = http
                .get(&file.url)
                .timeout(std::time::Duration::from_secs(300))
                .send()
                .await?
                .error_for_status()?
                .bytes()
                .await?;
            let got: String = sha1::Sha1::digest(&bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            if !got.eq_ignore_ascii_case(&file.sha1) {
                anyhow::bail!("{} was damaged in download (checksum mismatch)", file.path);
            }
            tokio::fs::write(&saved, &bytes).await?;
            anyhow::Ok((file.path, saved))
        }
    });
    let results: Vec<anyhow::Result<(String, std::path::PathBuf)>> =
        futures::stream::iter(downloads)
            .buffer_unordered(8)
            .collect()
            .await;
    results.into_iter().collect()
}

/// A file-name-safe version of a pack name.
fn uuid_like(name: &str) -> String {
    let safe: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let safe = safe.trim_matches('-').to_string();
    if safe.is_empty() { "pack".into() } else { safe }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Talks to the real API: `CALCITE_CURSEFORGE_KEY=… cargo test -p
    /// calcite-core curseforge -- --ignored`.
    #[tokio::test]
    #[ignore = "needs a CurseForge key and the network"]
    async fn live_search_pick_and_download() {
        use sha1::Digest as _;
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::Client::builder()
            .user_agent("calcite-tests")
            .build()
            .unwrap();
        let client = Client::new(http.clone()).expect("build with CALCITE_CURSEFORGE_KEY set");
        let results = client
            .search(&Query {
                text: "Just Enough Items".into(),
                class: Some(Class::Mods),
                game_version: Some("1.20.1".into()),
                loader: Some("forge".into()),
                offset: 0,
            })
            .await
            .unwrap();
        assert!(results.total > 0);
        let jei = results
            .projects
            .iter()
            .find(|p| p.slug == "jei")
            .expect("JEI is found");
        let files = client
            .files(jei.id, Some("1.20.1"), Some("forge"))
            .await
            .unwrap();
        let file = files
            .iter()
            .find(|f| f.download_url.is_some())
            .expect("a downloadable 1.20.1 Forge file");
        assert!(
            file.game_versions.iter().any(|v| v == "1.20.1"),
            "{:?}",
            file.game_versions
        );
        let bytes = http
            .get(file.download_url.as_ref().unwrap())
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        let got: String = sha1::Sha1::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(Some(got.as_str()), file.sha1());
        assert!(bytes.starts_with(b"PK"), "a jar");
        println!(
            "ok: {} → {} ({} bytes)",
            jei.name,
            file.file_name,
            bytes.len()
        );
    }

    /// `CALCITE_CURSEFORGE_KEY=… CF_PACK_OUT=/some/dir cargo test -p
    /// calcite-core live_convert -- --ignored`: downloads a real pack and
    /// converts it, leaving both files in CF_PACK_OUT.
    #[tokio::test]
    #[ignore = "needs a CurseForge key and the network"]
    async fn live_convert_a_real_pack() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::Client::builder()
            .user_agent("calcite-tests")
            .build()
            .unwrap();
        let client = Client::new(http).expect("build with CALCITE_CURSEFORGE_KEY set");
        let found = client
            .search(&Query {
                text: "Fabulously Optimized".into(),
                class: Some(Class::Modpacks),
                ..Query::default()
            })
            .await
            .unwrap();
        let pack = found
            .projects
            .iter()
            .find(|p| p.name.contains("Fabulously Optimized"))
            .expect("the pack is on CurseForge");
        let out = std::env::var("CF_PACK_OUT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir().join("calcite-cf-live"));
        let (mrpack, name, manual) = prepare_pack(&client, pack.id, &out).await.unwrap();
        let mut archive = zip::ZipArchive::new(std::fs::File::open(&mrpack).unwrap()).unwrap();
        let index: serde_json::Value = {
            use std::io::Read as _;
            let mut text = String::new();
            archive
                .by_name("modrinth.index.json")
                .unwrap()
                .read_to_string(&mut text)
                .unwrap();
            serde_json::from_str(&text).unwrap()
        };
        let files = (0..archive.len())
            .filter(|&i| {
                archive
                    .by_index(i)
                    .unwrap()
                    .name()
                    .is_ok_and(|n| n.starts_with("overrides/mods/"))
            })
            .count();
        println!(
            "ok: {name}: {files} mods inside, {} by hand, deps {}, {} entries, at {}",
            manual.len(),
            index["dependencies"],
            archive.len(),
            mrpack.display()
        );
        assert!(files > 5);
        assert!(index["dependencies"]["minecraft"].is_string());
    }

    fn sample_manifest() -> Manifest {
        serde_json::from_str(
            r#"{"minecraft":{"version":"1.20.1","modLoaders":[{"id":"forge-47.2.0","primary":true}]},
            "manifestType":"minecraftModpack","manifestVersion":1,"name":"Test Pack","version":"2.1",
            "files":[{"projectID":1,"fileID":11,"required":true},
                     {"projectID":2,"fileID":22,"required":true},
                     {"projectID":3,"fileID":33,"required":true},
                     {"projectID":4,"fileID":44,"required":false}],
            "overrides":"overrides"}"#,
        )
        .unwrap()
    }

    fn file(id: u64, project: u64, name: &str, url: Option<&str>) -> File {
        File {
            id,
            mod_id: project,
            display_name: name.into(),
            file_name: name.into(),
            download_url: url.map(str::to_string),
            game_versions: vec![],
            hashes: vec![Hash {
                value: format!("sha{id}"),
                algo: 1,
            }],
            file_length: Some(10),
        }
    }

    fn project(id: u64, name: &str, class: u32) -> Project {
        Project {
            id,
            name: name.into(),
            slug: name.to_lowercase(),
            summary: String::new(),
            download_count: 0.0,
            logo: None,
            authors: vec![],
            links: None,
            class_id: Some(class),
        }
    }

    #[test]
    fn a_curseforge_manifest_becomes_a_modrinth_index() {
        let manifest = sample_manifest();
        assert_eq!(manifest.loader(), Some(("forge", "47.2.0".into())));
        let files = [
            file(11, 1, "jei.jar", Some("https://edge.forgecdn.net/jei.jar")),
            file(
                22,
                2,
                "faithful.zip",
                Some("https://edge.forgecdn.net/faithful.zip"),
            ),
            file(33, 3, "blocked.jar", None),
        ];
        let projects = [
            project(1, "JEI", 6),
            project(2, "Faithful", 12),
            project(3, "Blocked Mod", 6),
        ];
        let (index, planned, manual) = build_index(&manifest, &files, &projects).unwrap();
        assert_eq!(index["name"], "Test Pack");
        assert_eq!(index["dependencies"]["minecraft"], "1.20.1");
        assert_eq!(index["dependencies"]["forge"], "47.2.0");
        let paths: Vec<&str> = planned.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["mods/jei.jar", "resourcepacks/faithful.zip"]);
        assert_eq!(planned[0].sha1, "sha11");
        // Downloaded by Calcite and shipped inside the pack instead.
        assert_eq!(index["files"].as_array().unwrap().len(), 0);
        // Website-only files are listed for the player; optional ones skipped.
        assert_eq!(manual, ["Blocked Mod"]);
    }

    #[test]
    fn overrides_are_carried_over_safely() {
        use std::io::{Read as _, Write as _};
        let dir = tempfile::tempdir().unwrap();
        let cf = dir.path().join("pack.zip");
        {
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&cf).unwrap());
            let o = zip::write::SimpleFileOptions::default();
            zip.start_file("manifest.json", o).unwrap();
            zip.write_all(b"{}").unwrap();
            zip.start_file("overrides/config/jei.toml", o).unwrap();
            zip.write_all(b"x=1").unwrap();
            zip.start_file("../evil.txt", o).unwrap();
            zip.write_all(b"no").unwrap();
            zip.finish().unwrap();
        }
        let out = dir.path().join("out.mrpack");
        let jar = dir.path().join("0.part");
        std::fs::write(&jar, b"PKjar").unwrap();
        write_mrpack(
            &cf,
            "overrides",
            &serde_json::json!({"name":"x"}),
            &[("mods/jei.jar".into(), jar)],
            &out,
        )
        .unwrap();
        let mut pack = zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap();
        let names: Vec<String> = pack
            .file_names()
            .filter_map(|n| n.ok())
            .map(String::from)
            .collect();
        assert!(names.contains(&"modrinth.index.json".to_string()));
        assert!(names.contains(&"overrides/config/jei.toml".to_string()));
        assert!(names.contains(&"overrides/mods/jei.jar".to_string()));
        assert!(!names.iter().any(|n| n.contains("evil")));
        let mut text = String::new();
        pack.by_name("overrides/config/jei.toml")
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        assert_eq!(text, "x=1");
    }

    #[test]
    fn queries_carry_the_filters() {
        let q = Query {
            text: " sodium ".into(),
            class: Some(Class::Mods),
            game_version: Some("1.21.4".into()),
            loader: Some("NeoForge".into()),
            offset: 20,
        };
        let p = q.params();
        let get = |k: &str| p.iter().find(|(key, _)| *key == k).map(|(_, v)| v.as_str());
        assert_eq!(get("gameId"), Some("432"));
        assert_eq!(get("classId"), Some("6"));
        assert_eq!(get("searchFilter"), Some("sodium"));
        assert_eq!(get("gameVersion"), Some("1.21.4"));
        assert_eq!(get("modLoaderType"), Some("6"));
        assert_eq!(get("index"), Some("20"));
        assert_eq!(
            Query::default()
                .params()
                .iter()
                .filter(|(k, _)| *k == "searchFilter")
                .count(),
            0
        );
    }

    #[test]
    fn reads_search_results_and_files() {
        let page: Page<Project> = serde_json::from_str(
            r#"{"data":[{"id":394468,"name":"Sodium","slug":"sodium","summary":"Fast",
            "downloadCount":1.5e8,"logo":{"thumbnailUrl":"https://media.forgecdn.net/x.png"},
            "authors":[{"name":"jellysquid3"}],"links":{"websiteUrl":"https://www.curseforge.com/minecraft/mc-mods/sodium"},
            "classId":6}],"pagination":{"index":0,"pageSize":20,"resultCount":1,"totalCount":1}}"#,
        )
        .unwrap();
        let sodium = &page.data[0];
        assert_eq!(sodium.author(), "jellysquid3");
        assert_eq!(sodium.icon_url(), Some("https://media.forgecdn.net/x.png"));
        assert!(sodium.page_url().ends_with("/mc-mods/sodium"));

        let files: Page<File> = serde_json::from_str(
            r#"{"data":[{"id":1,"displayName":"Sodium 0.6","fileName":"sodium.jar","downloadUrl":null,
            "gameVersions":["1.21.4","Fabric"],"hashes":[{"value":"abc","algo":1},{"value":"d","algo":2}],
            "fileLength":100}]}"#,
        )
        .unwrap();
        assert_eq!(files.data[0].download_url, None);
        assert_eq!(files.data[0].sha1(), Some("abc"));
    }
}
