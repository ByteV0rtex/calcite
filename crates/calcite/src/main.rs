//! Calcite: a fast, native Minecraft launcher.
#![recursion_limit = "512"]

mod assets;
mod backend;
mod images;
mod theme;
mod ui;

use gpui_kit::component::{Root, TitleBar};
use gpui_kit::prelude::*;
use gpui_kit::{App, Bounds, WindowBounds, WindowOptions, px, size};

use crate::backend::Backend;
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
    let route_arg = |name: &str| match name {
        "gallery" => Route::Gallery,
        "library" => Route::Library,
        "discover" => Route::Discover,
        "instance" => Route::Instance("fo"),
        "instance-logs" | "instance-health" => Route::Instance("cobblemon"),
        _ => Route::Home,
    };
    // `--snapshot <page> <out.png> [light|oled]` (dev builds with the `snapshot` feature).
    let snapshot = match args.first().map(String::as_str) {
        Some("--snapshot") => Some((
            args.get(1).cloned().unwrap_or_default(),
            args.get(2).cloned().unwrap_or_else(|| "calcite.png".into()),
            args.get(3).cloned().unwrap_or_default(),
        )),
        _ => None,
    };
    let start = match (&snapshot, args.first().map(String::as_str)) {
        (Some((page, _, _)), _) => route_arg(page),
        (None, Some(flag)) if flag.starts_with("--") => route_arg(&flag[2..]),
        _ => Route::Home,
    };

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

            // Bring the launcher backend up in the background; the window
            // opens immediately and never waits on disk or network.
            let started = std::time::Instant::now();
            let init = backend.init_state();
            cx.spawn(async move |_cx| match init.await {
                Ok(()) => tracing::info!(
                    "backend ready in {:?} (data folder: {})",
                    started.elapsed(),
                    backend::data_id()
                ),
                Err(error) => tracing::error!("backend failed to start: {error:#}"),
            })
            .detach();

            // Drain backend events. Milestone 2 routes these into app state.
            cx.spawn(async move |_cx| {
                use futures::StreamExt as _;
                let mut events = events;
                while let Some(event) = events.next().await {
                    tracing::debug!("backend event: {}", event_name(&event));
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
            let page = snapshot.as_ref().map(|s| s.0.clone()).unwrap_or_default();
            let window = cx
                .open_window(options, |window, cx| {
                    window.set_window_title("Calcite");
                    let shell = cx.new(|cx| {
                        let mut shell = Shell::new(start.clone(), window, cx);
                        if page == "instance-logs" {
                            shell.set_instance_tab(ui::pages::instance::Tab::Logs, cx);
                        } else if page == "instance-health" {
                            shell.set_instance_tab(ui::pages::instance::Tab::Health, cx);
                        }
                        shell
                    });
                    cx.new(|cx| Root::new(shell, window, cx))
                })
                .expect("open the main window");
            cx.activate(true);

            if let Some((_, out, _)) = snapshot.clone() {
                take_snapshot(window.into(), out, cx);
            }
        });
}

/// Wait for network images to arrive, render the window offscreen, save it.
#[cfg(feature = "snapshot")]
fn take_snapshot(window: gpui_kit::AnyWindowHandle, out: String, cx: &mut App) {
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(std::time::Duration::from_secs(5))
            .await;
        let _ = cx.update_window(window, |_, window, _| window.refresh());
        cx.background_executor()
            .timer(std::time::Duration::from_millis(400))
            .await;
        let result = cx.update_window(window, |_, window, _| window.render_to_image());
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
fn take_snapshot(_window: gpui_kit::AnyWindowHandle, _out: String, _cx: &mut App) {
    eprintln!("This build can't take snapshots; rebuild with `--features snapshot`.");
}

fn event_name(event: &theseus::AppEvent) -> &'static str {
    use theseus::AppEvent::*;
    match event {
        Loading(_) => "loading",
        Process(_) => "process",
        Instance(_) => "instance",
        InstanceGroupsChanged(_) => "instance-groups",
        OnboardingChecklist(_) => "onboarding",
        InstallJob(_) => "install-job",
        Command(_) => "command",
        Warning(_) => "warning",
        Friend(_) => "friend",
        Notification(_) => "notification",
        Log(_) => "log",
        AdsConsentRequired(_) => "ads-consent",
    }
}
