//! Files dropped onto Calcite: work out what each one is, and bring worlds
//! into an instance. Zips are judged by what's inside, not their name.

use std::fs::File;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dropped {
    /// A Modrinth modpack: becomes its own instance.
    Modpack,
    /// A CurseForge modpack export (manifest.json + overrides).
    CurseForgePack,
    Mod,
    ResourcePack,
    ShaderPack,
    DataPack,
    World,
    Unknown,
}

impl Dropped {
    /// Goes into an instance (rather than becoming one).
    pub fn needs_instance(&self) -> bool {
        matches!(
            self,
            Dropped::Mod
                | Dropped::ResourcePack
                | Dropped::ShaderPack
                | Dropped::DataPack
                | Dropped::World
        )
    }

    pub fn label(&self) -> &'static str {
        match self {
            Dropped::Modpack => "modpack",
            Dropped::CurseForgePack => "CurseForge modpack",
            Dropped::Mod => "mod",
            Dropped::ResourcePack => "resource pack",
            Dropped::ShaderPack => "shader pack",
            Dropped::DataPack => "data pack",
            Dropped::World => "world",
            Dropped::Unknown => "file",
        }
    }
}

/// What a dropped file or folder is.
pub fn classify(path: &Path) -> Dropped {
    if path.is_dir() {
        return if path.join("level.dat").is_file() {
            Dropped::World
        } else {
            Dropped::Unknown
        };
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_lowercase();
    match ext.as_str() {
        "mrpack" => Dropped::Modpack,
        "jar" | "litemod" => Dropped::Mod,
        "zip" => classify_zip(path).unwrap_or(Dropped::Unknown),
        _ => Dropped::Unknown,
    }
}

fn classify_zip(path: &Path) -> anyhow::Result<Dropped> {
    let archive = zip::ZipArchive::new(File::open(path)?)?;
    let names: Vec<String> = archive
        .file_names()
        .filter_map(|n| n.ok())
        .map(|n| n.replace('\\', "/"))
        .collect();
    let has = |name: &str| names.iter().any(|n| n == name);
    let depth = |n: &str| n.trim_end_matches('/').matches('/').count();
    if has("modrinth.index.json") {
        return Ok(Dropped::Modpack);
    }
    if has("manifest.json") && names.iter().any(|n| n.starts_with("overrides/")) {
        return Ok(Dropped::CurseForgePack);
    }
    if names
        .iter()
        .any(|n| n.ends_with("level.dat") && depth(n) <= 1 && !n.contains("__MACOSX"))
    {
        return Ok(Dropped::World);
    }
    if names
        .iter()
        .any(|n| (n.starts_with("shaders/") || n.contains("/shaders/")) && depth(n) <= 2)
    {
        return Ok(Dropped::ShaderPack);
    }
    if has("pack.mcmeta") {
        return Ok(if names.iter().any(|n| n.starts_with("data/")) {
            Dropped::DataPack
        } else {
            Dropped::ResourcePack
        });
    }
    if has("fabric.mod.json") || has("META-INF/mods.toml") || has("quilt.mod.json") {
        return Ok(Dropped::Mod);
    }
    Ok(Dropped::Unknown)
}

/// A folder name in `saves` that isn't taken: "World", "World (2)"…
fn free_name(saves: &Path, wanted: &str) -> String {
    let base = wanted.trim().trim_matches('.');
    let base = if base.is_empty() { "World" } else { base };
    if !saves.join(base).exists() {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base} ({n})"))
        .find(|name| !saves.join(name).exists())
        .unwrap_or_else(|| base.to_string())
}

/// Copy a world (a folder, or a zip holding one) into `saves`. Returns the
/// folder name it got. Never overwrites a world that's already there.
pub fn import_world(source: &Path, saves: &Path) -> anyhow::Result<String> {
    std::fs::create_dir_all(saves)?;
    if source.is_dir() {
        let wanted = source
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("World");
        let name = free_name(saves, wanted);
        copy_dir(source, &saves.join(&name))?;
        return Ok(name);
    }
    let mut archive = zip::ZipArchive::new(File::open(source)?)?;
    // Where level.dat sits decides what to strip: "MyWorld/level.dat" or "level.dat".
    let level = archive
        .file_names()
        .filter_map(|n| n.ok())
        .map(|n| n.replace('\\', "/"))
        .filter(|n| n.ends_with("level.dat") && !n.contains("__MACOSX"))
        .min_by_key(|n| n.len())
        .context("that zip has no level.dat, so it isn't a world")?;
    let prefix = level.trim_end_matches("level.dat").to_string();
    let wanted = prefix
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            source
                .file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "World".into());
    let name = free_name(saves, &wanted);
    let target = saves.join(&name);
    for ix in 0..archive.len() {
        let mut entry = archive.by_index(ix)?;
        // enclosed_name refuses "../" tricks.
        let Some(inner) = entry.enclosed_name() else {
            continue;
        };
        let inner = inner.to_string_lossy().replace('\\', "/");
        let Some(rest) = inner.strip_prefix(&prefix) else {
            continue;
        };
        if rest.is_empty() || rest.starts_with("__MACOSX") {
            continue;
        }
        let out = target.join(rest);
        if !out.starts_with(&target) {
            bail!("refusing to unpack outside the world folder");
        }
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
        } else {
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut file = File::create(&out)?;
            let mut buffer = Vec::new();
            entry.read_to_end(&mut buffer)?;
            io::Write::write_all(&mut file, &buffer)?;
        }
    }
    Ok(name)
}

