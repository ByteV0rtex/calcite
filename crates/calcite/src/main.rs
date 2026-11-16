//! Calcite: a fast, native Minecraft launcher.
#![recursion_limit = "512"]

mod assets;
mod backend;
mod cli;
mod images;
mod login;
mod modrinth_login;
mod prefs;
mod shortcuts;
mod state;
mod theme;
mod ui;
mod versions;

use gpui_kit::component::{Root, TitleBar};
use gpui_kit::prelude::*;
use gpui_kit::{App, Bounds, WindowBounds, WindowOptions, px, size};

use crate::backend::Backend;
use crate::state::AppState;
use crate::theme::Appearance;
use crate::ui::pages::discover::Api;
use crate::ui::shell::{Route, Shell};

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "calcite=info,theseus=warn".into()),
        )
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();

    // Command-line mode: `calcite list`, `calcite create …`, `calcite play …`.
    if let Some(code) = cli::run(&args) {
        std::process::exit(code);
    }

    // `--snapshot <page> <out.png> [light|oled]` (dev builds with the `snapshot` feature).
    let snapshot = match args.first().map(String::as_str) {
        Some("--snapshot") => Some((
            args.get(1).cloned().unwrap_or_default(),
            args.get(2).cloned().unwrap_or_else(|| "calcite.png".into()),
            args.get(3).cloned().unwrap_or_default(),
        )),
        _ => None,
    };
    let page_arg = snapshot
        .as_ref()
        .map(|s| s.0.clone())
        .or_else(|| {
            args.first()
                .and_then(|a| a.strip_prefix("--"))
                .map(str::to_string)
        })
        .unwrap_or_default();

    let (backend, events) = match Backend::start() {
        Ok(started) => started,
        Err(error) => {
            eprintln!("Calcite couldn't start its backend: {error:#}");
            std::process::exit(1);
        }
    };

    gpui_kit::application()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            gpui_kit::init(cx);
            if let Err(error) = assets::load_fonts(cx) {
                tracing::warn!("bundled fonts failed to load: {error:#}");
            }
            let mut saved = prefs::Prefs::load();
            match snapshot.as_ref().map(|s| s.2.as_str()) {
                Some("light") => saved.appearance = Appearance::Light,
                Some("oled") => saved.appearance = Appearance::Oled,
                Some(_) => saved.appearance = Appearance::Dark,
                None => {}
            }
            if snapshot
                .as_ref()
                .is_some_and(|s| s.0.starts_with("onboarding"))
            {
                saved.onboarding_done = false;
            } else if snapshot.is_some() {
                saved.onboarding_done = true;
            }
            theme::apply(saved.appearance, saved.accent, cx);
            cx.set_global(saved);

            let api = calcite_core::modrinth::Client::new(&calcite_core::user_agent())
                .expect("HTTP client");
            images::Images::init(api.clone(), cx);
            cx.set_global(Api(api));
            cx.set_global(backend.clone());
            AppState::start(events, cx);
            // A skin change waits a moment before it's sent; send it now if
            // Calcite is closing.
            cx.on_app_quit(|cx| {
                let flush = Backend::global(cx)
                    .call(async { theseus::minecraft_skins::flush_pending_skin_change().await });
                async move {
                    let _ = flush.await;
                }
            })
            .detach();

            let bounds = Bounds::centered(None, size(px(1280.0), px(820.0)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(960.0), px(600.0))),
                app_id: Some("com.calcite.launcher".into()),
                titlebar: Some(gpui_kit::TitlebarOptions {
                    // Centre the traffic lights on Calcite's 40px title bar.
                    traffic_light_position: Some(gpui_kit::point(px(13.0), px(13.0))),
                    ..TitleBar::title_bar_options()
                }),
                ..TitleBar::window_options()
            };
            let window = cx
                .open_window(options, |window, cx| {
                    window.set_window_title("Calcite");
                    let start = match page_arg.as_str() {
                        "gallery" => Route::Gallery,
                        "library" => Route::Library,
                        "discover" => Route::Discover,
                        "move-in" => Route::Settings(ui::pages::settings::Section::MoveIn),
                        "settings" => Route::Settings(ui::pages::settings::Section::Appearance),
                        "accounts" => Route::Settings(ui::pages::settings::Section::Accounts),
                        "game-settings" => Route::Settings(ui::pages::settings::Section::Game),
                        "storage" => Route::Settings(ui::pages::settings::Section::Storage),
                        "about" => Route::Settings(ui::pages::settings::Section::About),
                        "advanced" => Route::Settings(ui::pages::settings::Section::Advanced),
                        "skins" => Route::Skins,
                        "screenshots" => Route::Screenshots,
                        "user" => Route::User(
                            std::env::var("CALCITE_SNAPSHOT_USER")
                                .unwrap_or_else(|_| "jellysquid3".into()),
                        ),
                        "home" => Route::Home,
                        _ => match cx.global::<prefs::Prefs>().start_page {
                            prefs::StartPage::Home => Route::Home,
                            prefs::StartPage::Library => Route::Library,
                        },
                    };
                    let shell = cx.new(|cx| Shell::new(start, window, cx));
                    cx.new(|cx| Root::new(shell, window, cx))
                })
                .expect("open the main window");
            cx.activate(true);

            // ⌘K / Ctrl+K opens the command palette from anywhere.
            cx.bind_keys([gpui_kit::KeyBinding::new(
                if cfg!(target_os = "macos") {
                    "cmd-k"
                } else {
                    "ctrl-k"
                },
                ui::shell::TogglePalette,
                None,
            )]);
            cx.on_action(move |_: &ui::shell::TogglePalette, cx| {
                let _ = window.update(cx, |root, window, cx| {
                    if let Ok(shell) = root.view().clone().downcast::<Shell>() {
                        shell.update(cx, |s, cx| s.toggle_palette(window, cx));
                    }
                });
            });

            if let Some((page, out, _)) = snapshot.clone() {
                take_snapshot(window.into(), page, out, cx);
            }
        });
}

