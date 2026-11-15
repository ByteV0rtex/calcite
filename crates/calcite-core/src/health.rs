//! Instance health check: read every mod's own metadata and find problems
//! before the game does: duplicates, missing dependencies, declared
//! conflicts, and mods built for a different loader.
//!
//! Mods bundle libraries inside themselves (jar-in-jar), and those count as
//! present too, so nested jars are read as well.

use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read, Seek};
use std::path::Path;

use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LoaderKind {
    Fabric,
    Quilt,
    Forge,
    NeoForge,
}

impl LoaderKind {
    pub fn name(self) -> &'static str {
        match self {
            LoaderKind::Fabric => "Fabric",
            LoaderKind::Quilt => "Quilt",
            LoaderKind::Forge => "Forge",
            LoaderKind::NeoForge => "NeoForge",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Dependency {
    pub id: String,
    /// The version requirement as written, e.g. ">=0.16.0" or "*".
    pub range: String,
}

/// What a mod file says about itself.
#[derive(Clone, Debug, PartialEq)]
pub struct ModInfo {
    pub file: String,
    pub id: String,
    pub name: String,
    pub version: String,
    /// Loaders this file has metadata for.
    pub loaders: Vec<LoaderKind>,
    pub depends: Vec<Dependency>,
    pub breaks: Vec<Dependency>,
    /// Ids this file makes available: its own, `provides`, and everything
    /// bundled inside it.
    pub provided: Vec<String>,
    /// False for files turned off (`.jar.disabled`).
    pub enabled: bool,
}

/// Ids that are never separate mods.
const BUILT_IN: [&str; 12] = [
    "minecraft",
    "java",
    "fabricloader",
    "fabric-loader",
    "quilt_loader",
    "quilt_base",
    "forge",
    "neoforge",
    "javafml",
    "lowcodefml",
    "mixinextras",
    "fml",
];

// ------------------------------------------------------------ parsing

#[derive(Deserialize)]
struct FabricJson {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    version: String,
    #[serde(default)]
    depends: HashMap<String, serde_json::Value>,
    #[serde(default)]
    breaks: HashMap<String, serde_json::Value>,
    #[serde(default)]
    provides: Vec<serde_json::Value>,
    #[serde(default)]
    jars: Vec<FabricJar>,
}

#[derive(Deserialize)]
struct FabricJar {
    file: String,
}

fn range_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(items) => items
            .iter()
            .filter_map(|i| i.as_str())
            .collect::<Vec<_>>()
            .join(" || "),
        _ => "*".into(),
    }
}

fn deps(map: &HashMap<String, serde_json::Value>) -> Vec<Dependency> {
    let mut list: Vec<Dependency> = map
        .iter()
        .map(|(id, v)| Dependency {
            id: id.clone(),
            range: range_text(v),
        })
        .collect();
    list.sort_by(|a, b| a.id.cmp(&b.id));
    list
}

