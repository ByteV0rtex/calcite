//! Calcite: a fast, native Minecraft launcher.
#![recursion_limit = "512"]

mod assets;
mod backend;
mod cli;
mod images;
mod login;
mod state;
mod theme;
mod ui;
mod versions;

use gpui_kit::component::{Root, TitleBar};
use gpui_kit::prelude::*;
use gpui_kit::{App, Bounds, WindowBounds, WindowOptions, px, size};

use crate::backend::Backend;
use crate::state::AppState;
use crate::theme::{Accent, Appearance};
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
            let appearance = match snapshot.as_ref().map(|s| s.2.as_str()) {
                Some("light") => Appearance::Light,
                Some("oled") => Appearance::Oled,
                _ => Appearance::Dark,
            };
            theme::apply(appearance, Accent::Honey, cx);

            let api = calcite_core::modrinth::Client::new(&calcite_core::user_agent())
                .expect("HTTP client");
            images::Images::init(api.clone(), cx);
            cx.set_global(Api(api));
            cx.set_global(backend.clone());
            AppState::start(events, cx);

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
                        _ => Route::Home,
                    };
                    let shell = cx.new(|cx| Shell::new(start, window, cx));
                    cx.new(|cx| Root::new(shell, window, cx))
                })
                .expect("open the main window");
            cx.activate(true);

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
                    ("instance" | "instance-logs", Some(id)) => {
                        shell.navigate(Route::Instance(id), window, cx);
                        if page == "instance-logs" {
                            shell.with_instance_page(cx, |p, cx| {
                                p.set_tab(ui::pages::instance::Tab::Logs, cx)
                            });
                        }
                    }
                    ("create", _) => shell.open_create_dialog(window, cx),
                    _ => {}
                }
            });
        });
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
        let _ = cx.update(|cx| cx.quit());
    })
    .detach();
}

#[cfg(not(feature = "snapshot"))]
fn take_snapshot(_window: gpui_kit::AnyWindowHandle, _page: String, _out: String, _cx: &mut App) {
    eprintln!("This build can't take snapshots; rebuild with `--features snapshot`.");
}
