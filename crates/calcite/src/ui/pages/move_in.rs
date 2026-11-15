//! Move in: bring instances, worlds and accounts over from other launchers.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use calcite_core::launchers::{self, Installation};
use calcite_core::transfer::{self, Manifest, ModrinthPreview, Progress, human_bytes};
use gpui_kit::prelude::*;
use gpui_kit::{ClickEvent, Context, FontWeight, SharedString, Window, div, px};
use theseus::pack::import::ImportLauncherType;

use crate::backend::{self, Backend};
use crate::state::AppState;
use crate::theme::{PaletteExt, radius, space, type_scale};
use crate::ui::components::{Button, chip, empty_state, icon, panel, progress_bar, skeleton};

#[derive(Clone)]
struct OtherLauncher {
    kind: ImportLauncherType,
    label: &'static str,
    base: PathBuf,
    instances: Vec<String>,
}

#[derive(Clone)]
enum Staging {
    Idle,
    Running { label: SharedString, fraction: f32 },
    Done(Manifest),
    Failed(String),
}

struct Found {
    modrinth: Option<ModrinthPreview>,
    others: Vec<OtherLauncher>,
    installations: Vec<(&'static str, Installation)>,
    clients_without_mods: Vec<&'static str>,
}

pub struct MoveInPage {
    found: Option<Found>,
    staging: Staging,
    started: HashSet<String>,
    cancel: Arc<AtomicBool>,
}

fn launcher_label(kind: ImportLauncherType) -> &'static str {
    match kind {
        ImportLauncherType::PrismLauncher => "Prism Launcher",
        ImportLauncherType::MultiMC => "MultiMC",
        ImportLauncherType::ATLauncher => "ATLauncher",
        ImportLauncherType::GDLauncher => "GDLauncher",
        ImportLauncherType::Curseforge => "CurseForge app",
        ImportLauncherType::Unknown => "Other launcher",
    }
}

