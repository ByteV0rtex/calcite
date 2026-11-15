//! A small client for Modrinth's public v2 API (search and project pages).
//!
//! Launching, installing and updating go through the backend; this module
//! covers the browsing side, which Modrinth App does from its web UI.

use serde::{Deserialize, Serialize};

pub const API_BASE: &str = "https://api.modrinth.com/v2";

/// What kind of content a search is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ProjectType {
    #[default]
    Modpack,
    Mod,
    Resourcepack,
    Shader,
    Datapack,
    Plugin,
}

impl ProjectType {
    pub const BROWSABLE: [ProjectType; 5] = [
        ProjectType::Modpack,
        ProjectType::Mod,
        ProjectType::Resourcepack,
        ProjectType::Shader,
        ProjectType::Datapack,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ProjectType::Modpack => "modpack",
            ProjectType::Mod => "mod",
            ProjectType::Resourcepack => "resourcepack",
            ProjectType::Shader => "shader",
            ProjectType::Datapack => "datapack",
            ProjectType::Plugin => "plugin",
        }
    }

    /// Plural label for tabs and headings.
    pub fn plural_label(self) -> &'static str {
        match self {
            ProjectType::Modpack => "Modpacks",
            ProjectType::Mod => "Mods",
            ProjectType::Resourcepack => "Resource packs",
            ProjectType::Shader => "Shaders",
            ProjectType::Datapack => "Data packs",
            ProjectType::Plugin => "Plugins",
        }
    }
}

/// Sort order offered by Modrinth search.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SearchIndex {
    #[default]
    Relevance,
    Downloads,
    Follows,
    Newest,
    Updated,
}

impl SearchIndex {
    pub const ALL: [SearchIndex; 5] = [
        SearchIndex::Relevance,
        SearchIndex::Downloads,
        SearchIndex::Follows,
        SearchIndex::Newest,
        SearchIndex::Updated,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            SearchIndex::Relevance => "relevance",
            SearchIndex::Downloads => "downloads",
            SearchIndex::Follows => "follows",
            SearchIndex::Newest => "newest",
            SearchIndex::Updated => "updated",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SearchIndex::Relevance => "Relevance",
            SearchIndex::Downloads => "Most downloaded",
            SearchIndex::Follows => "Most followed",
            SearchIndex::Newest => "Newest",
            SearchIndex::Updated => "Recently updated",
        }
    }
}

/// A search request. Facets are AND-ed groups of OR-ed terms, exactly as
/// Modrinth defines them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchQuery {
    pub text: String,
    pub project_type: ProjectType,
    pub index: SearchIndex,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub categories: Vec<String>,
    pub client_side_only: bool,
    pub server_side_only: bool,
    pub open_source_only: bool,
    pub offset: u32,
    pub limit: u32,
}

impl SearchQuery {
    pub fn new(project_type: ProjectType) -> Self {
        SearchQuery {
            project_type,
            limit: 20,
            ..Default::default()
        }
    }

    /// The `facets` parameter as Modrinth expects it.
    pub fn facets(&self) -> String {
        let mut groups: Vec<Vec<String>> =
            vec![vec![format!("project_type:{}", self.project_type.as_str())]];
        if !self.game_versions.is_empty() {
            groups.push(
                self.game_versions
                    .iter()
                    .map(|v| format!("versions:{v}"))
                    .collect(),
            );
        }
        if !self.loaders.is_empty() {
            groups.push(
                self.loaders
                    .iter()
                    .map(|l| format!("categories:{l}"))
                    .collect(),
            );
        }
        // Each selected category must match.
        for category in &self.categories {
            groups.push(vec![format!("categories:{category}")]);
        }
        if self.client_side_only {
            groups.push(vec!["client_side:required".into()]);
        }
        if self.server_side_only {
            groups.push(vec!["server_side:required".into()]);
        }
        if self.open_source_only {
            groups.push(vec!["open_source:true".into()]);
        }
        serde_json::to_string(&groups).expect("facets serialize")
    }

