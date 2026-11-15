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
