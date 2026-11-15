//! Which game versions each mod loader supports, and which loader versions
//! go with a game version. Mirrors the rules Modrinth App uses with the same
//! launcher metadata.

use std::collections::HashMap;

use theseus::data::ModLoader;

/// Fabric-like manifests list every loader version under this placeholder.
const PLACEHOLDER: &str = "${modrinth.gameVersion}";

#[derive(Clone, Debug, PartialEq)]
pub struct LoaderVersion {
    pub id: String,
    pub stable: bool,
}

#[derive(Clone, Debug, Default)]
pub struct LoaderCatalog {
    /// Loader versions that work on every supported game version.
    universal: Option<Vec<LoaderVersion>>,
    /// Game version → its loader versions (or a version group).
    per_game: Vec<(String, Vec<LoaderVersion>, Option<String>)>,
    groups: HashMap<String, Vec<LoaderVersion>>,
}

impl LoaderCatalog {
    pub fn from_manifest(manifest: &theseus::metadata::Manifest) -> Self {
        let convert = |loaders: &[minecraft_meta_manifest::fabriclike::LoaderVersion]| {
            loaders
                .iter()
                .map(|l| LoaderVersion {
                    id: l.id.clone(),
                    stable: l.stable,
                })
                .collect::<Vec<_>>()
        };
        let mut catalog = LoaderCatalog::default();
        for gv in &manifest.game_versions {
            if gv.id == PLACEHOLDER {
                catalog.universal = Some(convert(&gv.loaders));
            } else {
                catalog.per_game.push((
                    gv.id.clone(),
                    convert(&gv.loaders),
                    gv.version_group.clone(),
                ));
            }
        }
        for group in &manifest.version_groups {
            catalog
                .groups
                .insert(group.id.clone(), convert(&group.loaders));
        }
        catalog
    }

    /// Game versions this loader can run.
    pub fn supports(&self, game_version: &str) -> bool {
        self.per_game.iter().any(|(id, loaders, group)| {
            id == game_version
                && (self.universal.is_some() || !loaders.is_empty() || group.is_some())
        })
    }

    /// Loader versions for a game version, newest first as listed.
    pub fn loaders_for(&self, game_version: &str) -> Vec<LoaderVersion> {
        let Some((_, loaders, group)) = self.per_game.iter().find(|(id, ..)| id == game_version)
        else {
            return Vec::new();
        };
        if let Some(universal) = &self.universal {
            return universal.clone();
        }
        if let Some(group) = group
            && let Some(loaders) = self.groups.get(group)
        {
            return loaders.clone();
        }
        loaders.clone()
    }

    /// The version to pick by default: the first stable one, else the first.
    pub fn default_loader(&self, game_version: &str) -> Option<LoaderVersion> {
        let all = self.loaders_for(game_version);
        all.iter()
            .find(|l| l.stable)
            .cloned()
            .or_else(|| all.first().cloned())
    }
}

/// The name Modrinth's metadata uses for each loader.
pub fn manifest_name(loader: ModLoader) -> Option<&'static str> {
    match loader {
        ModLoader::Vanilla => None,
        ModLoader::Fabric => Some("fabric"),
        ModLoader::Quilt => Some("quilt"),
        ModLoader::Forge => Some("forge"),
        ModLoader::NeoForge => Some("neo"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lv(id: &str, stable: bool) -> LoaderVersion {
        LoaderVersion {
            id: id.into(),
            stable,
        }
    }

    #[test]
    fn fabric_style_placeholder_applies_to_listed_versions() {
        let catalog = LoaderCatalog {
            universal: Some(vec![lv("0.18.0-beta", false), lv("0.17.2", true)]),
            per_game: vec![
                ("1.21.4".into(), vec![], None),
                ("26.3".into(), vec![], None),
            ],
            groups: HashMap::new(),
        };
        assert!(catalog.supports("26.3"));
        assert!(!catalog.supports("1.8.9"));
        assert_eq!(catalog.default_loader("1.21.4"), Some(lv("0.17.2", true)));
        assert!(catalog.loaders_for("1.8.9").is_empty());
    }

    #[test]
    fn forge_style_lists_per_game_version() {
        let catalog = LoaderCatalog {
            universal: None,
            per_game: vec![
                (
                    "1.21.1".into(),
                    vec![lv("52.0.3", false), lv("52.0.1", true)],
                    None,
                ),
                ("1.7.10".into(), vec![], None),
            ],
            groups: HashMap::new(),
        };
        assert!(catalog.supports("1.21.1"));
        assert!(!catalog.supports("1.7.10"));
        assert_eq!(catalog.default_loader("1.21.1").unwrap().id, "52.0.1");
    }

    #[test]
    fn version_groups_share_loaders() {
        let mut groups = HashMap::new();
        groups.insert("g1".to_string(), vec![lv("1.0", true)]);
        let catalog = LoaderCatalog {
            universal: None,
            per_game: vec![("1.20.1".into(), vec![], Some("g1".into()))],
            groups,
        };
        assert!(catalog.supports("1.20.1"));
        assert_eq!(catalog.loaders_for("1.20.1"), vec![lv("1.0", true)]);
    }
}