/// Wait for data and images to arrive, render the window offscreen, save it.
/// Pages: home, library, discover, gallery, instance, instance-logs, create.
#[cfg(feature = "snapshot")]
fn take_snapshot(window: gpui_kit::AnyWindowHandle, page: String, out: String, cx: &mut App) {
    cx.spawn(async move |cx| {
        let executor = cx.background_executor().clone();
        let wait = |ms| executor.timer(std::time::Duration::from_millis(ms));
        wait(2500).await;
        // Pages that need app state are opened once the backend is ready.
        let _ = cx.update_window(window, |root, window, cx| {
            let Ok(root) = root.downcast::<Root>() else {
                return;
            };
            let Some(shell) = root.read(cx).view().clone().downcast::<Shell>().ok() else {
                return;
            };
            shell.update(cx, |shell, cx| {
                let first = AppState::global(cx)
                    .read(cx)
                    .instances
                    .first()
                    .map(|i| i.id.clone());
                match (page.as_str(), first) {
                    (
                        "instance" | "instance-logs" | "instance-worlds" | "instance-shots"
                        | "instance-snapshots" | "instance-settings" | "instance-health"
                        | "instance-files",
                        Some(id),
                    ) => {
                        use ui::pages::instance::Tab;
                        // Prefer an instance with worlds for the world/screenshot views.
                        let id = std::env::var("CALCITE_SNAPSHOT_INSTANCE").unwrap_or(id);
                        shell.navigate(Route::Instance(id.clone()), window, cx);
                        let tab = match page.as_str() {
                            "instance-logs" => Some(Tab::Logs),
                            "instance-worlds" => Some(Tab::Worlds),
                            "instance-shots" => Some(Tab::Screenshots),
                            "instance-snapshots" => Some(Tab::Snapshots),
                            "instance-files" => Some(Tab::Files),
                            "instance-health" => Some(Tab::Health),
                            _ => None,
                        };
                        if let Some(tab) = tab {
                            shell.with_instance_page(cx, |p, cx| p.set_tab(tab, cx));
                        }
                        if page == "instance-settings" {
                            shell.open_instance_settings(id, window, cx);
                        }
                    }
                    ("create", _) => shell.open_create_dialog(window, cx),
                    ("onboarding-movein", _) => shell.set_onboarding_step(1, window, cx),
                    ("project", _) => shell.navigate(Route::Project("sodium".into(), None), window, cx),
                    ("discover-instance", Some(id)) => shell.open_discover(Some(id), window, cx),
                    ("picker", _) => shell.open_picker(
                        ui::instance_picker::ProjectSummary {
                            id: "AANobbMI".into(),
                            title: "Sodium".into(),
                            icon_url: Some("https://cdn.modrinth.com/data/AANobbMI/295862f4724dc3f78df3447ad6072b2dcd3ef0c9_96.webp".into()),
                            kind: calcite_core::modrinth::ProjectType::Mod,
                        },
                        window,
                        cx,
                    ),
                    _ => {}
                }
            });
        });
        // Dev hook: comma-separated steps run before the picture is taken
        // (`play`, `wait` = 5 s, and the "Find the broken mod" answers).
        if let Ok(script) = std::env::var("CALCITE_SNAPSHOT_SCRIPT") {
            wait(2500).await;
            for word in script.split(',') {
                let word = word.trim().to_string();
                if word == "wait" {
                    wait(5000).await;
                    continue;
                }
                if let Some(address) = word.strip_prefix("add-server=") {
                    let (id, address) = (
                        std::env::var("CALCITE_SNAPSHOT_INSTANCE").unwrap_or_default(),
                        address.to_string(),
                    );
                    let task = cx.update(|cx| {
                        Backend::global(cx).call(async move {
                            theseus::worlds::add_server_to_instance(
                                &id,
                                address.clone(),
                                address,
                                theseus::worlds::ServerPackStatus::Prompt,
                                None,
                                None,
                            )
                            .await
                        })
                    });
                    let _ = task.await;
                    continue;
                }
                if word == "reload-worlds" {
                    let _ = cx.update_window(window, |root, _, cx| {
                        if let Ok(root) = root.downcast::<Root>()
                            && let Ok(shell) = root.read(cx).view().clone().downcast::<Shell>()
                        {
                            shell.update(cx, |shell, cx| shell.with_instance_page(cx, |p, cx| p.load_worlds(cx)));
                        }
                    });
                    continue;
                }
                if word == "demo-jobs" {
                    cx.update(|cx| AppState::global(cx).update(cx, |s, cx| s.demo_jobs(cx)));
                    continue;
                }
                if word == "demo-friends" {
                    cx.update(|cx| AppState::global(cx).update(cx, |s, cx| s.demo_friends(cx)));
                    continue;
                }
                let _ = cx.update_window(window, |root, window, cx| {
                    let Ok(root) = root.downcast::<Root>() else { return };
                    let Some(shell) = root.read(cx).view().clone().downcast::<Shell>().ok() else {
                        return;
                    };
                    shell.update(cx, |shell, cx| {
                        if word == "join" {
                            shell.open_join(window, cx);
                        }
                        if let Some(text) = word.strip_prefix("search=") {
                            shell.type_search(text, window, cx);
                        }
                        if word == "palette" {
                            shell.toggle_palette(window, cx);
                        }
                        if let Some(text) = word.strip_prefix("palette=") {
                            shell.toggle_palette(window, cx);
                            shell.palette_query(text, window, cx);
                        }
                        if word == "edit-profile" {
                            shell.open_profile_editor(window, cx);
                        }
                        if word == "servers" {
                            shell.open_servers(window, cx);
                        }
                        if word == "open-downloads" {
                            shell.open_downloads(cx);
                        }
                        if word.starts_with("lib-") {
                            shell.dev_library(&word, cx);
                        }
                        if word == "change-version" {
                            let id = std::env::var("CALCITE_SNAPSHOT_INSTANCE").unwrap_or_default();
                            shell.request_change_version(id, cx);
                        }
                        if let Some(skins) = shell.skins_page()
                            && word == "skins-back"
                        {
                            skins.update(cx, |s, cx| s.look_from_behind(cx));
                        }
                        if let Some(storage) = shell.storage_page() {
                            match word.as_str() {
                                "clean-logs" => storage.update(cx, |s, cx| s.delete_logs(cx)),
                                "clean-store" => storage.update(cx, |s, cx| s.clean_store(cx)),
                                _ => {}
                            }
                        }
                        if word.starts_with("dev-")
                            && let Some(page) = shell.instance_page_entity()
                        {
                            page.update(cx, |p, cx| p.dev(&word, window, cx));
                        }
                        shell.with_instance_page(cx, |p, cx| match word.as_str() {
                            "start" => p.start_bisect(cx),
                            "works" => p.answer_bisect(false, cx),
                            "broken" => p.answer_bisect(true, cx),
                            "stop" => p.finish_bisect(Vec::new(), cx),
                            "share" => cx.emit(ui::pages::instance::InstancePageEvent::Share(p.id().to_string())),
                            "more" => p.open_more(cx),
                            w if w.starts_with("open-file=") => {
                                p.open_file(w.trim_start_matches("open-file=").to_string(), cx)
                            }
                            "play" => {
                                let id = p.id().to_string();
                                p.state().update(cx, |s, cx| s.play(&id, cx));
                            }
                            _ => {}
                        })
                    });
                });
                wait(1500).await;
            }
        }
        wait(3500).await;
        // Draw explicitly: macOS pauses drawing for windows hidden behind others.
        let result = cx.update_window(window, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
            window.render_to_image()
        });
        match result {
            Ok(Ok(image)) => match image.save(&out) {
                Ok(()) => println!("snapshot saved to {out}"),
                Err(e) => eprintln!("saving snapshot failed: {e}"),
            },
            Ok(Err(e)) | Err(e) => eprintln!("snapshot failed: {e:#}"),
        }
        cx.update(|cx| cx.quit());
    })
    .detach();
}

#[cfg(not(feature = "snapshot"))]
fn take_snapshot(_window: gpui_kit::AnyWindowHandle, _page: String, _out: String, _cx: &mut App) {
    eprintln!("This build can't take snapshots; rebuild with `--features snapshot`.");
}
