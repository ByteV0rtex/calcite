//! Calcite's own preferences (look and first-run state), stored as a small
//! JSON file in the data folder. Launcher settings live in the backend.

use serde::{Deserialize, Serialize};

use crate::theme::{Accent, Appearance};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Prefs {
    pub appearance: Appearance,
    /// Light or dark following the system, instead of `appearance`.
    pub follow_system: bool,
    pub accent: Accent,
    pub onboarding_done: bool,
    /// Back up the worlds you played when the game closes.
    pub backup_worlds_on_exit: bool,
    /// Instances left out of the Library (kept on disk).
    pub hidden_instances: Vec<String>,
    /// Library as rows instead of cards.
    pub library_compact: bool,
    pub minimize_on_launch: bool,
    pub refocus_on_exit: bool,
    pub start_page: StartPage,
    /// Look for a newer Calcite on GitHub at startup.
    pub check_updates: bool,
    /// Desktop notifications when Calcite is in the background.
    pub notify_when_away: bool,
    /// Instance tabs the player hid (by label).
    pub hidden_tabs: Vec<String>,
}

/// The page Calcite opens on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum StartPage {
    #[default]
    Home,
    Library,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            appearance: Appearance::default(),
            follow_system: false,
            accent: Accent::default(),
            onboarding_done: false,
            backup_worlds_on_exit: true,
            hidden_instances: Vec::new(),
            library_compact: false,
            minimize_on_launch: false,
            refocus_on_exit: true,
            start_page: StartPage::Home,
            check_updates: true,
            notify_when_away: false,
            hidden_tabs: Vec::new(),
        }
    }
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

    /// Change the look, apply it everywhere, and remember it. Picking a
    /// fixed appearance stops following the system.
    pub fn set_look(appearance: Appearance, accent: Accent, cx: &mut gpui_kit::App) {
        let prefs = cx.global_mut::<Prefs>();
        prefs.appearance = appearance;
        prefs.follow_system = false;
        prefs.accent = accent;
        prefs.save();
        crate::theme::apply(appearance, accent, cx);
    }

    /// Keep the accent but switch light and dark with the system.
    pub fn set_follow_system(accent: Accent, cx: &mut gpui_kit::App) {
        let prefs = cx.global_mut::<Prefs>();
        prefs.follow_system = true;
        prefs.accent = accent;
        prefs.save();
        Prefs::apply_current(cx);
    }

    /// The appearance to show right now.
    pub fn effective_appearance(&self, cx: &gpui_kit::App) -> Appearance {
        if !self.follow_system {
            return self.appearance;
        }
        match cx.window_appearance() {
            gpui_kit::WindowAppearance::Light | gpui_kit::WindowAppearance::VibrantLight => {
                Appearance::Light
            }
            _ => Appearance::Dark,
        }
    }

    /// Apply the saved look (following the system if asked).
    pub fn apply_current(cx: &mut gpui_kit::App) {
        let prefs = cx.global::<Prefs>().clone();
        let appearance = prefs.effective_appearance(cx);
        crate::theme::apply(appearance, prefs.accent, cx);
    }

    pub fn set_backup_worlds(on: bool, cx: &mut gpui_kit::App) {
        let prefs = cx.global_mut::<Prefs>();
        prefs.backup_worlds_on_exit = on;
        prefs.save();
    }

    /// Hide or show instances in the Library.
    pub fn set_hidden(ids: &[String], hidden: bool, cx: &mut gpui_kit::App) {
        let prefs = cx.global_mut::<Prefs>();
        prefs.hidden_instances.retain(|h| !ids.contains(h));
        if hidden {
            prefs.hidden_instances.extend(ids.iter().cloned());
        }
        prefs.save();
    }

    pub fn set_compact(on: bool, cx: &mut gpui_kit::App) {
        let prefs = cx.global_mut::<Prefs>();
        prefs.library_compact = on;
        prefs.save();
    }

    /// Show or hide an instance tab.
    pub fn set_tab_hidden(label: &str, hidden: bool, cx: &mut gpui_kit::App) {
        let prefs = cx.global_mut::<Prefs>();
        prefs.hidden_tabs.retain(|t| t != label);
        if hidden {
            prefs.hidden_tabs.push(label.to_string());
        }
        prefs.save();
        cx.refresh_windows();
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
            follow_system: true,
            accent: Accent::Jade,
            onboarding_done: true,
            backup_worlds_on_exit: false,
            hidden_instances: vec!["a".into()],
            library_compact: true,
            minimize_on_launch: true,
            refocus_on_exit: false,
            start_page: StartPage::Library,
            check_updates: false,
            notify_when_away: true,
            hidden_tabs: vec!["Snapshots".into()],
        };
        let json = serde_json::to_string(&prefs).unwrap();
        assert_eq!(serde_json::from_str::<Prefs>(&json).unwrap(), prefs);
        // Missing fields fall back to defaults.
        let old: Prefs = serde_json::from_str(r#"{"accent":"teal"}"#).unwrap();
        assert_eq!(old.accent, Accent::Teal);
        assert!(!old.onboarding_done);
        assert!(old.backup_worlds_on_exit, "backups default to on");
        assert!(old.check_updates, "update checks default to on");
        assert!(!old.notify_when_away, "notifications are opt-in");
    }
}