impl MoveInPage {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut page = MoveInPage {
            found: None,
            staging: Staging::Idle,
            started: HashSet::new(),
            cancel: Arc::new(AtomicBool::new(false)),
        };
        if let Some(manifest) = backend::data_dir().and_then(|d| transfer::staged(&d)) {
            page.staging = Staging::Done(manifest);
        }
        page.scan(cx);
        page
    }

    /// Look for every launcher we know, without changing anything.
    fn scan(&mut self, cx: &mut Context<Self>) {
        let scan = Backend::global(cx).run(async move {
            let modrinth = match transfer::modrinth_app_dir() {
                Some(dir) => transfer::preview_modrinth(&dir).await.ok(),
                None => None,
            };
            let mut others = Vec::new();
            for kind in [
                ImportLauncherType::PrismLauncher,
                ImportLauncherType::MultiMC,
                ImportLauncherType::ATLauncher,
                ImportLauncherType::GDLauncher,
                ImportLauncherType::Curseforge,
            ] {
                let Some(base) = theseus::pack::import::get_default_launcher_path(kind) else {
                    continue;
                };
                if !base.is_dir() {
                    continue;
                }
                if let Ok(mut instances) =
                    theseus::pack::import::get_importable_instances(kind, base.clone()).await
                    && !instances.is_empty()
                {
                    instances.sort_by_key(|i| i.to_lowercase());
                    others.push(OtherLauncher {
                        kind,
                        label: launcher_label(kind),
                        base,
                        instances,
                    });
                }
            }
            let home = dirs::home_dir().unwrap_or_default();
            let data = dirs::data_dir().unwrap_or_default();
            let mc = launchers::minecraft_dir().unwrap_or_else(|| {
                if cfg!(target_os = "macos") {
                    data.join("minecraft")
                } else if cfg!(windows) {
                    data.join(".minecraft")
                } else {
                    home.join(".minecraft")
                }
            });
            let mut installations: Vec<(&'static str, Installation)> = Vec::new();
            if launchers::minecraft_dir().is_some()
                && let Ok(list) = launchers::installations(&mc)
            {
                installations.extend(list.into_iter().map(|i| ("Minecraft Launcher", i)));
            }
            installations.extend(
                launchers::lunar_installations(&home, &mc)
                    .into_iter()
                    .map(|i| ("Lunar Client", i)),
            );
            installations.extend(
                launchers::feather_installations(&home, &data, &mc)
                    .into_iter()
                    .map(|i| ("Feather Client", i)),
            );
            let (lunar, feather) = launchers::client_present(&home, &data, &mc);
            let mut clients_without_mods = Vec::new();
            if lunar && !installations.iter().any(|(s, _)| *s == "Lunar Client") {
                clients_without_mods.push("Lunar Client");
            }
            if feather && !installations.iter().any(|(s, _)| *s == "Feather Client") {
                clients_without_mods.push("Feather Client");
            }
            Found {
                modrinth,
                others,
                installations,
                clients_without_mods,
            }
        });
        cx.spawn(async move |this, cx| {
            if let Ok(found) = scan.await {
                let _ = this.update(cx, |this, cx| {
                    this.found = Some(found);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn stage_modrinth(&mut self, preview: ModrinthPreview, cx: &mut Context<Self>) {
        let Some(calcite_dir) = backend::data_dir() else {
            self.staging = Staging::Failed("Calcite's data folder couldn't be found.".into());
            cx.notify();
            return;
        };
        let schema: i64 = backend::DB_SCHEMA.parse().unwrap_or(0);
        let (tx, mut rx) = futures::channel::mpsc::unbounded::<Progress>();
        let cancel = self.cancel.clone();
        let task = Backend::global(cx).run(async move {
            transfer::stage_modrinth(&preview, &calcite_dir, schema, cancel, move |p| {
                let _ = tx.unbounded_send(p);
            })
            .await
        });
        self.staging = Staging::Running {
            label: "Getting ready".into(),
            fraction: 0.0,
        };
        cx.notify();
        cx.spawn(async move |this, cx| {
            use futures::StreamExt as _;
            while let Some(progress) = rx.next().await {
                let (label, fraction): (SharedString, f32) = match progress {
                    Progress::Snapshotting => ("Copying your library database".into(), 0.02),
                    Progress::Copying { done, total } => (
                        format!(
                            "Bringing files over · {} of {}",
                            human_bytes(done.bytes),
                            human_bytes(total.bytes)
                        )
                        .into(),
                        if total.bytes == 0 {
                            0.0
                        } else {
                            (done.bytes as f64 / total.bytes as f64) as f32 * 0.95
                        },
                    ),
                    Progress::Verifying => ("Checking everything arrived".into(), 0.97),
                };
                let _ = this.update(cx, |this, cx| {
                    this.staging = Staging::Running { label, fraction };
                    cx.notify();
                });
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            let result = task.await.and_then(|r| r);
            let _ = this.update(cx, |this, cx| {
                this.staging = match result {
                    Ok(manifest) => Staging::Done(manifest),
                    Err(error) => Staging::Failed(format!("{error:#}")),
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn modrinth_card(
        &self,
        preview: &ModrinthPreview,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let p = cx.palette().clone();
        let schema: i64 = backend::DB_SCHEMA.parse().unwrap_or(0);
        let supported = preview.schema_supported(schema);
        let has_instances = !AppState::global(cx).read(cx).instances.is_empty();

        let body: gpui_kit::AnyElement = match &self.staging {
            Staging::Running { label, fraction } => div()
                .flex()
                .flex_col()
                .gap(space(2.0))
                .child(
                    div()
                        .text_size(px(type_scale::SMALL))
                        .text_color(p.text_secondary)
                        .child(label.clone()),
                )
                .child(progress_bar(*fraction, cx))
                .into_any_element(),
            Staging::Done(manifest) => div()
                .flex()
                .items_center()
                .gap(space(3.0))
                .child(icon("circle-check", 18.0, p.running))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(type_scale::BODY))
                        .text_color(p.text_secondary)
                        .child(format!(
                            "Ready: {} instances. {} shared with Modrinth App instead of copied. Restart Calcite to finish.",
                            manifest.instances,
                            human_bytes(manifest.cloned_bytes)
                        )),
                )
                .child(
                    Button::new("restart-calcite")
                        .icon("rotate-ccw")
                        .label("Restart now")
                        .primary()
                        .on_click(|_, _, cx| cx.restart()),
                )
                .into_any_element(),
            Staging::Failed(error) => div()
                .flex()
                .items_center()
                .gap(space(3.0))
                .child(icon("circle-alert", 18.0, p.danger))
                .child(
                    div()
                        .flex_1()
                        .text_size(px(type_scale::SMALL))
                        .text_color(p.text_secondary)
                        .child(error.clone()),
                )
                .into_any_element(),
            Staging::Idle if !supported => div()
                .text_size(px(type_scale::SMALL))
                .text_color(p.warning)
                .child("Modrinth App has been updated past what this Calcite understands. Update Calcite, then come back.")
                .into_any_element(),
            Staging::Idle => {
                let preview = preview.clone();
                div()
                    .flex()
                    .items_center()
                    .gap(space(3.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(type_scale::SMALL))
                            .text_color(p.text_secondary)
                            .child(if has_instances {
                                "Brings over instances, groups, accounts, settings and playtime. Calcite's current instances are kept in a backup folder."
                            } else {
                                "Brings over instances, groups, accounts, settings and playtime. Modrinth App's folder is only read, never changed."
                            }),
                    )
                    .child(
                        Button::new("move-modrinth")
                            .icon("arrow-down-to-line")
                            .label("Bring it all over")
                            .primary()
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.stage_modrinth(preview.clone(), cx)
                            })),
                    )
                    .into_any_element()
            }
        };

        panel(cx)
            .p(space(4.0))
            .flex()
            .flex_col()
            .gap(space(4.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space(3.0))
                    .child(source_badge("M", cx))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .gap(space(1.0))
                            .child(
                                div()
                                    .text_size(px(type_scale::BODY_LG))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(p.text)
                                    .child("Modrinth App"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap(space(1.5))
                                    .child(chip(format!("{} instances", preview.instances), cx))
                                    .child(chip(
                                        format!(
                                            "{} account{}",
                                            preview.accounts,
                                            if preview.accounts == 1 { "" } else { "s" }
                                        ),
                                        cx,
                                    ))
                                    .child(chip(human_bytes(preview.size.bytes), cx)),
                            ),
                    ),
            )
            .child(body)
            .into_any_element()
    }

    fn other_card(&self, launcher: &OtherLauncher, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let p = cx.palette().clone();
        let all_started = launcher
            .instances
            .iter()
            .all(|i| self.started.contains(&format!("{}:{i}", launcher.label)));
        let launcher_all = launcher.clone();
        let mut rows = div().flex().flex_col();
        for (ix, name) in launcher.instances.iter().enumerate() {
            let key = format!("{}:{name}", launcher.label);
            let started = self.started.contains(&key);
            let (kind, base, folder) = (launcher.kind, launcher.base.clone(), name.clone());
            rows = rows.child(
                div()
                    .flex()
                    .items_center()
                    .gap(space(3.0))
                    .h(px(40.0))
                    .when(ix > 0, |d| d.border_t_1().border_color(p.border))
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_size(px(type_scale::BODY))
                            .text_color(p.text)
                            .child(name.clone()),
                    )
                    .child(
                        Button::new(SharedString::from(format!(
                            "import-{}-{ix}",
                            launcher.label
                        )))
                        .label(if started { "Importing" } else { "Import" })
                        .icon(if started {
                            "check"
                        } else {
                            "arrow-down-to-line"
                        })
                        .small()
                        .disabled(started)
                        .on_click(cx.listener(
                            move |this, _: &ClickEvent, _, cx| {
                                this.started.insert(key.clone());
                                let (base, folder) = (base.clone(), folder.clone());
                                AppState::global(cx).update(cx, |state, cx| {
                                    state.import_from_launcher(kind, base, folder, cx)
                                });
                                cx.notify();
                            },
                        )),
                    ),
            );
        }
        panel(cx)
            .p(space(4.0))
            .flex()
            .flex_col()
            .gap(space(3.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space(3.0))
                    .child(source_badge(&launcher.label[..1], cx))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(type_scale::BODY_LG))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.text)
                            .child(launcher.label),
                    )
                    .child(
                        Button::new(SharedString::from(format!("import-all-{}", launcher.label)))
                            .label("Import all")
                            .small()
                            .disabled(all_started)
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                for name in &launcher_all.instances {
                                    let key = format!("{}:{name}", launcher_all.label);
                                    if !this.started.insert(key) {
                                        continue;
                                    }
                                    let (kind, base, folder) = (
                                        launcher_all.kind,
                                        launcher_all.base.clone(),
                                        name.clone(),
                                    );
                                    AppState::global(cx).update(cx, |state, cx| {
                                        state.import_from_launcher(kind, base, folder, cx)
                                    });
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(rows)
            .into_any_element()
    }

    fn installation_card(
        &self,
        source: &'static str,
        items: Vec<Installation>,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let p = cx.palette().clone();
        let mut rows = div().flex().flex_col();
        for (ix, inst) in items.into_iter().enumerate() {
            let key = format!("{source}:{}:{}", inst.name, inst.version.game_version);
            let started = self.started.contains(&key);
            let mut detail = vec![
                format!("{:?} {}", inst.version.loader, inst.version.game_version)
                    .replace("latest-release", "latest release")
                    .replace("latest-snapshot", "latest snapshot"),
            ];
            if inst.mod_count > 0 {
                detail.push(format!("{} mods", inst.mod_count));
            }
            if inst.has_content {
                detail.push("worlds and settings".into());
            }
            let install = inst.clone();
            rows = rows.child(
                div()
                    .flex()
                    .items_center()
                    .gap(space(3.0))
                    .h(px(48.0))
                    .when(ix > 0, |d| d.border_t_1().border_color(p.border))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(type_scale::BODY))
                                    .text_color(p.text)
                                    .child(inst.name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(type_scale::CAPTION))
                                    .text_color(p.text_muted)
                                    .child(detail.join(" · ")),
                            ),
                    )
                    .child(
                        Button::new(SharedString::from(format!("install-{source}-{ix}")))
                            .label(if started { "Importing" } else { "Import" })
                            .icon(if started {
                                "check"
                            } else {
                                "arrow-down-to-line"
                            })
                            .small()
                            .disabled(started)
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.started.insert(key.clone());
                                let install = install.clone();
                                AppState::global(cx)
                                    .update(cx, |state, cx| state.import_installation(install, cx));
                                cx.notify();
                            })),
                    ),
            );
        }
        panel(cx)
            .p(space(4.0))
            .flex()
            .flex_col()
            .gap(space(3.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space(3.0))
                    .child(source_badge(&source[..1], cx))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(space(0.5))
                            .child(
                                div()
                                    .text_size(px(type_scale::BODY_LG))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(p.text)
                                    .child(source),
                            )
                            .child(
                                div()
                                    .text_size(px(type_scale::SMALL))
                                    .text_color(p.text_muted)
                                    .child(if source == "Minecraft Launcher" {
                                        "Each installation becomes an instance, with its worlds, packs and settings."
                                    } else {
                                        "Your added Fabric mods, worlds, packs and settings come over. The client's own built-in mods stay with it."
                                    }),
                            ),
                    ),
            )
            .child(rows)
            .into_any_element()
    }
}