fn read_entry<R: Read + Seek>(zip: &mut zip::ZipArchive<R>, name: &str) -> Option<Vec<u8>> {
    let mut file = zip.by_name(name).ok()?;
    let mut bytes = Vec::with_capacity(file.size() as usize);
    file.read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// Read a mod from jar bytes. `depth` limits nested-jar recursion.
fn read_mod<R: Read + Seek>(
    zip: &mut zip::ZipArchive<R>,
    file: &str,
    depth: u8,
) -> Option<ModInfo> {
    let mut info: Option<ModInfo> = None;

    if let Some(bytes) = read_entry(zip, "fabric.mod.json")
        && let Ok(json) = serde_json::from_slice::<FabricJson>(&strip_bom(&bytes))
    {
        let mut provided = vec![json.id.clone()];
        provided.extend(
            json.provides
                .iter()
                .filter_map(|p| p.as_str().map(str::to_string)),
        );
        if depth < 3 {
            for jar in &json.jars {
                if let Some(inner) = read_entry(zip, &jar.file)
                    && let Ok(mut nested) = zip::ZipArchive::new(Cursor::new(inner))
                    && let Some(nested) = read_mod(&mut nested, &jar.file, depth + 1)
                {
                    provided.extend(nested.provided);
                }
            }
        }
        info = Some(ModInfo {
            file: file.to_string(),
            name: json.name.unwrap_or_else(|| json.id.clone()),
            id: json.id,
            version: json.version,
            loaders: vec![LoaderKind::Fabric],
            depends: deps(&json.depends),
            breaks: deps(&json.breaks),
            provided,
            enabled: true,
        });
    }

    if let Some(bytes) = read_entry(zip, "quilt.mod.json") {
        if let Some(info) = info.as_mut() {
            info.loaders.push(LoaderKind::Quilt);
        } else if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&strip_bom(&bytes)) {
            let loader = &json["quilt_loader"];
            let id = loader["id"].as_str().unwrap_or_default().to_string();
            if !id.is_empty() {
                let depends = loader["depends"]
                    .as_array()
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|d| {
                                d.as_str()
                                    .map(|id| Dependency {
                                        id: id.into(),
                                        range: "*".into(),
                                    })
                                    .or_else(|| {
                                        let optional = d["optional"].as_bool().unwrap_or(false);
                                        (!optional).then(|| Dependency {
                                            id: d["id"].as_str().unwrap_or_default().to_string(),
                                            range: d["versions"]
                                                .as_str()
                                                .unwrap_or("*")
                                                .to_string(),
                                        })
                                    })
                            })
                            .filter(|d| !d.id.is_empty())
                            .collect()
                    })
                    .unwrap_or_default();
                info = Some(ModInfo {
                    file: file.to_string(),
                    name: loader["metadata"]["name"]
                        .as_str()
                        .unwrap_or(&id)
                        .to_string(),
                    version: loader["version"].as_str().unwrap_or_default().to_string(),
                    loaders: vec![LoaderKind::Quilt],
                    depends,
                    breaks: Vec::new(),
                    provided: vec![id.clone()],
                    enabled: true,
                    id,
                });
            }
        }
    }

    for (path, kind) in [
        ("META-INF/neoforge.mods.toml", LoaderKind::NeoForge),
        ("META-INF/mods.toml", LoaderKind::Forge),
    ] {
        let Some(bytes) = read_entry(zip, path) else {
            continue;
        };
        if let Some(info) = info.as_mut() {
            if !info.loaders.contains(&kind) {
                info.loaders.push(kind);
            }
            continue;
        }
        if let Some(parsed) = parse_forge_toml(&String::from_utf8_lossy(&bytes), file, kind) {
            let mut parsed = parsed;
            // Forge jar-in-jar libraries live in META-INF/jarjar.
            if depth < 3 {
                let nested: Vec<String> = zip
                    .file_names()
                    .filter_map(|n| n.ok().map(|n| n.to_string()))
                    .filter(|n| n.starts_with("META-INF/jarjar/") && n.ends_with(".jar"))
                    .collect();
                for name in nested {
                    if let Some(inner) = read_entry(zip, &name)
                        && let Ok(mut nested) = zip::ZipArchive::new(Cursor::new(inner))
                        && let Some(nested) = read_mod(&mut nested, &name, depth + 1)
                    {
                        parsed.provided.extend(nested.provided);
                    }
                }
            }
            info = Some(parsed);
        }
    }
    info
}

fn parse_forge_toml(text: &str, file: &str, kind: LoaderKind) -> Option<ModInfo> {
    let value: toml::Value = toml::from_str(text).ok()?;
    let first = value.get("mods")?.as_array()?.first()?;
    let id = first.get("modId")?.as_str()?.to_string();
    let deps_table = value
        .get("dependencies")
        .and_then(|d| d.get(&id))
        .and_then(|d| d.as_array());
    let mut depends = Vec::new();
    let mut breaks = Vec::new();
    for dep in deps_table.into_iter().flatten() {
        let dep_id = dep
            .get("modId")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let range = dep
            .get("versionRange")
            .and_then(|v| v.as_str())
            .unwrap_or("*")
            .to_string();
        let kind = dep
            .get("type")
            .and_then(|v| v.as_str())
            .map(str::to_ascii_lowercase);
        let mandatory = dep.get("mandatory").and_then(|v| v.as_bool());
        let entry = Dependency { id: dep_id, range };
        match (kind.as_deref(), mandatory) {
            (Some("incompatible"), _) => breaks.push(entry),
            (Some("required"), _) | (None, Some(true)) => depends.push(entry),
            _ => {}
        }
    }
    let version = first
        .get("version")
        .and_then(|v| v.as_str())
        .filter(|v| !v.contains("${"))
        .unwrap_or_default()
        .to_string();
    Some(ModInfo {
        file: file.to_string(),
        name: first
            .get("displayName")
            .and_then(|v| v.as_str())
            .unwrap_or(&id)
            .to_string(),
        version,
        loaders: vec![kind],
        depends,
        breaks,
        provided: vec![id.clone()],
        enabled: true,
        id,
    })
}

