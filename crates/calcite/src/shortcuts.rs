//! Desktop shortcuts that start an instance straight away
//! (`calcite play <id>`), the way each system expects them.

use std::path::{Path, PathBuf};

use anyhow::Context as _;

/// A file name that works on every system.
fn safe(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " -_().".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').to_string();
    if trimmed.is_empty() {
        "Minecraft".into()
    } else {
        trimmed
    }
}

/// Make a shortcut for `instance_id` on the desktop. Returns where it went.
pub fn create(name: &str, instance_id: &str, icon: Option<&Path>) -> anyhow::Result<PathBuf> {
    let exe = std::env::current_exe().context("couldn't find Calcite itself")?;
    let desktop = dirs::desktop_dir()
        .or_else(dirs::home_dir)
        .context("couldn't find your desktop")?;
    platform(&desktop, &safe(name), &exe, instance_id, icon)
}

#[cfg(target_os = "macos")]
fn platform(
    desktop: &Path,
    name: &str,
    exe: &Path,
    id: &str,
    icon: Option<&Path>,
) -> anyhow::Result<PathBuf> {
    // A tiny app bundle whose executable is a script, so it opens without
    // a Terminal window and can carry the instance's icon.
    let app = desktop.join(format!("{name}.app"));
    let macos = app.join("Contents/MacOS");
    let resources = app.join("Contents/Resources");
    std::fs::create_dir_all(&macos)?;
    std::fs::create_dir_all(&resources)?;
    let script = macos.join("launch");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nexec \"{}\" play \"{}\"\n",
            exe.display(),
            id.replace('"', "\\\"")
        ),
    )?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    let mut has_icon = false;
    if let Some(icon) = icon {
        has_icon = std::process::Command::new("sips")
            .args(["-s", "format", "icns"])
            .arg(icon)
            .arg("--out")
            .arg(resources.join("icon.icns"))
            .output()
            .is_ok_and(|o| o.status.success());
    }
    let bundle_id: String = id.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    std::fs::write(
        app.join("Contents/Info.plist"),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>launch</string>
<key>CFBundleName</key><string>{name}</string>
<key>CFBundleIdentifier</key><string>com.calcite.shortcut.{bundle_id}</string>
<key>CFBundlePackageType</key><string>APPL</string>
{icon}<key>LSUIElement</key><true/>
</dict></plist>
"#,
            name = xml(name),
            icon = if has_icon {
                "<key>CFBundleIconFile</key><string>icon</string>\n"
            } else {
                ""
            },
        ),
    )?;
    Ok(app)
}

#[cfg(target_os = "macos")]
fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(target_os = "windows")]
fn platform(
    desktop: &Path,
    name: &str,
    exe: &Path,
    id: &str,
    icon: Option<&Path>,
) -> anyhow::Result<PathBuf> {
    // A real .lnk, made with the Windows Script Host through PowerShell.
    let link = desktop.join(format!("{name}.lnk"));
    let quote = |s: &str| s.replace('\'', "''");
    let mut script = format!(
        "$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{}');$s.TargetPath='{}';$s.Arguments='play \"{}\"';",
        quote(&link.to_string_lossy()),
        quote(&exe.to_string_lossy()),
        quote(id),
    );
    if let Some(icon) = icon.filter(|p| p.extension().is_some_and(|e| e == "ico")) {
        script.push_str(&format!(
            "$s.IconLocation='{}';",
            quote(&icon.to_string_lossy())
        ));
    }
    script.push_str("$s.Save()");
    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .status()
        .context("couldn't run PowerShell to make the shortcut")?;
    anyhow::ensure!(status.success(), "PowerShell couldn't make the shortcut");
    Ok(link)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform(
    desktop: &Path,
    name: &str,
    exe: &Path,
    id: &str,
    icon: Option<&Path>,
) -> anyhow::Result<PathBuf> {
    // A .desktop entry in the app menu, and a copy on the desktop.
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName={name}\nComment=Play {name} with Calcite\nExec=\"{}\" play \"{}\"\nIcon={}\nCategories=Game;\nTerminal=false\n",
        exe.display(),
        id,
        icon.map(|i| i.display().to_string())
            .unwrap_or_else(|| "applications-games".into()),
    );
    let file = format!(
        "calcite-{}.desktop",
        id.chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
    );
    if let Some(apps) = dirs::data_dir().map(|d| d.join("applications")) {
        std::fs::create_dir_all(&apps)?;
        std::fs::write(apps.join(&file), &entry)?;
    }
    let on_desktop = desktop.join(&file);
    std::fs::write(&on_desktop, &entry)?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&on_desktop, std::fs::Permissions::from_mode(0o755))?;
    Ok(on_desktop)
}

#[cfg(test)]
mod tests {
    use super::safe;

    #[cfg(target_os = "macos")]
    #[test]
    fn makes_a_launchable_app_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let app = super::platform(
            dir.path(),
            "My Pack",
            std::path::Path::new("/Applications/Calcite.app/Contents/MacOS/calcite"),
            "local:abc",
            None,
        )
        .unwrap();
        assert_eq!(app.file_name().unwrap(), "My Pack.app");
        let script = std::fs::read_to_string(app.join("Contents/MacOS/launch")).unwrap();
        assert!(script.contains("play \"local:abc\""), "{script}");
        let plist = std::fs::read_to_string(app.join("Contents/Info.plist")).unwrap();
        assert!(plist.contains("<string>launch</string>") && plist.contains("My Pack"));
    }

    #[test]
    fn names_are_safe_everywhere() {
        assert_eq!(safe("Create: Above & Beyond"), "Create_ Above _ Beyond");
        assert_eq!(safe("../.."), "_");
        assert_eq!(safe("   "), "Minecraft");
        assert_eq!(safe("Fabric 1.21.4"), "Fabric 1.21.4");
    }
}
