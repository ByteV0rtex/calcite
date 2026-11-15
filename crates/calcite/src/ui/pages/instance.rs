//! A single instance: header with Play, then its content and live logs.

use chrono::Utc;
use gpui_kit::prelude::*;
use gpui_kit::{
    ClickEvent, Context, Entity, FontWeight, ListSizingBehavior, PathPromptOptions, SharedString,
    Subscription, UniformListScrollHandle, Window, div, px, uniform_list,
};
use theseus::data::{ContentItem, ProjectType};

use crate::backend::Backend;
use crate::images::Images;
use crate::state::{AppState, LogLevel, LogLine, parse_legacy_line};
use crate::theme::{self, PaletteExt, radius, space, type_scale};
use crate::ui::components::{
    Button, ButtonSize, chip, empty_state, icon_chip, progress_bar, relative_time, short_duration,
    skeleton, sparkline_chip, thumb,
};
use crate::ui::shell::{instance_icon, running_chip};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Content,
    Health,
    Worlds,
    Screenshots,
    Snapshots,
    Logs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Filter {
    All,
    Mods,
    ResourcePacks,
    Shaders,
    DataPacks,
}

impl Filter {
    fn label(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Mods => "Mods",
            Filter::ResourcePacks => "Resource packs",
            Filter::Shaders => "Shaders",
            Filter::DataPacks => "Data packs",
        }
    }
    fn matches(self, t: ProjectType) -> bool {
        match self {
            Filter::All => true,
            Filter::Mods => t == ProjectType::Mod,
            Filter::ResourcePacks => t == ProjectType::ResourcePack,
            Filter::Shaders => t == ProjectType::ShaderPack,
            Filter::DataPacks => t == ProjectType::DataPack,
        }
    }
}

#[derive(Clone, Debug)]
struct Row {
    path: String,
    title: SharedString,
    author: Option<SharedString>,
    version: SharedString,
    file_name: SharedString,
    icon_url: Option<String>,
    enabled: bool,
    has_update: bool,
    update_version: Option<String>,
    project_id: Option<String>,
    kind: ProjectType,
    /// Part of the instance's modpack rather than added by the player.
    from_pack: bool,
}

impl Row {
    fn from_item(item: ContentItem) -> Self {
        let file_name = item.file_name.trim_end_matches(".disabled").to_string();
        Row {
            title: item
                .project
                .as_ref()
                .map(|p| p.title.clone())
                .unwrap_or_else(|| file_name.clone())
                .into(),
            author: item.owner.as_ref().map(|o| o.name.clone().into()),
            version: item
                .version
                .as_ref()
                .map(|v| v.version_number.clone())
                .unwrap_or_default()
                .into(),
            icon_url: item.project.as_ref().and_then(|p| p.icon_url.clone()),
            enabled: item.enabled,
            has_update: item.has_update,
            update_version: item.update_version_id.clone(),
            project_id: item.project.as_ref().map(|p| p.id.clone()),
            kind: item.project_type,
            from_pack: false,
            path: item.file_path,
            file_name: file_name.into(),
        }
    }
}

pub enum InstancePageEvent {
    /// Open Discover filtered to this instance.
    AddContent(String),
    OpenProject(String),
    OpenSettings(String),
}

impl gpui_kit::EventEmitter<InstancePageEvent> for InstancePage {}

pub struct InstancePage {
    id: String,
    seen_revision: u64,
    tab: Tab,
    filter: Filter,
    content: Option<Result<Vec<Row>, String>>,
    busy_paths: Vec<String>,
    log_scroll: UniformListScrollHandle,
    follow_logs: bool,
    state: Entity<AppState>,
    pub(crate) extra: crate::ui::pages::instance_tabs::Extra,
    /// The modpack's own files (modpack instances only).
    pack_rows: Vec<Row>,
    pack_loading: bool,
    _subscriptions: Vec<Subscription>,
}

