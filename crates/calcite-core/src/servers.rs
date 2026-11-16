//! Minecraft servers listed on Modrinth: search, and what each one needs
//! (plain Minecraft of some version, or a modpack) to join it.

use serde::Deserialize;

use crate::modrinth::Client;

const SEARCH: &str = "https://api.modrinth.com/v3/search";

/// Gameplay tags worth offering as quick filters.
pub const TAGS: [(&str, &str); 14] = [
    ("smp", "SMP"),
    ("survival-mode", "Survival"),
    ("vanilla-like", "Vanilla-like"),
    ("pvp", "PvP"),
    ("minigames", "Minigames"),
    ("skyblock", "Skyblock"),
    ("oneblock", "Oneblock"),
    ("creative-mode", "Creative"),
    ("technical", "Technical"),
    ("roleplay", "Roleplay"),
    ("rpg", "RPG"),
    ("pokemon", "Pokémon"),
    ("anarchy", "Anarchy"),
    ("custom-content", "Custom content"),
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Order {
    #[default]
    Relevance,
    Popular,
    Newest,
}

impl Order {
    pub const ALL: [Order; 3] = [Order::Relevance, Order::Popular, Order::Newest];
    fn index(self) -> &'static str {
        match self {
            Order::Relevance => "relevance",
            Order::Popular => "downloads",
            Order::Newest => "newest",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Order::Relevance => "Relevance",
            Order::Popular => "Popular",
            Order::Newest => "Newest",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ServerQuery {
    pub text: String,
    pub tags: Vec<String>,
    pub order: Order,
    pub offset: usize,
}

impl ServerQuery {
    pub const PAGE: usize = 20;

    fn params(&self) -> Vec<(&'static str, String)> {
        let mut facets = vec![vec!["project_types:minecraft_java_server".to_string()]];
        for tag in &self.tags {
            facets.push(vec![format!("categories:{tag}")]);
        }
        let mut params = vec![
            ("facets", serde_json::to_string(&facets).unwrap_or_default()),
            ("index", self.order.index().to_string()),
            ("limit", Self::PAGE.to_string()),
            ("offset", self.offset.to_string()),
        ];
        if !self.text.trim().is_empty() {
            params.push(("query", self.text.trim().to_string()));
        }
        params
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct ServerResults {
    pub hits: Vec<Server>,
    pub total_hits: usize,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Server {
    pub project_id: String,
    #[serde(default)]
    pub slug: Option<String>,
    pub name: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub minecraft_server: Option<ServerInfo>,
    #[serde(default)]
    pub minecraft_java_server: Option<JavaServer>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ServerInfo {
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub languages: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct JavaServer {
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub content: Option<Content>,
    #[serde(default)]
    pub ping: Option<Ping>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Content {
    Vanilla {
        #[serde(default)]
        supported_game_versions: Vec<String>,
        #[serde(default)]
        recommended_game_version: Option<String>,
    },
    Modpack {
        version_id: String,
        project_id: String,
        #[serde(default)]
        project_name: Option<String>,
        #[serde(default)]
        project_icon: Option<String>,
    },
    #[serde(other)]
    Other,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Ping {
    #[serde(default)]
    pub data: Option<PingData>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct PingData {
    #[serde(default)]
    pub players_online: u32,
    #[serde(default)]
    pub players_max: u32,
    #[serde(default)]
    pub version_name: Option<String>,
    #[serde(default)]
    pub latency: Option<Latency>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
pub struct Latency {
    pub secs: u64,
    pub nanos: u32,
}

impl Server {
    pub fn address(&self) -> Option<&str> {
        self.minecraft_java_server
            .as_ref()
            .map(|j| j.address.trim())
            .filter(|a| !a.is_empty())
    }

    pub fn content(&self) -> Option<&Content> {
        self.minecraft_java_server.as_ref()?.content.as_ref()
    }

    /// Players and latency from Modrinth's last check, if it answered.
    pub fn ping(&self) -> Option<&PingData> {
        self.minecraft_java_server
            .as_ref()?
            .ping
            .as_ref()?
            .data
            .as_ref()
    }

    pub fn latency_ms(&self) -> Option<u64> {
        self.ping()?
            .latency
            .map(|l| l.secs * 1000 + u64::from(l.nanos) / 1_000_000)
    }

    /// "Europe", "US East"...
    pub fn region(&self) -> Option<String> {
        let region = self.minecraft_server.as_ref()?.region.as_deref()?;
        Some(match region {
            "us_east" => "US East".into(),
            "us_west" => "US West".into(),
            "us_central" => "US Central".into(),
            other => other
                .split('_')
                .map(|w| {
                    let mut c = w.chars();
                    c.next()
                        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>()
                .join(" "),
        })
    }

    pub fn web_url(&self) -> String {
        format!(
            "https://modrinth.com/server/{}",
            self.slug.as_deref().unwrap_or(&self.project_id)
        )
    }
}

impl Client {
    pub async fn search_servers(&self, query: &ServerQuery) -> anyhow::Result<ServerResults> {
        let response = self
            .http()
            .get(SEARCH)
            .query(&query.params())
            .send()
            .await?
            .error_for_status()?;
        Ok(response.json().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_listings() {
        let results: ServerResults = serde_json::from_value(serde_json::json!({
            "total_hits": 2,
            "hits": [
                {
                    "project_id": "OgcNPs6n", "slug": "aero-server", "name": "Aero SMP",
                    "summary": "Create.", "categories": ["smp"],
                    "minecraft_server": {"region": "europe", "languages": ["en"]},
                    "minecraft_java_server": {
                        "address": "play.aerosmp.com",
                        "content": {"kind": "modpack", "version_id": "GDELFL7Y", "project_id": "OgcNPs6n", "project_name": "Aero SMP"},
                        "ping": {"data": {"latency": {"secs": 0, "nanos": 539268426}, "players_online": 14, "players_max": 300}}
                    }
                },
                {
                    "project_id": "v", "name": "Hive",
                    "minecraft_server": {"region": "us_east"},
                    "minecraft_java_server": {
                        "address": " hive.example ",
                        "content": {"kind": "vanilla", "supported_game_versions": ["26.2"], "recommended_game_version": "26.2"},
                        "ping": {"data": null}
                    }
                }
            ]
        }))
        .unwrap();
        let aero = &results.hits[0];
        assert_eq!(aero.address(), Some("play.aerosmp.com"));
        assert_eq!(aero.latency_ms(), Some(539));
        assert_eq!(aero.ping().unwrap().players_online, 14);
        assert_eq!(aero.region().as_deref(), Some("Europe"));
        assert!(
            matches!(aero.content(), Some(Content::Modpack { version_id, .. }) if version_id == "GDELFL7Y")
        );
        let hive = &results.hits[1];
        assert_eq!(hive.address(), Some("hive.example"));
        assert!(hive.ping().is_none());
        assert_eq!(hive.region().as_deref(), Some("US East"));
        assert!(
            matches!(hive.content(), Some(Content::Vanilla { recommended_game_version: Some(v), .. }) if v == "26.2")
        );
        assert_eq!(hive.web_url(), "https://modrinth.com/server/v");
    }

    #[test]
    fn query_has_the_server_facet_and_tags() {
        let q = ServerQuery {
            text: "  create ".into(),
            tags: vec!["smp".into()],
            order: Order::Popular,
            offset: 20,
        };
        let params = q.params();
        let get = |k: &str| {
            params
                .iter()
                .find(|(key, _)| *key == k)
                .map(|(_, v)| v.clone())
        };
        assert_eq!(
            get("facets").unwrap(),
            r#"[["project_types:minecraft_java_server"],["categories:smp"]]"#
        );
        assert_eq!(get("index").unwrap(), "downloads");
        assert_eq!(get("query").unwrap(), "create");
        assert_eq!(get("offset").unwrap(), "20");
    }
}