fn strip_bom(bytes: &[u8]) -> Vec<u8> {
    bytes
        .strip_prefix(&[0xEF, 0xBB, 0xBF])
        .unwrap_or(bytes)
        .to_vec()
}

/// Read the mod metadata of one jar file (`.jar` or `.jar.disabled`).
/// Jars without mod metadata are treated as libraries named after their
/// file (e.g. `kotlinforforge-4.11.0-all.jar` provides `kotlinforforge`).
pub fn read_jar(path: &Path) -> Option<ModInfo> {
    let file = std::fs::File::open(path).ok()?;
    let mut zip = zip::ZipArchive::new(file).ok()?;
    let name = path.file_name()?.to_string_lossy().to_string();
    let enabled = !name.ends_with(".disabled");
    if let Some(mut info) = read_mod(&mut zip, &name, 0) {
        info.enabled = enabled;
        return Some(info);
    }
    let id = library_id(&name)?;
    Some(ModInfo {
        file: name,
        name: id.clone(),
        version: String::new(),
        loaders: Vec::new(),
        depends: Vec::new(),
        breaks: Vec::new(),
        provided: vec![id.clone()],
        enabled,
        id,
    })
}

/// "kotlinforforge-4.11.0-all.jar" → "kotlinforforge".
fn library_id(file: &str) -> Option<String> {
    let stem = file.trim_end_matches(".disabled").trim_end_matches(".jar");
    let mut prefix = String::new();
    for (i, part) in stem.split(['-', '_']).enumerate() {
        if part.chars().next().is_some_and(|c| c.is_ascii_digit())
            || (i > 0 && part.starts_with("mc"))
        {
            break;
        }
        if !prefix.is_empty() {
            prefix.push('-');
        }
        prefix.push_str(part);
    }
    let id = prefix.to_ascii_lowercase();
    (!id.is_empty()).then_some(id)
}

/// Read every mod in a mods folder, including turned-off ones.
pub fn read_mods_folder(dir: &Path) -> Vec<ModInfo> {
    let mut mods: Vec<ModInfo> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            name.ends_with(".jar") || name.ends_with(".jar.disabled")
        })
        .filter_map(|p| read_jar(&p))
        .collect();
    mods.sort_by(|a, b| a.file.cmp(&b.file));
    mods
}

// ------------------------------------------------------------ versions

/// Compare dotted versions numerically; pre-releases sort before releases.
pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    fn parse(v: &str) -> (Vec<u64>, bool) {
        let core = v.split('+').next().unwrap_or(v);
        let (num, pre) = match core.split_once('-') {
            Some((n, _)) => (n, true),
            None => (core, false),
        };
        let parts = num
            .split('.')
            .map(|p| {
                p.chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
            })
            .map(|p| p.parse().unwrap_or(0))
            .collect();
        (parts, pre)
    }
    let (pa, pre_a) = parse(a);
    let (pb, pre_b) = parse(b);
    let len = pa.len().max(pb.len());
    for i in 0..len {
        let (x, y) = (
            pa.get(i).copied().unwrap_or(0),
            pb.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x.cmp(&y);
        }
    }
    // 1.0-beta < 1.0
    pre_b.cmp(&pre_a)
}