impl InstancePage {
    pub fn new(id: String, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let subscriptions = vec![cx.observe(&state, |this, state, cx| {
            // Reload content whenever the backend says it changed.
            let revision = state
                .read(cx)
                .content_revision
                .get(&this.id)
                .copied()
                .unwrap_or(0);
            if revision != this.seen_revision {
                this.seen_revision = revision;
                this.load_content(cx);
            }
            if this.follow_logs && this.tab == Tab::Logs {
                this.log_scroll
                    .scroll_to_item(usize::MAX, gpui_kit::ScrollStrategy::Bottom);
            }
            cx.notify()
        })];
        let seen_revision = state
            .read(cx)
            .content_revision
            .get(&id)
            .copied()
            .unwrap_or(0);
        let mut page = InstancePage {
            seen_revision,
            id,
            tab: Tab::Content,
            filter: Filter::All,
            content: None,
            busy_paths: Vec::new(),
            log_scroll: UniformListScrollHandle::new(),
            follow_logs: true,
            state,
            extra: Default::default(),
            pack_rows: Vec::new(),
            pack_loading: false,
            _subscriptions: subscriptions,
        };
        page.load_content(cx);
        page.load_log_buffer(cx);
        page.check_updates(cx);
        page.load_health(cx);
        page.resume_bisect();
        page
    }

    /// Ask Modrinth for newer versions of everything, then reload.
    fn check_updates(&mut self, cx: &mut Context<Self>) {
        let id = self.id.clone();
        let check = Backend::global(cx)
            .call(async move { theseus::instance::refresh_content_updates(&id).await });
        cx.spawn(async move |this, cx| {
            if check.await.is_ok() {
                let _ = this.update(cx, |this, cx| this.load_content(cx));
            }
        })
        .detach();
    }

    fn update_one(&mut self, path: String, cx: &mut Context<Self>) {
        let id = self.id.clone();
        self.busy_paths.push(path.clone());
        cx.notify();
        let call = Backend::global(cx)
            .call(async move { theseus::instance::update_project(&id, &path, None).await });
        self.after(call, "Couldn't update that", cx);
    }

