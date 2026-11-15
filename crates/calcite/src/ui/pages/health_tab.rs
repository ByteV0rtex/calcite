//! Health tab: problems found in the instance's mods, the latest crash
//! explained, memory advice, and one-click fixes for each.

use calcite_core::crash::{self, Cause, Diagnosis};
use calcite_core::health::{self, Issue, LoaderKind, ModInfo};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, ClickEvent, Context, FontWeight, SharedString, div, px};
use theseus::data::ModLoader;

use crate::backend::Backend;
use crate::state::AppState;
use crate::theme::{PaletteExt, radius, space, type_scale};
use crate::ui::components::{Button, empty_state, icon, panel, relative_time, skeleton};
use crate::ui::instance_picker::content_type;
use crate::ui::pages::discover::Api;
use crate::ui::pages::instance::{InstancePage, InstancePageEvent};

/// Popular, safe performance mods, tried in this order.
const PERFORMANCE_MODS: [&str; 6] = [
    "sodium",
    "lithium",
    "ferrite-core",
    "entityculling",
    "immediatelyfast",
    "modernfix",
];

#[derive(Clone, Debug)]
pub struct HealthReport {
    pub mods: Vec<ModInfo>,
    pub issues: Vec<Issue>,
    pub crash: Option<(chrono::DateTime<chrono::Utc>, Diagnosis)>,
    pub memory_mb: Option<u32>,
    pub recommended_mb: u32,
}

fn loader_kind(loader: ModLoader) -> Option<LoaderKind> {
    match loader {
        ModLoader::Fabric => Some(LoaderKind::Fabric),
        ModLoader::Quilt => Some(LoaderKind::Quilt),
        ModLoader::Forge => Some(LoaderKind::Forge),
        ModLoader::NeoForge => Some(LoaderKind::NeoForge),
        ModLoader::Vanilla => None,
    }
}

impl InstancePage {
    pub(crate) fn load_health(&mut self, cx: &mut Context<Self>) {
        let id = self.id().to_string();
        let loader = self
            .state()
            .read(cx)
            .instance(&id)
            .map(|i| i.loader)
            .unwrap_or(ModLoader::Vanilla);
        let task = Backend::global(cx).run(async move {
            let dir = theseus::instance::get_full_path(&id).await.ok()?;
            let meta = theseus::instance::get(&id).await.ok().flatten();
            let settings = theseus::settings::get().await.ok();
            let system_mb = theseus::jre::get_max_memory().await.unwrap_or(0) / 1024;
            tokio::task::spawn_blocking(move || {
                let mods = health::read_mods_folder(&dir.join("mods"));
                let issues = match loader_kind(loader) {
                    Some(kind) => health::check(&mods, kind),
                    None => Vec::new(),
                };
                // The newest crash report, if any.
                let crash = std::fs::read_dir(dir.join("crash-reports"))
                    .into_iter()
                    .flatten()
                    .flatten()
                    .filter(|e| e.path().extension().is_some_and(|x| x == "txt"))
                    .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
                    .max_by_key(|(t, _)| *t)
                    .and_then(|(time, path)| {
                        let text = std::fs::read_to_string(path).ok()?;
                        Some((
                            chrono::DateTime::<chrono::Utc>::from(time),
                            crash::analyze(&text),
                        ))
                    });
                let enabled = mods
                    .iter()
                    .filter(|m| m.enabled && !m.loaders.is_empty())
                    .count();
                let memory_mb = meta
                    .and_then(|m| m.launch_overrides.memory.map(|m| m.maximum))
                    .or(settings.map(|s| s.memory.maximum));
                HealthReport {
                    recommended_mb: health::recommended_memory_mb(enabled, system_mb),
                    mods,
                    issues,
                    crash,
                    memory_mb,
                }
            })
            .await
            .ok()
        });
        cx.spawn(async move |this, cx| {
            let report = task.await.ok().flatten();
            let _ = this.update(cx, |this, cx| {
                this.extra.health =
                    Some(report.ok_or_else(|| "Couldn't read this instance's mods.".to_string()));
                cx.notify();
            });
        })
        .detach();
    }

