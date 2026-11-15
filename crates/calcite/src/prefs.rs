//! Calcite's own preferences (look and first-run state), stored as a small
//! JSON file in the data folder. Launcher settings live in the backend.

use serde::{Deserialize, Serialize};

use crate::theme::{Accent, Appearance};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Prefs {
    pub appearance: Appearance,
    pub accent: Accent,
    pub onboarding_done: bool,
}

impl gpui_kit::Global for Prefs {}

fn path() -> Option<std::path::PathBuf> {
    crate::backend::data_dir().map(|d| d.join("calcite-prefs.json"))
}

impl Prefs {
    pub fn load() -> Prefs {
        path()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(bytes) = serde_json::to_vec_pretty(self)
            && let Err(error) = std::fs::write(&path, bytes)
        {
            tracing::warn!("saving preferences failed: {error}");
        }
    }

    /// Change the look, apply it everywhere, and remember it.
    pub fn set_look(appearance: Appearance, accent: Accent, cx: &mut gpui_kit::App) {
        let prefs = cx.global_mut::<Prefs>();
        prefs.appearance = appearance;
        prefs.accent = accent;
        prefs.save();
        crate::theme::apply(appearance, accent, cx);
    }

    pub fn finish_onboarding(cx: &mut gpui_kit::App) {
        let prefs = cx.global_mut::<Prefs>();
        prefs.onboarding_done = true;
        prefs.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefs_round_trip_and_tolerate_old_files() {
        let prefs = Prefs {
            appearance: Appearance::Light,
            accent: Accent::Jade,
            onboarding_done: true,
        };
        let json = serde_json::to_string(&prefs).unwrap();
        assert_eq!(serde_json::from_str::<Prefs>(&json).unwrap(), prefs);
        // Missing fields fall back to defaults.
        let old: Prefs = serde_json::from_str(r#"{"accent":"teal"}"#).unwrap();
        assert_eq!(old.accent, Accent::Teal);
        assert!(!old.onboarding_done);
    }
}