    fn update_all(&mut self, cx: &mut Context<Self>) {
        let Some(Ok(rows)) = &self.content else {
            return;
        };
        let updates: Vec<theseus::install::ContentUpdateSelection> = rows
            .iter()
            .filter(|r| r.has_update)
            .filter_map(|r| {
                Some(theseus::install::ContentUpdateSelection {
                    project_path: r.path.clone(),
                    version_id: r.update_version.clone()?,
                })
            })
            .collect();
        if updates.is_empty() {
            return;
        }
        self.busy_paths
            .extend(updates.iter().map(|u| u.project_path.clone()));
        cx.notify();
        let id = self.id.clone();
        let state = self.state.clone();
        let snapshots = crate::ui::pages::instance_tabs::snapshots_dir(&id);
        let count = updates.len();
        let call = Backend::global(cx).call(async move {
            // Save what's there first so the update can be rolled back.
            if let Some(snaps) = snapshots
                && let Ok(dir) = theseus::instance::get_full_path(&id).await
            {
                let label = format!(
                    "Before updating {count} mod{}",
                    if count == 1 { "" } else { "s" }
                );
                let _ = tokio::task::spawn_blocking(move || {
                    let result = calcite_core::snapshots::take(&dir, &snaps, &label, true);
                    calcite_core::snapshots::prune_automatic(&snaps, 10);
                    result
                })
                .await;
            }
            theseus::install::bulk_update_content(id, updates).await
        });
        cx.spawn(async move |this, cx| {
            let result = call.await;
            state.update(cx, |state, cx| {
                match result {
                    Ok(snapshot) => state.track_job(snapshot, cx),
                    Err(error) => {
                        state.notify_error("Couldn't start the update", format!("{error:#}"))
                    }
                }
                cx.notify();
            });
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }

    pub(crate) fn state(&self) -> &Entity<AppState> {
        &self.state
    }

    fn is_modpack(&self, cx: &App) -> bool {
        self.state
            .read(cx)
            .instance(&self.id)
            .is_some_and(|i| i.is_modpack)
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.tab = tab;
        match tab {
            Tab::Worlds => self.load_worlds(cx),
            Tab::Health => self.load_health(cx),
            Tab::Screenshots => self.load_shots(cx),
            Tab::Snapshots => self.load_snapshots(cx),
            _ => {}
        }
        cx.notify();
    }

    pub(crate) fn load_content(&mut self, cx: &mut Context<Self>) {
        let id = self.id.clone();
        let fetch = Backend::global(cx).call(async move {
            // Pick up files added outside Calcite (dropped into the mods folder).
            if let Err(error) = theseus::instance::sync_content_files(&id).await {
                tracing::warn!("content sync for {id} failed: {error}");
            }
            let added = theseus::instance::get_content_items(&id, None).await?;
            Ok((added, Vec::new()))
        });
        // Phase 2: a modpack's own files. The first lookup per pack asks
        // Modrinth about every file, so it arrives separately.
        if self.is_modpack(cx) {
            self.pack_loading = true;
            let id = self.id.clone();
            let pack = Backend::global(cx).call(async move {
                theseus::instance::get_linked_modpack_content(&id, None).await
            });
            cx.spawn(async move |this, cx| {
                let result = pack.await;
                let _ = this.update(cx, |this, cx| {
                    this.pack_loading = false;
                    if let Ok(items) = result {
                        let mut pack_rows: Vec<Row> = items
                            .into_iter()
                            .map(|item| Row {
                                from_pack: true,
                                ..Row::from_item(item)
                            })
                            .collect();
                        pack_rows.sort_by_key(|r| r.title.to_lowercase());
                        this.pack_rows = pack_rows;
                    }
                    cx.notify();
                });
            })
            .detach();
        }
        cx.spawn(async move |this, cx| {
            let result = fetch.await;
            let _ = this.update(cx, |this, cx| {
                this.content = Some(
                    result
                        .map(|(added, from_pack)| {
                            let mut rows: Vec<Row> =
                                added.into_iter().map(Row::from_item).collect();
                            let mut pack_rows: Vec<Row> = from_pack
                                .into_iter()
                                .map(|item| Row {
                                    from_pack: true,
                                    ..Row::from_item(item)
                                })
                                .filter(|p| !rows.iter().any(|r| r.path == p.path))
                                .collect();
                            rows.sort_by_key(|r| r.title.to_lowercase());
                            pack_rows.sort_by_key(|r| r.title.to_lowercase());
                            rows.extend(pack_rows);
                            rows
                        })
                        .map_err(|e| format!("{e:#}")),
                );
                this.busy_paths.clear();
                cx.notify();
            });
        })
        .detach();
    }

    /// Lines the game printed earlier in this session (before this page opened).
    fn load_log_buffer(&mut self, cx: &mut Context<Self>) {
        if self.state.read(cx).logs.contains_key(&self.id) {
            return;
        }
        let id = self.id.clone();
        let fetch =
            Backend::global(cx).call(async move { theseus::logs::get_live_log_buffer(&id).await });
        let state = self.state.clone();
        let id = self.id.clone();
        cx.spawn(async move |_, cx| {
            if let Ok(buffer) = fetch.await {
                let text = serde_json::to_value(&buffer)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_default();
                if text.trim().is_empty() {
                    return;
                }
                state.update(cx, |state, cx| {
                    let lines = text.lines().map(parse_legacy_line).collect();
                    state.logs.entry(id).or_insert(lines);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn toggle(&mut self, path: String, enabled: bool, cx: &mut Context<Self>) {
        let id = self.id.clone();
        self.busy_paths.push(path.clone());
        cx.notify();
        let call = Backend::global(cx).call(async move {
            theseus::instance::toggle_disable_project(&id, &path, Some(!enabled)).await
        });
        self.after(call, "Couldn't change that file", cx);
    }

    fn remove(&mut self, path: String, cx: &mut Context<Self>) {
        let id = self.id.clone();
        self.busy_paths.push(path.clone());
        cx.notify();
        let call = Backend::global(cx)
            .call(async move { theseus::instance::remove_project(&id, &path).await });
        self.after(call, "Couldn't remove that file", cx);
    }

    fn add_files(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Add".into()),
        });
        let id = self.id.clone();
        let backend = Backend::global(cx).clone();
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let result = backend
                .call(async move {
                    for path in paths {
                        theseus::instance::add_project_from_path(&id, &path, None).await?;
                    }
                    Ok(())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = result {
                    this.state.update(cx, |s, cx| {
                        s.notify_error("Couldn't add those files", format!("{error:#}"));
                        cx.notify();
                    });
                }
                this.load_content(cx);
            });
        })
        .detach();
    }

    fn open_folder(&mut self, cx: &mut Context<Self>) {
        let id = self.id.clone();
        let fetch =
            Backend::global(cx).call(async move { theseus::instance::get_full_path(&id).await });
        cx.spawn(async move |_, cx| {
            if let Ok(path) = fetch.await {
                cx.update(|cx| cx.open_with_system(&path));
            }
        })
        .detach();
    }

    fn after(
        &mut self,
        call: impl std::future::Future<Output = anyhow::Result<impl Sized + 'static>> + 'static,
        failure: &'static str,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let result = call.await;
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = result {
                    this.state.update(cx, |s, cx| {
                        s.notify_error(failure, format!("{error:#}"));
                        cx.notify();
                    });
                }
                this.load_content(cx);
            });
        })
        .detach();
    }