    /// Query-string pairs for `/search`.
    pub fn params(&self) -> Vec<(&'static str, String)> {
        let mut params = vec![
            ("facets", self.facets()),
            ("index", self.index.as_str().to_string()),
            ("offset", self.offset.to_string()),
            ("limit", self.limit.clamp(1, 100).to_string()),
        ];
        let text = self.text.trim();
        if !text.is_empty() {
            params.push(("query", text.to_string()));
        }
        params
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct SearchHit {
    pub project_id: String,
    pub project_type: String,
    pub slug: String,
    pub author: String,
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub display_categories: Vec<String>,
    #[serde(default)]
    pub versions: Vec<String>,
    pub downloads: u64,
    pub follows: u64,
    pub icon_url: Option<String>,
    pub date_modified: String,
    pub latest_version: Option<String>,
    pub client_side: Option<String>,
    pub server_side: Option<String>,
    #[serde(default)]
    pub gallery: Vec<String>,
    pub featured_gallery: Option<String>,
    /// The icon's dominant color as 0xRRGGBB.
    pub color: Option<u32>,
}

impl SearchHit {
    /// The loaders a mod supports, taken from its categories.
    pub fn loaders(&self) -> Vec<&str> {
        self.categories
            .iter()
            .map(String::as_str)
            .filter(|c| KNOWN_LOADERS.contains(c))
            .collect()
    }

    /// The newest release game version this project supports, if any.
    pub fn newest_game_version(&self) -> Option<&str> {
        self.versions
            .iter()
            .filter(|v| is_release_version(v))
            .max_by(|a, b| compare_game_versions(a, b))
            .map(String::as_str)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct SearchResults {
    pub hits: Vec<SearchHit>,
    pub offset: u32,
    pub limit: u32,
    pub total_hits: u32,
}

pub const KNOWN_LOADERS: [&str; 9] = [
    "fabric",
    "quilt",
    "forge",
    "neoforge",
    "iris",
    "optifine",
    "canvas",
    "datapack",
    "minecraft",
];

/// True for plain release versions like "1.21.4" or "26.3" (no snapshots,
/// pre-releases or release candidates).
pub fn is_release_version(v: &str) -> bool {
    !v.is_empty()
        && v.split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

/// Order release versions numerically ("1.21.10" > "1.21.9").
pub fn compare_game_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |v: &str| -> Vec<u32> { v.split('.').map(|p| p.parse().unwrap_or(0)).collect() };
    parse(a).cmp(&parse(b))
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct GalleryImage {
    /// Pre-resized (350px) image for thumbnails.
    pub url: String,
    pub raw_url: Option<String>,
    #[serde(default)]
    pub featured: bool,
    pub title: Option<String>,
    pub description: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct License {
    pub id: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Project {
    pub id: String,
    pub slug: String,
    pub project_type: String,
    pub team: String,
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub body: String,
    pub published: String,
    pub updated: String,
    pub license: Option<License>,
    pub downloads: u64,
    pub followers: u64,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub loaders: Vec<String>,
    #[serde(default)]
    pub game_versions: Vec<String>,
    pub icon_url: Option<String>,
    pub issues_url: Option<String>,
    pub source_url: Option<String>,
    pub wiki_url: Option<String>,
    pub discord_url: Option<String>,
    #[serde(default)]
    pub gallery: Vec<GalleryImage>,
    pub client_side: Option<String>,
    pub server_side: Option<String>,
}

impl Project {
    pub fn project_type(&self) -> ProjectType {
        match self.project_type.as_str() {
            "mod" => ProjectType::Mod,
            "resourcepack" => ProjectType::Resourcepack,
            "shader" => ProjectType::Shader,
            "datapack" => ProjectType::Datapack,
            "plugin" => ProjectType::Plugin,
            _ => ProjectType::Modpack,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct VersionFile {
    pub url: String,
    pub filename: String,
    #[serde(default)]
    pub primary: bool,
    pub size: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Version {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub version_number: String,
    #[serde(default)]
    pub changelog: Option<String>,
    pub date_published: String,
    pub downloads: u64,
    /// "release", "beta" or "alpha".
    pub version_type: String,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub loaders: Vec<String>,
    #[serde(default)]
    pub files: Vec<VersionFile>,
}

impl Version {
    /// "1.20.1–1.21.4" style summary of the supported game versions.
    pub fn game_version_summary(&self) -> String {
        let mut releases: Vec<&str> = self
            .game_versions
            .iter()
            .map(String::as_str)
            .filter(|v| is_release_version(v))
            .collect();
        releases.sort_by(|a, b| compare_game_versions(a, b));
        match (releases.first(), releases.last()) {
            (Some(first), Some(last)) if first == last => first.to_string(),
            (Some(first), Some(last)) => format!("{first}–{last}"),
            _ => self.game_versions.first().cloned().unwrap_or_default(),
        }
    }

    /// Supports this loader (resource packs and shaders are loader-free).
    pub fn supports(&self, loader: &str, game_version: &str) -> bool {
        let loader_ok = self.loaders.iter().any(|l| l == loader)
            || self.loaders.iter().all(|l| {
                matches!(
                    l.as_str(),
                    "minecraft" | "iris" | "optifine" | "canvas" | "datapack"
                )
            });
        loader_ok && self.game_versions.iter().any(|v| v == game_version)
    }
}

/// Shared HTTP client for Modrinth's API and CDN.
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base: String,
}

impl Client {
    pub fn new(user_agent: &str) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(user_agent)
            .timeout(std::time::Duration::from_secs(20))
            .build()?;
        Ok(Client {
            http,
            base: API_BASE.to_string(),
        })
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    pub async fn search(&self, query: &SearchQuery) -> anyhow::Result<SearchResults> {
        let response = self
            .http
            .get(format!("{}/search", self.base))
            .query(&query.params())
            .send()
            .await?
            .error_for_status()?;
        Ok(response.json().await?)
    }

    pub async fn project(&self, id_or_slug: &str) -> anyhow::Result<Project> {
        let response = self
            .http
            .get(format!("{}/project/{id_or_slug}", self.base))
            .send()
            .await?
            .error_for_status()?;
        Ok(response.json().await?)
    }

    /// Every version of a project, newest first.
    pub async fn versions(&self, id_or_slug: &str) -> anyhow::Result<Vec<Version>> {
        let response = self
            .http
            .get(format!("{}/project/{id_or_slug}/version", self.base))
            .send()
            .await?
            .error_for_status()?;
        Ok(response.json().await?)
    }

    pub async fn version(&self, id: &str) -> anyhow::Result<Version> {
        let response = self
            .http
            .get(format!("{}/version/{id}", self.base))
            .send()
            .await?
            .error_for_status()?;
        Ok(response.json().await?)
    }

    /// Release game versions, newest first (for filters).
    pub async fn release_game_versions(&self) -> anyhow::Result<Vec<String>> {
        #[derive(Deserialize)]
        struct Tag {
            version: String,
            version_type: String,
        }
        let tags: Vec<Tag> = self
            .http
            .get(format!("{}/tag/game_version", self.base))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(tags
            .into_iter()
            .filter(|t| t.version_type == "release")
            .map(|t| t.version)
            .collect())
    }

    /// Download raw bytes (icons, gallery images).
    pub async fn bytes(&self, url: &str) -> anyhow::Result<Vec<u8>> {
        let response = self.http.get(url).send().await?.error_for_status()?;
        Ok(response.bytes().await?.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facets_group_terms_correctly() {
        let mut q = SearchQuery::new(ProjectType::Mod);
        q.game_versions = vec!["1.21.4".into(), "1.21.3".into()];
        q.loaders = vec!["fabric".into()];
        q.categories = vec!["optimization".into(), "utility".into()];
        q.open_source_only = true;
        assert_eq!(
            q.facets(),
            r#"[["project_type:mod"],["versions:1.21.4","versions:1.21.3"],["categories:fabric"],["categories:optimization"],["categories:utility"],["open_source:true"]]"#
        );
    }

    #[test]
    fn params_skip_empty_text_and_clamp_limit() {
        let mut q = SearchQuery::new(ProjectType::Modpack);
        q.text = "   ".into();
        q.limit = 500;
        let params = q.params();
        assert!(params.iter().all(|(k, _)| *k != "query"));
        assert!(params.contains(&("limit", "100".to_string())));
    }

    #[test]
    fn release_versions_sort_numerically() {
        assert!(is_release_version("1.21.4"));
        assert!(is_release_version("26.3"));
        assert!(!is_release_version("26.3-rc-1"));
        assert!(!is_release_version("24w14a"));
        assert_eq!(
            compare_game_versions("1.21.10", "1.21.9"),
            std::cmp::Ordering::Greater
        );
    }

    #[test]
    fn version_summaries_and_support() {
        let v: Version = serde_json::from_value(serde_json::json!({
            "id": "v", "project_id": "p", "name": "n", "version_number": "1.0",
            "date_published": "2026-01-01T00:00:00Z", "downloads": 3,
            "version_type": "release",
            "game_versions": ["1.21.4", "1.20.1", "1.21.1", "26.4-snapshot-1"],
            "loaders": ["fabric", "quilt"], "files": []
        }))
        .unwrap();
        assert_eq!(v.game_version_summary(), "1.20.1–1.21.4");
        assert!(v.supports("fabric", "1.21.1"));
        assert!(!v.supports("forge", "1.21.1"));
        assert!(!v.supports("fabric", "1.19.2"));
        let pack: Version = serde_json::from_value(serde_json::json!({
            "id": "v", "project_id": "p", "name": "n", "version_number": "1.0",
            "date_published": "2026-01-01T00:00:00Z", "downloads": 3,
            "version_type": "release", "game_versions": ["1.21.1"],
            "loaders": ["minecraft"], "files": []
        }))
        .unwrap();
        assert!(pack.supports("fabric", "1.21.1"));
    }

    #[test]
    fn newest_game_version_ignores_snapshots() {
        let hit: SearchHit = serde_json::from_value(serde_json::json!({
            "project_id": "x", "project_type": "mod", "slug": "x", "author": "a",
            "title": "X", "description": "", "downloads": 1, "follows": 1,
            "icon_url": null, "date_modified": "2026-01-01T00:00:00Z",
            "latest_version": null, "client_side": null, "server_side": null,
            "versions": ["1.21.9", "1.21.10", "26.4-snapshot-1", "1.20.1"],
            "categories": ["fabric", "optimization", "quilt"]
        }))
        .unwrap();
        assert_eq!(hit.newest_game_version(), Some("1.21.10"));
        assert_eq!(hit.loaders(), vec!["fabric", "quilt"]);
    }
}