fn copy_dir(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target: PathBuf = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn zip_with(dir: &Path, name: &str, files: &[&str]) -> PathBuf {
        let path = dir.join(name);
        let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
        for file in files {
            if file.ends_with('/') {
                zip.add_directory(*file, zip::write::SimpleFileOptions::default())
                    .unwrap();
            } else {
                zip.start_file(*file, zip::write::SimpleFileOptions::default())
                    .unwrap();
                zip.write_all(b"x").unwrap();
            }
        }
        zip.finish().unwrap();
        path
    }

    #[test]
    fn tells_zips_apart_by_contents() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        assert_eq!(
            classify(&zip_with(d, "a.zip", &["modrinth.index.json"])),
            Dropped::Modpack
        );
        assert_eq!(
            classify(&zip_with(
                d,
                "b.zip",
                &["manifest.json", "overrides/config/x.json"]
            )),
            Dropped::CurseForgePack
        );
        assert_eq!(
            classify(&zip_with(
                d,
                "c.zip",
                &["My World/level.dat", "My World/region/r.0.0.mca"]
            )),
            Dropped::World
        );
        assert_eq!(
            classify(&zip_with(
                d,
                "d.zip",
                &["shaders/composite.fsh", "shaders/lang/en_us.lang"]
            )),
            Dropped::ShaderPack
        );
        assert_eq!(
            classify(&zip_with(
                d,
                "e.zip",
                &["pack.mcmeta", "assets/minecraft/textures/x.png"]
            )),
            Dropped::ResourcePack
        );
        assert_eq!(
            classify(&zip_with(
                d,
                "f.zip",
                &["pack.mcmeta", "data/ns/function/x.mcfunction"]
            )),
            Dropped::DataPack
        );
        assert_eq!(
            classify(&zip_with(d, "g.zip", &["readme.txt"])),
            Dropped::Unknown
        );
        assert_eq!(classify(Path::new("mod.jar")), Dropped::Mod);
        assert_eq!(classify(Path::new("pack.mrpack")), Dropped::Modpack);
    }

    #[test]
    fn imports_worlds_without_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        let saves = dir.path().join("saves");
        let world = zip_with(
            dir.path(),
            "w.zip",
            &[
                "Skyblock/",
                "Skyblock/level.dat",
                "Skyblock/region/r.0.0.mca",
                "__MACOSX/._x",
            ],
        );
        assert_eq!(import_world(&world, &saves).unwrap(), "Skyblock");
        assert!(saves.join("Skyblock/region/r.0.0.mca").is_file());
        // Same world again gets a new folder.
        assert_eq!(import_world(&world, &saves).unwrap(), "Skyblock (2)");

        // A zip with level.dat at the top uses the zip's name.
        let flat = zip_with(
            dir.path(),
            "Creative Plot.zip",
            &["level.dat", "data/raids.dat"],
        );
        assert_eq!(import_world(&flat, &saves).unwrap(), "Creative Plot");
        assert!(saves.join("Creative Plot/data/raids.dat").is_file());

        // A folder is copied.
        let folder = dir.path().join("Hardcore");
        std::fs::create_dir_all(folder.join("region")).unwrap();
        std::fs::write(folder.join("level.dat"), b"x").unwrap();
        assert_eq!(classify(&folder), Dropped::World);
        assert_eq!(import_world(&folder, &saves).unwrap(), "Hardcore");
        assert!(saves.join("Hardcore/level.dat").is_file());
    }
}