    /// Turn a mod file on or off, then re-check.
    fn set_mod_enabled(&mut self, file: String, enabled: bool, cx: &mut Context<Self>) {
        let id = self.id().to_string();
        let task = Backend::global(cx).call(async move {
            theseus::instance::toggle_disable_project(&id, &format!("mods/{file}"), Some(enabled))
                .await
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = result {
                    this.state().update(cx, |s, cx| {
                        s.notify_error("Couldn't change that mod", format!("{error:#}"));
                        cx.notify();
                    });
                }
                this.load_health(cx);
                this.load_content(cx);
            });
        })
        .detach();
    }

    /// Look the missing mod up on Modrinth (mod ids usually match project
    /// slugs) and install it; otherwise open Discover to search for it.
    fn install_missing(&mut self, mod_id: String, cx: &mut Context<Self>) {
        let client = cx.global::<Api>().0.clone();
        let slug = mod_id.replace('_', "-");
        let lookup = Backend::global(cx).run(async move {
            match client.project(&slug).await {
                Ok(p) => Some(p),
                Err(_) => client.project(&mod_id).await.ok(),
            }
        });
        let instance_id = self.id().to_string();
        cx.spawn(async move |this, cx| {
            let project = lookup.await.ok().flatten();
            let _ = this.update(cx, |this, cx| match project {
                Some(project) => {
                    AppState::global(cx).update(cx, |s, cx| {
                        s.install_content(
                            instance_id,
                            project.id.clone(),
                            None,
                            content_type(project.project_type()),
                            cx,
                        )
                    });
                    this.extra.busy = Some(format!("Adding {}…", project.title).into());
                    cx.notify();
                }
                None => cx.emit(InstancePageEvent::AddContent(instance_id)),
            });
        })
        .detach();
    }

    fn add_performance_mods(&mut self, cx: &mut Context<Self>) {
        let Some(inst) = self.state().read(cx).instance(self.id()).cloned() else {
            return;
        };
        let installed: Vec<String> = self
            .extra
            .health
            .as_ref()
            .and_then(|h| h.as_ref().ok())
            .map(|h| h.mods.iter().map(|m| m.id.replace('_', "-")).collect())
            .unwrap_or_default();
        let client = cx.global::<Api>().0.clone();
        let loader = crate::ui::instance_picker::modrinth_loader(&inst);
        let game = inst.game_version.to_string();
        let find = Backend::global(cx).run(async move {
            let mut picks = Vec::new();
            for slug in PERFORMANCE_MODS {
                if installed
                    .iter()
                    .any(|i| i == slug || (slug == "ferrite-core" && i == "ferritecore"))
                {
                    continue;
                }
                if let Ok(versions) = client.versions(slug).await
                    && versions.iter().any(|v| v.supports(&loader, &game))
                    && let Ok(project) = client.project(slug).await
                {
                    picks.push(project.id);
                }
            }
            picks
        });
        let instance_id = self.id().to_string();
        self.extra.busy = Some("Finding performance mods for this version…".into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let picks = find.await.unwrap_or_default();
            let _ = this.update(cx, |this, cx| {
                let count = picks.len();
                for project in picks {
                    AppState::global(cx).update(cx, |s, cx| {
                        s.install_content(
                            instance_id.clone(),
                            project,
                            None,
                            theseus::data::ContentType::Mod,
                            cx,
                        )
                    });
                }
                this.extra.busy = Some(if count == 0 {
                    "Nothing to add: the performance mods that fit are already here.".into()
                } else {
                    format!(
                        "Adding {count} performance mod{}…",
                        if count == 1 { "" } else { "s" }
                    )
                    .into()
                });
                cx.notify();
            });
        })
        .detach();
    }

    fn file_for(&self, mod_id: &str) -> Option<String> {
        let report = self.extra.health.as_ref()?.as_ref().ok()?;
        let norm = |s: &str| s.to_ascii_lowercase().replace('-', "_");
        report
            .mods
            .iter()
            .find(|m| m.enabled && norm(&m.id) == norm(mod_id))
            .map(|m| m.file.clone())
    }

    pub(crate) fn health_tab(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette().clone();
        let report = match &self.extra.health {
            None => {
                let mut col = div()
                    .flex()
                    .flex_col()
                    .gap(space(3.0))
                    .px(space(8.0))
                    .pt(space(4.0));
                for _ in 0..3 {
                    col = col.child(skeleton(gpui_kit::relative(1.0), px(56.0), cx));
                }
                return col.into_any_element();
            }
            Some(Err(e)) => {
                return empty_state(
                    "circle-alert",
                    "Couldn't check this instance",
                    e.clone(),
                    None,
                    cx,
                )
                .into_any_element();
            }
            Some(Ok(r)) => r.clone(),
        };

        let card = |icon_name: &'static str,
                    tint: gpui_kit::Hsla,
                    title: String,
                    body: Option<String>,
                    actions: Vec<AnyElement>| {
            div()
                .flex()
                .items_center()
                .gap(space(3.0))
                .p(space(3.5))
                .border_b_1()
                .border_color(p.border)
                .child(icon(icon_name, 18.0, tint))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .gap(space(0.5))
                        .child(
                            div()
                                .text_size(px(type_scale::BODY))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.text)
                                .child(title),
                        )
                        .when_some(body, |d, b| {
                            d.child(
                                div()
                                    .text_size(px(type_scale::SMALL))
                                    .text_color(p.text_secondary)
                                    .child(b),
                            )
                        }),
                )
                .children(actions)
        };

        // ---- problems
        let mut problems = panel(cx).overflow_hidden();
        if report.issues.is_empty() {
            problems = problems.child(card(
                "circle-check",
                p.running,
                format!(
                    "No problems found in {} mods",
                    report.mods.iter().filter(|m| m.enabled && !m.loaders.is_empty()).count()
                ),
                Some("Every mod has what it needs, nothing is installed twice, and nothing declares a conflict.".into()),
                vec![],
            ));
        }
        for (ix, issue) in report.issues.iter().enumerate() {
            let (body, actions): (String, Vec<AnyElement>) = match issue.clone() {
                Issue::Duplicate { files, .. } => {
                    let keep = files.iter().max().cloned().unwrap_or_default();
                    let others: Vec<String> =
                        files.iter().filter(|f| **f != keep).cloned().collect();
                    (
                        format!("Files: {}. Only one copy can load.", files.join(", ")),
                        vec![
                            Button::new(SharedString::from(format!("fix-{ix}")))
                                .label("Keep one")
                                .small()
                                .primary()
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    for file in &others {
                                        this.set_mod_enabled(file.clone(), false, cx);
                                    }
                                }))
                                .into_any_element(),
                        ],
                    )
                }
                Issue::MissingDependency { needs, range, .. } => (
                    match health::describe_range(&range) {
                        Some(words) => format!(
                            "{needs} ({words}) isn't installed. The game won't start without it."
                        ),
                        None => {
                            format!("{needs} isn't installed. The game won't start without it.")
                        }
                    },
                    vec![
                        Button::new(SharedString::from(format!("fix-{ix}")))
                            .icon("arrow-down-to-line")
                            .label("Install it")
                            .small()
                            .primary()
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.install_missing(needs.clone(), cx)
                            }))
                            .into_any_element(),
                    ],
                ),
                Issue::DisabledDependency { needs_file, .. } => (
                    format!("It's installed as {needs_file} but turned off."),
                    vec![
                        Button::new(SharedString::from(format!("fix-{ix}")))
                            .label("Turn it on")
                            .small()
                            .primary()
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.set_mod_enabled(needs_file.clone(), true, cx)
                            }))
                            .into_any_element(),
                    ],
                ),
                Issue::Conflict {
                    other_file,
                    other_name,
                    range,
                    ..
                } => (
                    format!(
                        "It says it doesn't work with {other_name}{}. Update one of them, or turn one off.",
                        health::describe_range(&range)
                            .map(|w| format!(" ({w})"))
                            .unwrap_or_default()
                    ),
                    vec![
                        Button::new(SharedString::from(format!("fix-{ix}")))
                            .label(format!("Turn off {other_name}"))
                            .small()
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.set_mod_enabled(other_file.clone(), false, cx)
                            }))
                            .into_any_element(),
                    ],
                ),
                Issue::WrongLoader { file, .. } => (
                    format!(
                        "{file} can't load in this instance. Get the version for this loader, or turn it off."
                    ),
                    vec![
                        Button::new(SharedString::from(format!("fix-{ix}")))
                            .label("Turn it off")
                            .small()
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.set_mod_enabled(file.clone(), false, cx)
                            }))
                            .into_any_element(),
                    ],
                ),
            };
            problems = problems.child(card(
                "triangle-alert",
                p.warning,
                issue.title(),
                Some(body),
                actions,
            ));
        }

        // ---- last crash
        let crash_card: Option<AnyElement> = report.crash.clone().map(|(when, d)| {
            let tint = if d.cause == Cause::HungWhileClosing {
                p.text_muted
            } else {
                p.danger
            };
            let mut actions: Vec<AnyElement> = Vec::new();
            if let Some(file) = d.suspects.first().and_then(|id| self.file_for(id)) {
                let name = d.suspects[0].clone();
                actions.push(
                    Button::new("crash-disable")
                        .label(format!("Turn off {name}"))
                        .small()
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.set_mod_enabled(file.clone(), false, cx)
                        }))
                        .into_any_element(),
                );
            }
            if matches!(d.cause, Cause::OutOfMemory | Cause::WrongJava { .. }) {
                actions.push(
                    Button::new("crash-settings")
                        .icon("settings")
                        .label("Open settings")
                        .small()
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            cx.emit(InstancePageEvent::OpenSettings(this.id().to_string()))
                        }))
                        .into_any_element(),
                );
            }
            let mut body = d.advice.clone();
            if let Cause::MissingDependency { problems } = &d.cause {
                body = format!("{}\n{}", problems.join("\n"), body);
            }
            panel(cx)
                .overflow_hidden()
                .child(card(
                    "bug",
                    tint,
                    format!("Last crash, {}: {}", relative_time(when), d.title),
                    Some(body),
                    actions,
                ))
                .into_any_element()
        });

        // ---- memory and performance
        let memory_line = match report.memory_mb {
            Some(mb) if mb < report.recommended_mb => format!(
                "{mb} MB is set. About {} MB would suit {} mods on this computer.",
                report.recommended_mb,
                report
                    .mods
                    .iter()
                    .filter(|m| m.enabled && !m.loaders.is_empty())
                    .count()
            ),
            Some(mb) => format!("{mb} MB is set, which is enough for these mods."),
            None => "Uses the default from Settings.".into(),
        };
        let low_memory = report.memory_mb.is_some_and(|m| m < report.recommended_mb);
        let modded = self
            .state()
            .read(cx)
            .instance(self.id())
            .is_some_and(|i| i.loader != ModLoader::Vanilla);

        let tune = panel(cx)
            .overflow_hidden()
            .child(card(
                "memory-stick",
                if low_memory { p.warning } else { p.running },
                "Memory".into(),
                Some(memory_line),
                vec![Button::new("tune-memory")
                    .label("Change")
                    .small()
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        cx.emit(InstancePageEvent::OpenSettings(this.id().to_string()))
                    }))
                    .into_any_element()],
            ))
            .when(modded, |d| {
                d.child(card(
                    "gauge",
                    p.accent,
                    "Performance mods".into(),
                    Some("Adds Sodium, Lithium, FerriteCore, Entity Culling, ImmediatelyFast and ModernFix where they fit this version and aren't installed yet.".into()),
                    vec![Button::new("add-perf")
                        .icon("zap")
                        .label("Add them")
                        .small()
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.add_performance_mods(cx)))
                        .into_any_element()],
                ))
            });

        let section = |title: &'static str| {
            div()
                .pt(space(2.0))
                .text_size(px(type_scale::SMALL))
                .font_weight(FontWeight::MEDIUM)
                .text_color(p.text_muted)
                .child(title)
        };

        div()
            .id("health-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(space(3.0))
                    .px(space(8.0))
                    .py(space(4.0))
                    .children(self.extra.busy.clone().map(|b| {
                        div()
                            .px(space(3.0))
                            .py(space(2.0))
                            .rounded(px(radius::MD))
                            .bg(p.info_soft)
                            .text_size(px(type_scale::SMALL))
                            .text_color(p.info)
                            .child(b)
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .child(section("Mods"))
                            .child(div().flex_1())
                            .child(
                                Button::new("health-recheck")
                                    .icon("refresh-cw")
                                    .label("Check again")
                                    .ghost()
                                    .small()
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.extra.health = None;
                                        this.extra.busy = None;
                                        this.load_health(cx);
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(problems)
                    .when_some(crash_card, |d, c| d.child(section("Crashes")).child(c))
                    .child(section("Tuning"))
                    .child(tune),
            )
            .into_any_element()
    }
}
