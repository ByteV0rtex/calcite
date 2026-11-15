//! Health tab: problems found in the instance's mods, the latest crash
//! explained, memory advice, and one-click fixes for each.

use calcite_core::bisect::{Bisect, Step};
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

/// Where an in-progress search is saved, so a restart can finish it.
fn bisect_file(instance_id: &str) -> Option<std::path::PathBuf> {
    let safe: String = instance_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    crate::backend::data_dir().map(|d| d.join("calcite-bisect").join(format!("{safe}.json")))
}

impl InstancePage {
    pub(crate) fn resume_bisect(&mut self) {
        if let Some(path) = bisect_file(self.id())
            && let Ok(bytes) = std::fs::read(&path)
            && let Ok(saved) = serde_json::from_slice::<(Bisect, Step)>(&bytes)
        {
            self.extra.bisect = Some(saved);
        }
    }

    fn save_bisect(&self) {
        let Some(path) = bisect_file(self.id()) else {
            return;
        };
        // Kept until the player picks what to do with the result, so a
        // restart never strands mods turned off.
        match &self.extra.bisect {
            Some(state) => {
                let _ = std::fs::create_dir_all(path.parent().unwrap_or(&path));
                let _ = std::fs::write(&path, serde_json::to_vec(state).unwrap_or_default());
            }
            None => {
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    /// Turn mods off and on as one batch, then re-check. With `snapshot`,
    /// a snapshot is saved there first, before anything changes.
    fn apply_toggles(
        &mut self,
        off: Vec<String>,
        on: Vec<String>,
        snapshot: Option<std::path::PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let id = self.id().to_string();
        let starting = snapshot.is_some();
        let task = Backend::global(cx).call(async move {
            if let Some(snaps) = snapshot {
                let dir = theseus::instance::get_full_path(&id).await?;
                let taken = tokio::task::spawn_blocking(move || {
                    calcite_core::snapshots::take(
                        &dir,
                        &snaps,
                        "Before finding the broken mod",
                        true,
                    )
                })
                .await;
                if !matches!(taken, Ok(Ok(_))) {
                    return Err(theseus::ErrorKind::FSError(
                        "Couldn't save a snapshot first, so nothing was changed.".into(),
                    )
                    .into());
                }
            }
            for file in off {
                theseus::instance::toggle_disable_project(
                    &id,
                    &format!("mods/{file}"),
                    Some(false),
                )
                .await?;
            }
            for file in on {
                theseus::instance::toggle_disable_project(&id, &format!("mods/{file}"), Some(true))
                    .await?;
            }
            Ok(())
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = result {
                    this.state().update(cx, |s, cx| {
                        s.notify_error("Couldn't change the mods", format!("{error:#}"));
                        cx.notify();
                    });
                    // A search that couldn't start isn't running.
                    if starting {
                        this.extra.bisect = None;
                        this.save_bisect();
                    }
                }
                this.load_content(cx);
                // Once a search ends, re-check with everything back in place.
                if this.extra.bisect.is_none() {
                    this.load_health(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn start_bisect(&mut self, cx: &mut Context<Self>) {
        let Some(Ok(report)) = self.extra.health.clone() else {
            return;
        };
        let Some((bisect, step)) = Bisect::start(&report.mods) else {
            return;
        };
        // A snapshot first, in case anything goes wrong.
        let snapshot = crate::ui::pages::instance_tabs::snapshots_dir(self.id());
        if let Step::Test { turn_off, turn_on } = &step {
            self.apply_toggles(turn_off.clone(), turn_on.clone(), snapshot, cx);
        }
        self.extra.bisect = Some((bisect, step));
        self.save_bisect();
        cx.notify();
    }

    pub(crate) fn answer_bisect(&mut self, still_broken: bool, cx: &mut Context<Self>) {
        // Only a round in progress takes an answer.
        if !matches!(self.extra.bisect, Some((_, Step::Test { .. }))) {
            return;
        }
        let Some((mut bisect, _)) = self.extra.bisect.take() else {
            return;
        };
        let step = bisect.answer(still_broken);
        match &step {
            Step::Test { turn_off, turn_on } => {
                // Everything not in the new "off" group comes back on.
                let mut on: Vec<String> = turn_on.clone();
                for file in &bisect.touched {
                    if !turn_off.contains(file) && !on.contains(file) {
                        on.push(file.clone());
                    }
                }
                self.apply_toggles(turn_off.clone(), on, None, cx);
            }
            Step::Found(_) | Step::FoundGroup(_) | Step::NotAMod => {}
        }
        self.extra.bisect = Some((bisect, step));
        self.save_bisect();
        cx.notify();
    }

    /// End the search. `keep_off` stays turned off; everything else Calcite
    /// turned off comes back on.
    pub(crate) fn finish_bisect(&mut self, keep_off: Vec<String>, cx: &mut Context<Self>) {
        let Some((bisect, _)) = self.extra.bisect.take() else {
            return;
        };
        // Whatever can't load without the culprit stays off with it.
        let off = if keep_off.is_empty() {
            Vec::new()
        } else {
            bisect.and_dependents(&keep_off)
        };
        let on: Vec<String> = bisect
            .touched
            .iter()
            .filter(|f| !off.contains(f))
            .cloned()
            .collect();
        self.apply_toggles(off, on, None, cx);
        self.save_bisect();
        cx.notify();
    }

    /// A mod's display name from the last health check.
    fn mod_name(&self, file: &str) -> String {
        self.extra
            .health
            .as_ref()
            .and_then(|h| h.as_ref().ok())
            .and_then(|h| {
                h.mods
                    .iter()
                    .find(|m| m.file.trim_end_matches(".disabled") == file)
                    .map(|m| m.name.clone())
            })
            .unwrap_or_else(|| file.to_string())
    }

    fn bisect_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette().clone();
        let row = |title: String, body: String, actions: Vec<AnyElement>| {
            div()
                .flex()
                .items_center()
                .gap(space(3.0))
                .p(space(3.5))
                .child(icon("search", 18.0, p.accent))
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
                        .child(
                            div()
                                .text_size(px(type_scale::SMALL))
                                .text_color(p.text_secondary)
                                .child(body),
                        ),
                )
                .children(actions)
        };
        let content: AnyElement = match self.extra.bisect.clone() {
            None => row(
                "Find the broken mod".into(),
                "Something going wrong and no clear cause? Calcite turns off half your mods at a time; you press Play and say if it's fixed. It takes a few launches and puts everything back at the end.".into(),
                vec![Button::new("bisect-start")
                    .label("Start")
                    .small()
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.start_bisect(cx)))
                    .into_any_element()],
            )
            .into_any_element(),
            Some((bisect, Step::Test { turn_off, .. })) => row(
                format!(
                    "Round {}: {} mod{} turned off",
                    bisect.round,
                    turn_off.len(),
                    if turn_off.len() == 1 { "" } else { "s" }
                ),
                format!(
                    "Press Play and try what went wrong, then tell Calcite what happened. About {} more launch{} after this.",
                    bisect.rounds_left().saturating_sub(1),
                    if bisect.rounds_left().saturating_sub(1) == 1 { "" } else { "es" }
                ),
                vec![
                    Button::new("bisect-broken")
                        .label("Still broken")
                        .small()
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.answer_bisect(true, cx)))
                        .into_any_element(),
                    Button::new("bisect-works")
                        .label("Works now")
                        .small()
                        .primary()
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.answer_bisect(false, cx)))
                        .into_any_element(),
                    Button::new("bisect-stop")
                        .label("Stop")
                        .small()
                        .ghost()
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.finish_bisect(Vec::new(), cx)))
                        .into_any_element(),
                ],
            )
            .into_any_element(),
            Some((bisect, Step::Found(file))) => {
                let name = self.mod_name(&file);
                let also: Vec<String> = bisect
                    .and_dependents(std::slice::from_ref(&file))
                    .into_iter()
                    .filter(|f| *f != file)
                    .map(|f| self.mod_name(&f))
                    .collect();
                let keep = vec![file.clone()];
                let mut body = format!("The problem goes away when {file} is off.");
                if !also.is_empty() {
                    body.push_str(&format!(" {} need{} it, so {} stay off too.", also.join(", "), if also.len() == 1 { "s" } else { "" }, if also.len() == 1 { "it would" } else { "they would" }));
                }
                row(
                    format!("Found it: {name}"),
                    body,
                    vec![
                        Button::new("bisect-keep-off")
                            .label("Keep it off")
                            .small()
                            .primary()
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.finish_bisect(keep.clone(), cx)))
                            .into_any_element(),
                        Button::new("bisect-restore")
                            .label("Put everything back")
                            .small()
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.finish_bisect(Vec::new(), cx)))
                            .into_any_element(),
                    ],
                )
                .into_any_element()
            }
            Some((_, Step::FoundGroup(files))) => {
                let names: Vec<String> = files.iter().map(|f| self.mod_name(f)).collect();
                let keep = files.clone();
                row(
                    format!("It's one of these: {}", names.join(", ")),
                    "They need each other, so Calcite can't test them apart. Keep them off and turn the rest back on, or put everything back.".into(),
                    vec![
                        Button::new("bisect-keep-group-off")
                            .label("Keep them off")
                            .small()
                            .primary()
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.finish_bisect(keep.clone(), cx)))
                            .into_any_element(),
                        Button::new("bisect-restore")
                            .label("Put everything back")
                            .small()
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.finish_bisect(Vec::new(), cx)))
                            .into_any_element(),
                    ],
                )
                .into_any_element()
            }
            Some((_, Step::NotAMod)) => row(
                "It's probably not a mod".into(),
                "The problem happened with the mods turned off too. Check memory, Java and graphics drivers.".into(),
                vec![Button::new("bisect-done")
                    .label("Put everything back")
                    .small()
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.finish_bisect(Vec::new(), cx)))
                    .into_any_element()],
            )
            .into_any_element(),
        };
        panel(cx)
            .overflow_hidden()
            .child(content)
            .into_any_element()
    }

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
                    .when(modded, |d| {
                        d.child(section("Troubleshooting"))
                            .child(self.bisect_card(cx))
                    })
                    .child(section("Tuning"))
                    .child(tune),
            )
            .into_any_element()
    }
}