    // ------------------------------------------------------------ render

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();
        let state = self.state.read(cx);
        let Some(inst) = state.instance(&self.id).cloned() else {
            return div();
        };
        let running = state
            .running
            .get(&self.id)
            .map(|r| (Utc::now() - r.started).num_seconds().max(0) as u64);
        let usage: Vec<calcite_core::monitor::Sample> = state
            .running
            .get(&self.id)
            .map(|r| r.usage.iter().copied().collect())
            .unwrap_or_default();
        let job = state.job_for(&self.id).cloned();
        let source = instance_icon(&inst);
        let busy = job.is_some() || !inst.installed;
        let id = self.id.clone();

        let action = if running.is_some() {
            Button::new("instance-stop")
                .icon("square")
                .label("Stop")
                .danger()
                .size(ButtonSize::Large)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    let id = id.clone();
                    this.state.update(cx, |s, cx| s.stop(&id, cx));
                }))
        } else {
            Button::new("instance-play")
                .icon("play")
                .label(if busy { "Installing…" } else { "Play" })
                .primary()
                .disabled(busy)
                .size(ButtonSize::Large)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    let id = id.clone();
                    this.state.update(cx, |s, cx| s.play(&id, cx));
                }))
        };

        div()
            .flex()
            .flex_col()
            .gap(space(4.0))
            .px(space(8.0))
            .pt(space(7.0))
            .pb(space(5.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space(4.0))
                    .child(thumb(source, &inst.name, 64.0, radius::LG, cx))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(space(2.0))
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(type_scale::TITLE))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(p.text)
                                    .truncate()
                                    .child(inst.name.clone()),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .items_center()
                                    .gap(space(1.5))
                                    .child(chip(inst.loader_line(), cx))
                                    .when_some(inst.loader_version.clone(), |d, v| {
                                        d.child(chip(format!("Loader {v}"), cx))
                                    })
                                    .when(inst.played_secs > 0, |d| {
                                        d.child(icon_chip(
                                            "clock",
                                            short_duration(inst.played_secs),
                                            cx,
                                        ))
                                    })
                                    .when_some(
                                        inst.last_played.filter(|_| running.is_none()),
                                        |d, t| {
                                            d.child(
                                                div()
                                                    .text_size(px(type_scale::SMALL))
                                                    .text_color(p.text_muted)
                                                    .child(format!("Played {}", relative_time(t))),
                                            )
                                        },
                                    )
                                    .when_some(running, |d, secs| d.child(running_chip(secs, cx))),
                            )
                            .when_some(usage.last().copied(), |d, now| {
                                let memory: Vec<f32> =
                                    usage.iter().map(|s| s.memory as f32).collect();
                                let cpu: Vec<f32> = usage.iter().map(|s| s.cpu).collect();
                                let memory_max = memory.iter().copied().fold(0.0, f32::max);
                                let cpu_max = cpu.iter().copied().fold(25.0, f32::max);
                                d.child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(space(1.5))
                                        .child(sparkline_chip(
                                            "memory-stick",
                                            calcite_core::transfer::human_bytes(now.memory),
                                            &memory,
                                            memory_max,
                                            cx,
                                        ))
                                        .child(sparkline_chip(
                                            "cpu",
                                            format!("{:.0}% CPU", now.cpu),
                                            &cpu,
                                            cpu_max,
                                            cx,
                                        )),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(space(2.0))
                            .child(
                                Button::new("instance-settings")
                                    .icon("settings")
                                    .ghost()
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                        cx.emit(InstancePageEvent::OpenSettings(this.id.clone()))
                                    })),
                            )
                            .child(
                                Button::new("instance-folder")
                                    .icon("folder-open")
                                    .ghost()
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.open_folder(cx)
                                    })),
                            )
                            .child(action),
                    ),
            )
            .when_some(job, |d, job| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(space(2.0))
                        .p(space(3.0))
                        .rounded(px(radius::MD))
                        .bg(p.surface)
                        .border_1()
                        .border_color(p.border)
                        .child(
                            div()
                                .flex()
                                .justify_between()
                                .text_size(px(type_scale::SMALL))
                                .child(div().text_color(p.text).child(job.phase))
                                .child(div().text_color(p.text_muted).child(if job.total > 0 {
                                    format!("{}%", (job.fraction() * 100.0).round() as u32)
                                } else {
                                    String::new()
                                })),
                        )
                        .child(progress_bar(job.fraction(), cx)),
                )
            })
    }

    fn tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();
        let count = self
            .content
            .as_ref()
            .and_then(|c| c.as_ref().ok())
            .map(|added| added.len() + self.pack_rows.len());
        let mut row = div()
            .flex()
            .items_end()
            .gap(space(5.0))
            .h(px(40.0))
            .px(space(8.0))
            .border_b_1()
            .border_color(p.border);
        for (tab, label, badge) in [
            (
                Tab::Content,
                "Content",
                count.filter(|c| *c > 0).map(|c| c.to_string()),
            ),
            (
                Tab::Health,
                "Health",
                self.extra
                    .health
                    .as_ref()
                    .and_then(|h| h.as_ref().ok())
                    .map(|h| h.issues.len())
                    .filter(|n| *n > 0)
                    .map(|n| n.to_string()),
            ),
            (Tab::Worlds, "Worlds", None),
            (Tab::Screenshots, "Screenshots", None),
            (Tab::Snapshots, "Snapshots", None),
            (Tab::Logs, "Logs", None),
        ] {
            let active = tab == self.tab;
            row = row.child(
                div()
                    .id(SharedString::from(format!("tab-{label}")))
                    .flex()
                    .items_center()
                    .gap(space(1.5))
                    .h_full()
                    .border_b_2()
                    .border_color(if active {
                        p.accent
                    } else {
                        gpui_kit::Hsla::transparent_black()
                    })
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.set_tab(tab, cx)))
                    .child(
                        div()
                            .text_size(px(type_scale::BODY))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if active { p.text } else { p.text_secondary })
                            .child(label),
                    )
                    .when_some(badge, |d, b| {
                        d.child(
                            div()
                                .px(space(1.5))
                                .rounded(px(radius::SM))
                                .bg(p.raised)
                                .text_size(px(type_scale::CAPTION))
                                .text_color(p.text_muted)
                                .child(b),
                        )
                    }),
            );
        }
        row
    }

    fn content_tab(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let p = cx.palette().clone();
        let rows = match &self.content {
            None => {
                let mut list = div().flex().flex_col().px(space(8.0)).pt(space(3.0));
                for _ in 0..6 {
                    list = list.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(space(3.0))
                            .h(px(52.0))
                            .child(skeleton(px(28.0), px(16.0), cx))
                            .child(skeleton(px(32.0), px(32.0), cx))
                            .child(skeleton(px(220.0), px(14.0), cx)),
                    );
                }
                return list.into_any_element();
            }
            Some(Err(error)) => {
                return empty_state(
                    "circle-alert",
                    "Couldn't read this instance's content",
                    error.clone(),
                    None,
                    cx,
                )
                .into_any_element();
            }
            Some(Ok(rows)) => {
                let mut rows = rows.clone();
                rows.extend(
                    self.pack_rows
                        .iter()
                        .filter(|p| !rows.iter().any(|r| r.path == p.path))
                        .cloned()
                        .collect::<Vec<_>>(),
                );
                rows
            }
        };

        let counts = |f: Filter| rows.iter().filter(|r| f.matches(r.kind)).count();
        let mut filters = div().flex().items_center().gap(space(1.0));
        for f in [
            Filter::All,
            Filter::Mods,
            Filter::ResourcePacks,
            Filter::Shaders,
            Filter::DataPacks,
        ] {
            let n = counts(f);
            if f != Filter::All && n == 0 {
                continue;
            }
            let active = f == self.filter;
            filters = filters.child(
                div()
                    .id(SharedString::from(format!("filter-{}", f.label())))
                    .flex()
                    .items_center()
                    .gap(space(1.5))
                    .h(px(28.0))
                    .px(space(2.5))
                    .rounded(px(radius::SM))
                    .text_size(px(type_scale::SMALL))
                    .font_weight(FontWeight::MEDIUM)
                    .cursor_pointer()
                    .when(active, |d| d.bg(p.raised).text_color(p.text))
                    .when(!active, |d| {
                        d.text_color(p.text_secondary)
                            .hover(|s| s.text_color(p.text))
                    })
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.filter = f;
                        cx.notify();
                    }))
                    .child(f.label())
                    .child(div().text_color(p.text_muted).child(n.to_string())),
            );
        }

        let updates = rows
            .iter()
            .filter(|r| r.has_update && r.update_version.is_some())
            .count();
        let toolbar = div()
            .flex()
            .items_center()
            .gap(space(2.0))
            .px(space(8.0))
            .py(space(3.0))
            .child(filters)
            .child(div().flex_1())
            .when(updates > 0, |d| {
                d.child(
                    Button::new("update-all")
                        .icon("refresh-cw")
                        .label(format!("Update {updates}"))
                        .small()
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.update_all(cx))),
                )
            })
            .child(
                Button::new("add-files")
                    .icon("folder-open")
                    .label("Add files")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.add_files(cx))),
            )
            .child(
                Button::new("add-content")
                    .icon("plus")
                    .label("Add content")
                    .small()
                    .primary()
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        cx.emit(InstancePageEvent::AddContent(this.id.clone()))
                    })),
            );

        let visible: Vec<Row> = rows
            .into_iter()
            .filter(|r| self.filter.matches(r.kind))
            .collect();
        if visible.is_empty() && !self.pack_loading {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .child(toolbar)
                .child(empty_state(
                    "package",
                    "Nothing installed yet",
                    "Use Add content to browse mods, packs and shaders that fit this instance, or add files from your computer.",
                    None,
                    cx,
                ))
                .into_any_element();
        }

        let mut list = div()
            .id("content-list")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(space(8.0))
            .pb(space(8.0));
        let has_pack = visible.iter().any(|r| r.from_pack);
        let has_added = visible.iter().any(|r| !r.from_pack);
        let heading = |text: String| {
            div()
                .pt(space(4.0))
                .pb(space(1.5))
                .px(space(3.0))
                .text_size(px(type_scale::SMALL))
                .font_weight(FontWeight::MEDIUM)
                .text_color(p.text_muted)
                .child(text)
        };
        for (ix, row) in visible.iter().enumerate() {
            if has_pack && has_added && ix == 0 {
                list = list.child(heading("Added by you".into()));
            }
            if has_pack && row.from_pack && (ix == 0 || !visible[ix - 1].from_pack) {
                let count = visible.iter().filter(|r| r.from_pack).count();
                list = list.child(heading(format!("From the modpack · {count}")));
            }
            let source = row
                .icon_url
                .as_deref()
                .and_then(|url| Images::get(url, 64, cx));
            let busy = self.busy_paths.contains(&row.path);
            let (toggle_path, remove_path, enabled) =
                (row.path.clone(), row.path.clone(), row.enabled);
            list = list.child(
                div()
                    .id(ix)
                    .flex()
                    .items_center()
                    .gap(space(3.0))
                    .h(px(52.0))
                    .px(space(3.0))
                    .border_b_1()
                    .border_color(p.border)
                    .hover(|s| s.bg(p.surface))
                    .when(busy, |d| d.opacity(0.5))
                    .child(
                        div()
                            .id(SharedString::from(format!("toggle-{ix}")))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.toggle(toggle_path.clone(), enabled, cx)
                            }))
                            .child(toggle(row.enabled, cx)),
                    )
                    .child(div().when(!row.enabled, |d| d.opacity(0.45)).child(thumb(
                        source,
                        &row.title,
                        32.0,
                        radius::SM,
                        cx,
                    )))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .when(!row.enabled, |d| d.opacity(0.55))
                            .child(
                                div()
                                    .flex()
                                    .items_baseline()
                                    .gap(space(2.0))
                                    .min_w_0()
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("title-{ix}")))
                                            .truncate()
                                            .text_size(px(type_scale::BODY))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(p.text)
                                            .when_some(row.project_id.clone(), |d, project| {
                                                d.cursor_pointer()
                                                    .hover(|s| s.text_color(p.accent_text))
                                                    .on_click(cx.listener(
                                                        move |_, _: &ClickEvent, _, cx| {
                                                            cx.emit(InstancePageEvent::OpenProject(
                                                                project.clone(),
                                                            ))
                                                        },
                                                    ))
                                            })
                                            .child(row.title.clone()),
                                    )
                                    .when_some(row.author.clone(), |d, a| {
                                        d.child(
                                            div()
                                                .flex_none()
                                                .text_size(px(type_scale::SMALL))
                                                .text_color(p.text_muted)
                                                .child(format!("by {a}")),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(px(type_scale::CAPTION))
                                    .font_family(theme::fonts::MONO)
                                    .text_color(p.text_muted)
                                    .truncate()
                                    .child(row.file_name.clone()),
                            ),
                    )
                    .child(
                        div()
                            .w(px(140.0))
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(space(2.0))
                            .when(row.has_update && row.update_version.is_some(), |d| {
                                let path = row.path.clone();
                                d.child(
                                    Button::new(SharedString::from(format!("update-{ix}")))
                                        .icon("refresh-cw")
                                        .label("Update")
                                        .small()
                                        .on_click(cx.listener(
                                            move |this, _: &ClickEvent, _, cx| {
                                                this.update_one(path.clone(), cx)
                                            },
                                        )),
                                )
                            })
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(type_scale::SMALL))
                                    .text_color(p.text_secondary)
                                    .child(row.version.clone()),
                            ),
                    )
                    // Pack files come back with the next modpack update, so
                    // they can be turned off but not removed.
                    .child(if row.from_pack {
                        div().w(px(28.0)).into_any_element()
                    } else {
                        Button::new(SharedString::from(format!("remove-{ix}")))
                            .icon("trash")
                            .ghost()
                            .small()
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.remove(remove_path.clone(), cx)
                            }))
                            .into_any_element()
                    }),
            );
        }

        if self.pack_loading && !visible.iter().any(|r| r.from_pack) {
            list = list.child(heading("From the modpack · loading…".into()));
            for _ in 0..3 {
                list = list.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space(3.0))
                        .h(px(52.0))
                        .px(space(3.0))
                        .child(skeleton(px(28.0), px(16.0), cx))
                        .child(skeleton(px(32.0), px(32.0), cx))
                        .child(skeleton(px(220.0), px(14.0), cx)),
                );
            }
        }

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(toolbar)
            .child(list)
            .into_any_element()
    }

    fn logs_tab(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let p = cx.palette().clone();
        let lines: Vec<LogLine> = self
            .state
            .read(cx)
            .logs
            .get(&self.id)
            .map(|b| b.iter().cloned().collect())
            .unwrap_or_default();
        let running = self.state.read(cx).running.contains_key(&self.id);

        let toolbar = div()
            .flex()
            .items_center()
            .gap(space(2.0))
            .px(space(8.0))
            .pt(space(4.0))
            .child(chip(if running { "Live" } else { "This session" }, cx))
            .child(
                div()
                    .text_size(px(type_scale::SMALL))
                    .text_color(p.text_muted)
                    .child(format!("{} lines", lines.len())),
            )
            .child(div().flex_1())
            .child(
                Button::new("follow-logs")
                    .icon("arrow-down-to-line")
                    .label(if self.follow_logs {
                        "Following"
                    } else {
                        "Follow"
                    })
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.follow_logs = !this.follow_logs;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("copy-logs")
                    .icon("copy")
                    .label("Copy")
                    .small()
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        let text = this
                            .state
                            .read(cx)
                            .logs
                            .get(&this.id)
                            .map(|b| {
                                b.iter()
                                    .map(|l| format!("[{}] {}", l.time, l.text))
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            })
                            .unwrap_or_default();
                        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(text));
                    })),
            );

        if lines.is_empty() {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .child(toolbar)
                .child(empty_state(
                    "square-terminal",
                    "No output yet",
                    "Press Play and the game's log appears here as it runs.",
                    None,
                    cx,
                ))
                .into_any_element();
        }

        let list = uniform_list("log-lines", lines.len(), move |range, _window, cx| {
            let p = cx.palette().clone();
            range
                .map(|ix| {
                    let line = &lines[ix];
                    let (color, bg) = match line.level {
                        LogLevel::Warn => (p.warning, Some(p.warning_soft)),
                        LogLevel::Error => (p.danger, Some(p.danger_soft)),
                        LogLevel::Debug => (p.text_disabled, None),
                        LogLevel::Info => (p.text_muted, None),
                    };
                    let level = match line.level {
                        LogLevel::Warn => "WARN",
                        LogLevel::Error => "ERROR",
                        LogLevel::Debug => "DEBUG",
                        LogLevel::Info => "INFO",
                    };
                    div()
                        .id(ix)
                        .flex()
                        .gap(space(3.0))
                        .h(px(20.0))
                        .px(space(2.0))
                        .rounded(px(4.0))
                        .when_some(bg, |d, b| d.bg(b))
                        .child(
                            div()
                                .flex_none()
                                .w(px(64.0))
                                .text_color(p.text_disabled)
                                .child(line.time.clone()),
                        )
                        .child(div().flex_none().w(px(44.0)).text_color(color).child(level))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(p.text_secondary)
                                .child(line.text.clone()),
                        )
                })
                .collect()
        })
        .track_scroll(&self.log_scroll)
        .with_sizing_behavior(ListSizingBehavior::Infer)
        .flex_1();

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(toolbar)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .m(space(8.0))
                    .mt(space(3.0))
                    .p(space(2.0))
                    .rounded(px(radius::LG))
                    .bg(p.surface)
                    .border_1()
                    .border_color(p.border)
                    .font_family(theme::fonts::MONO)
                    .text_size(px(type_scale::SMALL))
                    .child(list),
            )
            .into_any_element()
    }
}

