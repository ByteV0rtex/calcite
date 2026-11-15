//! Embedded assets: Calcite's icons, brand marks and fonts.

use std::borrow::Cow;

use gpui_kit::{App, AssetSource, Result, SharedString};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "assets"]
#[include = "icons/*.svg"]
#[include = "brand/*.svg"]
#[include = "fonts/*.ttf"]
struct Embedded;

/// Serves Calcite's own assets first, then gpui-component's default icon set
/// (used internally by stock controls such as selects and dialogs).
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(file) = Embedded::get(path) {
            return Ok(Some(file.data));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut entries: Vec<SharedString> = Embedded::iter()
            .filter(|p| p.starts_with(path))
            .map(|p| SharedString::from(p.into_owned()))
            .collect();
        entries.extend(gpui_kit::assets::Assets.list(path)?);
        Ok(entries)
    }
}

/// Register the bundled UI and monospace fonts with the text system.
pub fn load_fonts(cx: &App) -> anyhow::Result<()> {
    let fonts: Vec<Cow<'static, [u8]>> = Embedded::iter()
        .filter(|p| p.ends_with(".ttf"))
        .filter_map(|p| Embedded::get(&p).map(|f| f.data))
        .collect();
    cx.text_system().add_fonts(fonts)?;
    Ok(())
}

/// An icon path inside the embedded `icons/` folder.
pub fn icon(name: &str) -> SharedString {
    SharedString::from(format!("icons/{name}.svg"))
}