fn source_badge(letter: &str, cx: &App) -> impl IntoElement {
    let p = cx.palette();
    div()
        .size(px(36.0))
        .flex_none()
        .rounded(px(radius::MD))
        .bg(p.raised)
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(type_scale::SECTION))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(p.text_secondary)
        .child(letter.to_string())
}

use gpui_kit::App;

impl Render for MoveInPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();
        let mut list = div().flex().flex_col().gap(space(4.0));
        match &self.found {
            None => {
                for _ in 0..3 {
                    list = list.child(
                        skeleton(gpui_kit::relative(1.0), px(96.0), cx).rounded(px(radius::LG)),
                    );
                }
            }
            Some(found) => {
                let nothing = found.modrinth.is_none()
                    && found.others.is_empty()
                    && found.installations.is_empty();
                if nothing {
                    list = list.child(empty_state(
                        "search",
                        "No other launchers found",
                        "Calcite looked for Modrinth App, Prism, MultiMC, ATLauncher, GDLauncher, the CurseForge app, the Minecraft Launcher, Lunar and Feather.",
                        None,
                        cx,
                    ));
                }
                if let Some(preview) = found.modrinth.clone() {
                    list = list.child(self.modrinth_card(&preview, cx));
                }
                for launcher in found.others.clone() {
                    list = list.child(self.other_card(&launcher, cx));
                }
                for source in ["Minecraft Launcher", "Lunar Client", "Feather Client"] {
                    let items: Vec<Installation> = found
                        .installations
                        .iter()
                        .filter(|(s, _)| *s == source)
                        .map(|(_, i)| i.clone())
                        .collect();
                    if !items.is_empty() {
                        list = list.child(self.installation_card(source, items, cx));
                    }
                }
                for client in found.clients_without_mods.clone() {
                    list = list.child(
                        panel(cx)
                            .p(space(4.0))
                            .flex()
                            .items_center()
                            .gap(space(3.0))
                            .child(source_badge(&client[..1], cx))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(space(0.5))
                                    .child(
                                        div()
                                            .text_size(px(type_scale::BODY_LG))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(p.text)
                                            .child(client),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(type_scale::SMALL))
                                            .text_color(p.text_muted)
                                            .child("Found it, but there are no added mods to bring. Your worlds and packs come over with the Minecraft Launcher entries."),
                                    ),
                            ),
                    );
                }
            }
        }

        div()
            .id("move-in-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(space(5.0))
                    .px(space(8.0))
                    .py(space(8.0))
                    .max_w(px(880.0))
                    .child(list),
            )
    }
}