impl Render for InstancePage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.state.read(cx).instance(&self.id).is_none() {
            return div()
                .size_full()
                .child(empty_state(
                    "circle-alert",
                    "This instance is gone",
                    "It may have been deleted. Pick another one from your library.",
                    None,
                    cx,
                ))
                .into_any_element();
        }
        let body = match self.tab {
            Tab::Content => self.content_tab(cx),
            Tab::Worlds => self.worlds_tab(cx),
            Tab::Health => self.health_tab(cx),
            Tab::Screenshots => self.shots_tab(cx),
            Tab::Snapshots => self.snapshots_tab(cx),
            Tab::Logs => self.logs_tab(cx),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.header(cx))
            .child(self.tabs(cx))
            .child(div().flex_1().min_h_0().child(body))
            .into_any_element()
    }
}

fn toggle(on: bool, cx: &App) -> impl IntoElement {
    let p = cx.palette();
    div()
        .w(px(28.0))
        .h(px(16.0))
        .flex_none()
        .rounded_full()
        .p(px(2.0))
        .bg(if on { p.accent } else { p.border_strong })
        .flex()
        .when(on, |d| d.justify_end())
        .child(div().size(px(12.0)).rounded_full().bg(if on {
            p.on_accent
        } else {
            p.text_secondary
        }))
}

use gpui_kit::App;