/// Whether `version` falls in a Fabric-style range ("*", ">=1.2", "<=2.0",
/// "1.2.x", "a || b", space-separated AND). Unknown syntax returns `None`.
pub fn in_range(version: &str, range: &str) -> Option<bool> {
    let range = range.trim();
    if range.is_empty() || range == "*" {
        return Some(true);
    }
    if range.contains("||") {
        let mut any_known = false;
        for part in range.split("||") {
            match in_range(version, part) {
                Some(true) => return Some(true),
                Some(false) => any_known = true,
                None => {}
            }
        }
        return any_known.then_some(false);
    }
    for part in range.split_whitespace() {
        let (op, target) = ["<=", ">=", "<", ">", "=", "~", "^"]
            .iter()
            .find_map(|op| part.strip_prefix(op).map(|t| (*op, t)))
            .unwrap_or(("", part));
        if target.contains('[') || target.contains('(') {
            return None; // Maven ranges (Forge) aren't evaluated.
        }
        use std::cmp::Ordering::*;
        let ok = if let Some(prefix) = target.strip_suffix(".x") {
            version == prefix || version.starts_with(&format!("{prefix}."))
        } else {
            let cmp = compare_versions(version, target);
            match op {
                "<=" => cmp != Greater,
                ">=" | "^" | "~" => cmp != Less,
                "<" => cmp == Less,
                ">" => cmp == Greater,
                _ => cmp == Equal,
            }
        };
        if !ok {
            return Some(false);
        }
    }
    Some(true)
}

// ------------------------------------------------------------ checks

#[derive(Clone, Debug, PartialEq)]
pub enum Issue {
    /// Two or more files are the same mod.
    Duplicate {
        id: String,
        name: String,
        files: Vec<String>,
    },
    /// A mod needs another mod that isn't installed.
    MissingDependency {
        file: String,
        name: String,
        needs: String,
        range: String,
    },
    /// A mod needs another mod that is installed but turned off.
    DisabledDependency {
        file: String,
        name: String,
        needs_name: String,
        needs_file: String,
    },
    /// A mod declares it doesn't work with another installed mod.
    Conflict {
        file: String,
        name: String,
        other_file: String,
        other_name: String,
        range: String,
    },
    /// A mod is for a different loader than the instance runs.
    WrongLoader {
        file: String,
        name: String,
        made_for: Vec<LoaderKind>,
    },
}

impl Issue {
    pub fn title(&self) -> String {
        match self {
            Issue::Duplicate { name, files, .. } => {
                format!("{name} is installed {} times", files.len())
            }
            Issue::MissingDependency { name, needs, .. } => format!("{name} needs {needs}"),
            Issue::DisabledDependency {
                name, needs_name, ..
            } => {
                format!("{name} needs {needs_name}, which is turned off")
            }
            Issue::Conflict {
                name, other_name, ..
            } => format!("{name} doesn't work with {other_name}"),
            Issue::WrongLoader { name, made_for, .. } => format!(
                "{name} is made for {}",
                made_for
                    .iter()
                    .map(|l| l.name())
                    .collect::<Vec<_>>()
                    .join(" or ")
            ),
        }
    }
}

