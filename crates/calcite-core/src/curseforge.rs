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
