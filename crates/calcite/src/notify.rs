//! Desktop notifications. Off unless the player turns them on, and only
//! sent when Calcite isn't the window they're looking at.

use gpui_kit::App;

use crate::prefs::Prefs;

pub fn send(title: impl Into<String>, body: impl Into<String>, cx: &App) {
    if !cx.global::<Prefs>().notify_when_away || cx.active_window().is_some() {
        return;
    }
    let (title, body) = (title.into(), body.into());
    // Showing one can block for a moment (D-Bus, the macOS service).
    let _ = std::thread::Builder::new()
        .name("notify".into())
        .spawn(move || {
            #[cfg(target_os = "macos")]
            {
                // Uses Calcite's name and icon; fails harmlessly after the first time.
                let _ = notify_rust::set_application("com.calcite.launcher");
            }
            if let Err(error) = notify_rust::Notification::new()
                .appname("Calcite")
                .summary(&title)
                .body(&body)
                .show()
            {
                tracing::debug!("notification not shown: {error}");
            }
        });
}