/// Find problems in the enabled mods of an instance running `loader`.
pub fn check(mods: &[ModInfo], loader: LoaderKind) -> Vec<Issue> {
    let mut issues = Vec::new();

    let (enabled, disabled): (Vec<&ModInfo>, Vec<&ModInfo>) = mods.iter().partition(|m| m.enabled);

    // Wrong loader. Quilt runs Fabric mods, and so do Forge and NeoForge
    // when Sinytra Connector is installed. Libraries (no metadata) run anywhere.
    let connector = matches!(loader, LoaderKind::Forge | LoaderKind::NeoForge)
        && enabled.iter().any(|m| m.id == "connector");
    let runs = |m: &ModInfo| {
        m.loaders.is_empty()
            || m.loaders.contains(&loader)
            || ((loader == LoaderKind::Quilt || connector)
                && m.loaders.contains(&LoaderKind::Fabric))
    };
    let (usable, foreign): (Vec<&ModInfo>, Vec<&ModInfo>) =
        enabled.into_iter().partition(|m| runs(m));
    for m in foreign {
        issues.push(Issue::WrongLoader {
            file: m.file.clone(),
            name: m.name.clone(),
            made_for: m.loaders.clone(),
        });
    }

    // Duplicates (by mod id, among real mods this loader runs).
    let mut by_id: HashMap<&str, Vec<&ModInfo>> = HashMap::new();
    for m in usable.iter().filter(|m| !m.loaders.is_empty()) {
        by_id.entry(m.id.as_str()).or_default().push(m);
    }
    let mut dup_ids: Vec<&&str> = by_id
        .iter()
        .filter(|(_, v)| v.len() > 1)
        .map(|(k, _)| k)
        .collect();
    dup_ids.sort();
    for id in dup_ids {
        let list = &by_id[*id];
        issues.push(Issue::Duplicate {
            id: id.to_string(),
            name: list[0].name.clone(),
            files: list.iter().map(|m| m.file.clone()).collect(),
        });
    }

    // Everything present, including bundled libraries. Ids are compared with
    // '-' and '_' treated alike (Forgified Fabric API names itself
    // `fabric_api`, and Connector maps ids the same way).
    let norm = |id: &str| id.to_ascii_lowercase().replace('-', "_");
    let provided_owned: HashSet<String> = usable
        .iter()
        .flat_map(|m| m.provided.iter().map(|p| norm(p)))
        .collect();
    let provided = |id: &str| provided_owned.contains(&norm(id));
    let version_of: HashMap<&str, &str> = usable
        .iter()
        .map(|m| (m.id.as_str(), m.version.as_str()))
        .collect();

    for m in &usable {
        for dep in &m.depends {
            let id = dep.id.as_str();
            let has_fabric_api = provided("fabric-api");
            if BUILT_IN.contains(&id)
                || (id.starts_with("fabric-") || id.starts_with("fabric_")) && has_fabric_api
            {
                continue;
            }
            if !provided(id) && !(id == "fabric" && has_fabric_api) {
                if let Some(off) = disabled
                    .iter()
                    .find(|d| d.provided.iter().any(|p| norm(p) == norm(id)))
                {
                    issues.push(Issue::DisabledDependency {
                        file: m.file.clone(),
                        name: m.name.clone(),
                        needs_name: off.name.clone(),
                        needs_file: off.file.clone(),
                    });
                    continue;
                }
                issues.push(Issue::MissingDependency {
                    file: m.file.clone(),
                    name: m.name.clone(),
                    needs: dep.id.clone(),
                    range: dep.range.clone(),
                });
            }
        }
        for brk in &m.breaks {
            let Some(other) = usable.iter().find(|o| o.id == brk.id && o.file != m.file) else {
                continue;
            };
            let version = version_of.get(brk.id.as_str()).copied().unwrap_or("");
            // Only report conflicts we can actually confirm.
            if in_range(version, &brk.range) == Some(true) {
                issues.push(Issue::Conflict {
                    file: m.file.clone(),
                    name: m.name.clone(),
                    other_file: other.file.clone(),
                    other_name: other.name.clone(),
                    range: brk.range.clone(),
                });
            }
        }
    }
    issues
}

/// A version requirement in words: "[16.0.0,)" or ">=16.0.0" → "16.0.0 or newer".
pub fn describe_range(range: &str) -> Option<String> {
    let r = range.trim();
    if r.is_empty() || r == "*" {
        return None;
    }
    if let Some(inner) = r.strip_prefix('[').or_else(|| r.strip_prefix('(')) {
        let inner = inner.trim_end_matches([']', ')']);
        return Some(match inner.split_once(',') {
            Some((low, "")) => format!("{low} or newer"),
            Some(("", high)) => format!("older than {high}"),
            Some((low, high)) => format!("{low} up to {high}"),
            None => inner.to_string(),
        });
    }
    if let Some(v) = r.strip_prefix(">=") {
        return Some(format!("{} or newer", v.trim()));
    }
    if let Some(v) = r.strip_prefix("<=") {
        return Some(format!("{} or older", v.trim()));
    }
    Some(r.to_string())
}

/// Memory to suggest for a mod count on a machine with `system_mb` of RAM.
pub fn recommended_memory_mb(mod_count: usize, system_mb: u64) -> u32 {
    let wanted = match mod_count {
        0..=10 => 2048,
        11..=60 => 3072,
        61..=150 => 4096,
        151..=250 => 6144,
        _ => 8192,
    };
    // Never more than half the machine, never less than 2 GB.
    let cap = (system_mb / 2).max(2048) as u32;
    wanted.min(cap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn jar(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let opts = zip::write::SimpleFileOptions::default();
            for (name, bytes) in entries {
                w.start_file(*name, opts).unwrap();
                w.write_all(bytes).unwrap();
            }
            w.finish().unwrap();
        }
        buf.into_inner()
    }

    fn fabric(id: &str, version: &str, extra: serde_json::Value) -> Vec<u8> {
        let mut json =
            serde_json::json!({ "schemaVersion": 1, "id": id, "version": version, "name": id });
        if let (Some(obj), Some(more)) = (json.as_object_mut(), extra.as_object()) {
            for (k, v) in more {
                obj.insert(k.clone(), v.clone());
            }
        }
        jar(&[("fabric.mod.json", serde_json::to_vec(&json).unwrap())])
    }

    fn write(dir: &Path, name: &str, bytes: Vec<u8>) {
        std::fs::write(dir.join(name), bytes).unwrap();
    }

    #[test]
    fn versions_and_ranges() {
        use std::cmp::Ordering::*;
        assert_eq!(compare_versions("1.21.10", "1.21.9"), Greater);
        assert_eq!(compare_versions("0.9.3-beta.5+mc26.3", "0.9.3"), Less);
        assert_eq!(in_range("1.11.7", "<=1.12.0"), Some(true));
        assert_eq!(in_range("1.13.0", "<=1.12.0"), Some(false));
        assert_eq!(in_range("26.3", "26.3.x"), Some(true));
        assert_eq!(in_range("0.17.2", ">=0.16.0 <1.0.0"), Some(true));
        assert_eq!(in_range("2.0", "1.x || >=2.0"), Some(true));
        assert_eq!(in_range("47.1", "[47,)"), None);
    }

    #[test]
    fn finds_real_problems_and_no_false_alarms() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        // Sodium needs a Fabric API module it bundles itself, and breaks old Iris.
        let nested = fabric("fabric-block-getter-api-v2", "2.0.9", serde_json::json!({}));
        let sodium_json = serde_json::json!({
            "schemaVersion": 1, "id": "sodium", "version": "0.9.3", "name": "Sodium",
            "depends": { "minecraft": "26.3.x", "fabricloader": ">=0.16.0", "fabric-block-getter-api-v2": "*" },
            "breaks": { "iris": "<=1.12.0", "canvas": "*" },
            "jars": [{ "file": "META-INF/jars/getter.jar" }]
        });
        write(
            dir,
            "sodium.jar",
            jar(&[
                ("fabric.mod.json", serde_json::to_vec(&sodium_json).unwrap()),
                ("META-INF/jars/getter.jar", nested),
            ]),
        );
        write(
            dir,
            "iris-old.jar",
            fabric(
                "iris",
                "1.11.7",
                serde_json::json!({ "name": "Iris Shaders" }),
            ),
        );
        // Needs a library that isn't installed.
        write(
            dir,
            "cobblemon.jar",
            fabric(
                "cobblemon",
                "1.6",
                serde_json::json!({
                    "name": "Cobblemon", "depends": { "architectury": ">=13" }
                }),
            ),
        );
        // Same mod twice.
        write(
            dir,
            "lithium-a.jar",
            fabric(
                "lithium",
                "0.17.0",
                serde_json::json!({ "name": "Lithium" }),
            ),
        );
        write(
            dir,
            "lithium-b.jar",
            fabric(
                "lithium",
                "0.17.1",
                serde_json::json!({ "name": "Lithium" }),
            ),
        );
        // A Forge-only mod.
        write(dir, "create.jar", jar(&[(
            "META-INF/mods.toml",
            b"[[mods]]\nmodId=\"create\"\ndisplayName=\"Create\"\nversion=\"0.5.1\"\n[[dependencies.create]]\nmodId=\"flywheel\"\nmandatory=true\nversionRange=\"[0.6,)\"\n".to_vec(),
        )]));
        // Not a mod at all.
        write(dir, "notes.jar", jar(&[("readme.txt", b"hi".to_vec())]));

        let mods = read_mods_folder(dir);
        // Six mods plus notes.jar, which counts as a library.
        assert_eq!(mods.len(), 7, "{mods:#?}");
        let issues = check(&mods, LoaderKind::Fabric);
        let titles: Vec<String> = issues.iter().map(Issue::title).collect();
        assert!(
            titles.contains(&"Create is made for Forge".to_string()),
            "{titles:?}"
        );
        assert!(
            titles.contains(&"Lithium is installed 2 times".to_string()),
            "{titles:?}"
        );
        assert!(
            titles.contains(&"Cobblemon needs architectury".to_string()),
            "{titles:?}"
        );
        assert!(
            titles.contains(&"Sodium doesn't work with Iris Shaders".to_string()),
            "{titles:?}"
        );
        // The bundled Fabric API module and built-ins are not reported.
        assert_eq!(issues.len(), 4, "{titles:?}");
    }

    #[test]
    fn libraries_connector_and_turned_off_mods() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        // Kotlin for Forge ships as a library without mods.toml.
        write(
            dir,
            "kotlinforforge-4.11.0-all.jar",
            jar(&[("thedarkcolour/Kff.class", b"x".to_vec())]),
        );
        write(dir, "slice.jar", jar(&[(
            "META-INF/mods.toml",
            b"[[mods]]\nmodId=\"sliceanddice\"\ndisplayName=\"Slice\"\n[[dependencies.sliceanddice]]\nmodId=\"kotlinforforge\"\nmandatory=true\n".to_vec(),
        )]));
        // Connector lets this Forge instance run Fabric mods and Fabric API.
        write(
            dir,
            "connector.jar",
            jar(&[(
                "META-INF/mods.toml",
                b"[[mods]]\nmodId=\"connector\"\n".to_vec(),
            )]),
        );
        write(
            dir,
            "fabric-api.jar",
            fabric("fabric-api", "0.92", serde_json::json!({})),
        );
        write(
            dir,
            "continuity.jar",
            fabric(
                "continuity",
                "3.0",
                serde_json::json!({ "depends": { "fabric-api": "*" } }),
            ),
        );
        // A dependency that's only turned off.
        write(
            dir,
            "vulkanmod.jar.disabled",
            fabric(
                "vulkanmod",
                "0.5",
                serde_json::json!({ "name": "VulkanMod" }),
            ),
        );
        write(
            dir,
            "nev.jar",
            fabric(
                "nev",
                "1.3",
                serde_json::json!({ "name": "Not Enough Vulkan", "depends": { "vulkanmod": "*" } }),
            ),
        );

        let mods = read_mods_folder(dir);
        let titles: Vec<String> = check(&mods, LoaderKind::Forge)
            .iter()
            .map(Issue::title)
            .collect();
        assert_eq!(
            titles,
            vec!["Not Enough Vulkan needs VulkanMod, which is turned off"]
        );
    }

    #[test]
    fn library_ids_come_from_file_names() {
        assert_eq!(
            library_id("kotlinforforge-4.11.0-all.jar").as_deref(),
            Some("kotlinforforge")
        );
        assert_eq!(
            library_id("cloth_config-mc1.20.jar").as_deref(),
            Some("cloth-config")
        );
    }

    #[test]
    fn quilt_runs_fabric_mods() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "a.jar",
            fabric("a", "1.0", serde_json::json!({})),
        );
        let mods = read_mods_folder(tmp.path());
        assert!(check(&mods, LoaderKind::Quilt).is_empty());
        assert_eq!(check(&mods, LoaderKind::NeoForge).len(), 1);
    }

    #[test]
    fn ranges_in_words() {
        assert_eq!(
            describe_range("[16.0.0,)").as_deref(),
            Some("16.0.0 or newer")
        );
        assert_eq!(describe_range(">=13").as_deref(), Some("13 or newer"));
        assert_eq!(
            describe_range("[1.0,2.0)").as_deref(),
            Some("1.0 up to 2.0")
        );
        assert_eq!(describe_range("*"), None);
    }

    #[test]
    fn memory_advice_respects_the_machine() {
        assert_eq!(recommended_memory_mb(5, 16384), 2048);
        assert_eq!(recommended_memory_mb(120, 16384), 4096);
        assert_eq!(recommended_memory_mb(300, 8192), 4096);
    }
}
