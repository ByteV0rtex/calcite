//! A single instance: header with Play, then its content and live logs.

use chrono::Utc;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button as KitButton, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt, DropdownMenu as _, PopupMenu, PopupMenuItem};
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
    Button, ButtonSize, chip, empty_state, icon, icon_chip, progress_bar, relative_time,
    short_duration, skeleton, sparkline_chip, thumb,
};
use crate::ui::shell::{instance_icon, running_chip};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Content,
    Health,
    Worlds,
    Screenshots,
    Snapshots,
    Files,
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
    /// Frozen: updates skip it and its version can't change.
    locked: bool,
    version_id: Option<String>,
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
            locked: item.locked,
            version_id: item.version.as_ref().map(|v| v.id.clone()),
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
    Share(String),
    /// Add (None) or edit a server in this instance's list.
    Server(String, Option<crate::ui::server_dialog::Existing>),
    /// Rename a world: instance folder, world folder, current name.
    RenameWorld(std::path::PathBuf, String, String),
    /// Pick another version: of one file, or (None) of the modpack.
    PickVersion(String, Option<crate::ui::version_picker::PickFile>),
}

/// The modpack this instance follows.
#[derive(Clone)]
struct PackInfo {
    title: SharedString,
    icon_url: Option<String>,
    version: Option<SharedString>,
    /// The newer version this instance's update channel allows.
    update: Option<SharedString>,
    update_id: Option<String>,
    project_id: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Bulk {
    Enable,
    Disable,
    Update,
    Remove,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PackConfirm {
    Reinstall,
    Unlink,
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
    /// Changes the owner published that this copy doesn't have yet
    /// (shared instances you joined).
    shared_update: Option<usize>,
    files: Option<Entity<crate::ui::pages::files_tab::FilesBrowser>>,
    more_open: bool,
    pack: Option<PackInfo>,
    /// Show the modpack's own files in the list.
    pack_open: bool,
    pack_confirm: Option<PackConfirm>,
    /// Rows picked for a bulk action, by path.
    selected: std::collections::HashSet<String>,
    /// Filters the content list.
    query: Entity<InputState>,
    /// The log being shown: the live one (None) or a saved file.
    log_file: Option<(theseus::logs::LogType, String)>,
    /// Saved logs and crash reports, newest first.
    log_files: Option<Vec<(theseus::logs::LogType, String)>>,
    old_log: Option<Result<Vec<LogLine>, String>>,
    log_query: Entity<InputState>,
    /// Only lines of this level (None: all).
    log_level: Option<LogLevel>,
    sharing_log: bool,
    _subscriptions: Vec<Subscription>,
}

/// What a ⋯ menu item does.
type MoreAction = Box<dyn Fn(&mut InstancePage, &mut Context<InstancePage>)>;

impl InstancePage {
    pub fn new(id: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Search content"));
        let log_query = cx.new(|cx| InputState::new(window, cx).placeholder("Search the log"));
        let mut subscriptions = vec![
            cx.subscribe(&query, |_, _, event: &InputEvent, cx| {
                if let InputEvent::Change = event {
                    cx.notify();
                }
            }),
            cx.subscribe(&log_query, |_, _, event: &InputEvent, cx| {
                if let InputEvent::Change = event {
                    cx.notify();
                }
            }),
        ];
        subscriptions.push(cx.observe(&state, |this, state, cx| {
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
        }));
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
            shared_update: None,
            files: None,
            more_open: false,
            pack: None,
            pack_open: false,
            pack_confirm: None,
            selected: Default::default(),
            query,
            log_file: None,
            log_files: None,
            old_log: None,
            log_query,
            log_level: None,
            sharing_log: false,
            _subscriptions: subscriptions,
        };
        page.load_content(cx);
        page.load_log_buffer(cx);
        page.check_updates(cx);
        page.load_health(cx);
        page.resume_bisect();
        page.check_shared_update(cx);
        page
    }

    /// The ⋯ button and its menu: less frequent things to do with this
    /// instance.
    fn more_menu(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let p = cx.palette().clone();
        let id = self.id.clone();
        let name = self
            .state
            .read(cx)
            .instance(&id)
            .map(|i| i.name.to_string())
            .unwrap_or_default();
        let hidden = cx
            .global::<crate::prefs::Prefs>()
            .hidden_instances
            .contains(&id);
        let item = |key: &'static str,
                    glyph: &'static str,
                    label: &'static str,
                    cx: &mut Context<Self>,
                    act: MoreAction| {
            div()
                .id(key)
                .flex()
                .items_center()
                .gap(space(2.0))
                .h(px(30.0))
                .px(space(2.5))
                .rounded(px(radius::SM))
                .text_size(px(type_scale::BODY))
                .text_color(p.text_secondary)
                .cursor_pointer()
                .hover(|s| s.bg(p.hover).text_color(p.text))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.more_open = false;
                    act(this, cx);
                    cx.notify();
                }))
                .child(icon(glyph, 14.0, p.text_muted))
                .child(label)
        };
        let menu = self.more_open.then(|| {
            let (a, b, c, d, e, f) = (
                id.clone(),
                id.clone(),
                id.clone(),
                id.clone(),
                id.clone(),
                id.clone(),
            );
            let export_name = name.clone();
            let panel = div()
                .id("instance-more-menu")
                .absolute()
                .top(px(36.0))
                .right(px(0.0))
                .min_w(px(220.0))
                .p(space(1.0))
                .rounded(px(radius::MD))
                .bg(p.overlay)
                .border_1()
                .border_color(p.border_strong)
                .shadow_lg()
                .flex()
                .flex_col()
                .on_mouse_down_out(cx.listener(|this, _: &gpui_kit::MouseDownEvent, _, cx| {
                    this.more_open = false;
                    cx.notify();
                }))
                .child(item(
                    "more-export",
                    "share-2",
                    "Export as .mrpack…",
                    cx,
                    Box::new(move |_, cx| {
                        crate::ui::instance_settings::export_instance(
                            a.clone(),
                            export_name.clone(),
                            cx,
                        )
                    }),
                ))
                .child(item(
                    "more-duplicate",
                    "copy",
                    "Duplicate",
                    cx,
                    Box::new(move |this, cx| {
                        let id = b.clone();
                        this.state.update(cx, |s, cx| s.duplicate(id, cx));
                    }),
                ))
                .child(item(
                    "more-shortcut",
                    "monitor",
                    "Create desktop shortcut",
                    cx,
                    Box::new(move |this, cx| {
                        let id = c.clone();
                        this.state.update(cx, |s, cx| s.make_shortcut(id, cx));
                    }),
                ))
                .child(item(
                    "more-repair",
                    "wrench",
                    "Repair",
                    cx,
                    Box::new(move |this, cx| {
                        let id = d.clone();
                        this.state.update(cx, |s, cx| s.repair(id, cx));
                    }),
                ))
                .child(item(
                    "more-hide",
                    if hidden { "eye" } else { "eye-off" },
                    if hidden {
                        "Show in Library"
                    } else {
                        "Hide from Library"
                    },
                    cx,
                    Box::new(move |_, cx| {
                        crate::prefs::Prefs::set_hidden(std::slice::from_ref(&e), !hidden, cx);
                    }),
                ))
                .child(div().h(px(1.0)).my(space(1.0)).bg(p.border))
                .child(item(
                    "more-delete",
                    "trash",
                    "Delete…",
                    cx,
                    Box::new(move |_, cx| cx.emit(InstancePageEvent::OpenSettings(f.clone()))),
                ));
            gpui_kit::deferred(panel).with_priority(1)
        });
        div()
            .relative()
            .child(
                Button::new("instance-more")
                    .icon("ellipsis")
                    .ghost()
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.more_open = !this.more_open;
                        cx.notify();
                    })),
            )
            .children(menu)
            .into_any_element()
    }

    /// For a shared instance you joined: has the owner published changes?
    fn check_shared_update(&mut self, cx: &mut Context<Self>) {
        let joined = self
            .state
            .read(cx)
            .instance(&self.id)
            .and_then(|i| i.shared.clone())
            .is_some_and(|s| !s.owner);
        if !joined {
            return;
        }
        let id = self.id.clone();
        let task = Backend::global(cx)
            .call(async move { theseus::instance::get_shared_instance_update_preview(&id).await });
        cx.spawn(async move |this, cx| {
            let Ok(Some(preview)) = task.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.shared_update = preview
                    .update_available
                    .then_some(preview.diffs.len().max(1));
                cx.notify();
            });
        })
        .detach();
    }

    fn apply_shared_update(&mut self, cx: &mut Context<Self>) {
        self.shared_update = None;
        let id = self.id.clone();
        let task = Backend::global(cx).call(async move {
            theseus::instance::update_shared_instance(&id)
                .await
                .map(|_| ())
        });
        cx.spawn(async move |this, cx| {
            if let Err(error) = task.await {
                let _ = this.update(cx, |this, cx| {
                    this.state.update(cx, |s, cx| {
                        s.notify_error("Couldn't update the shared instance", format!("{error:#}"));
                        cx.notify();
                    });
                });
            }
        })
        .detach();
        cx.notify();
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
            .filter(|r| r.has_update && !r.locked)
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

    /// Dev hook: open the ⋯ menu.
    #[cfg_attr(not(feature = "snapshot"), allow(dead_code))]
    pub(crate) fn open_more(&mut self, cx: &mut Context<Self>) {
        self.more_open = true;
        cx.notify();
    }

    /// Dev hooks for snapshots: `dev-switch-pack`, `dev-switch-first`,
    /// `dev-pack-files`, `dev-search=<text>`.
    #[cfg_attr(not(feature = "snapshot"), allow(dead_code))]
    pub(crate) fn dev(&mut self, word: &str, window: &mut Window, cx: &mut Context<Self>) {
        match word {
            "dev-switch-pack" => cx.emit(InstancePageEvent::PickVersion(self.id.clone(), None)),
            "dev-pack-files" => self.pack_open = true,
            "dev-open-log" => {
                let first = self.log_files.as_ref().and_then(|f| {
                    f.iter()
                        .find(|(k, _)| *k == theseus::logs::LogType::InfoLog)
                        .cloned()
                });
                self.open_log(first, cx);
            }
            "dev-errors" => self.log_level = Some(LogLevel::Error),
            "dev-select-3" => {
                if let Some(Ok(rows)) = &self.content {
                    let picked: Vec<String> = rows.iter().take(3).map(|r| r.path.clone()).collect();
                    self.selected.extend(picked);
                }
            }
            "dev-bulk-off" => self.bulk(Bulk::Disable, cx),
            "dev-bulk-on" => self.bulk(Bulk::Enable, cx),
            "dev-freeze-update" => {
                // Freeze the first row that has an update waiting.
                let path = self
                    .content
                    .as_ref()
                    .and_then(|c| c.as_ref().ok())
                    .and_then(|rows| rows.iter().find(|r| r.has_update && !r.from_pack))
                    .map(|r| r.path.clone());
                if let Some(path) = path {
                    self.set_locked(path, true, cx);
                }
            }
            "dev-switch-first" => {
                let first = self
                    .content
                    .as_ref()
                    .and_then(|c| c.as_ref().ok())
                    .and_then(|rows| {
                        rows.iter()
                            .find(|r| r.project_id.is_some() && !r.from_pack)
                            .cloned()
                    });
                if let Some(row) = first {
                    cx.emit(InstancePageEvent::PickVersion(
                        self.id.clone(),
                        Some(crate::ui::version_picker::PickFile {
                            path: row.path.clone(),
                            project_id: row.project_id.clone().unwrap_or_default(),
                            title: row.title.to_string(),
                            icon_url: row.icon_url.clone(),
                            current: row.version_id.clone(),
                            kind: row.kind,
                        }),
                    ));
                }
            }
            w if w.starts_with("dev-search=") => {
                let text = w.trim_start_matches("dev-search=").to_string();
                self.query.update(cx, |q, cx| q.set_value(text, window, cx));
            }
            _ => {}
        }
        cx.notify();
    }

    /// Dev hook: open a file in the Files tab.
    #[cfg_attr(not(feature = "snapshot"), allow(dead_code))]
    pub(crate) fn open_file(&mut self, path: String, cx: &mut Context<Self>) {
        self.set_tab(Tab::Files, cx);
        if let Some(files) = &self.files {
            files.update(cx, |f, cx| f.open_path(path, cx));
        }
    }

    pub fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.tab = tab;
        match tab {
            Tab::Worlds => self.load_worlds(cx),
            Tab::Logs => self.load_log_files(cx),
            Tab::Health => self.load_health(cx),
            Tab::Screenshots => self.load_shots(cx),
            Tab::Snapshots => self.load_snapshots(cx),
            Tab::Files => match &self.files {
                Some(files) => files.update(cx, |f, cx| f.load(cx)),
                None => {
                    let id = self.id.clone();
                    self.files =
                        Some(cx.new(|cx| crate::ui::pages::files_tab::FilesBrowser::new(id, cx)));
                }
            },
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
            self.load_pack_info(cx);
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

    /// The modpack this instance follows, and whether a newer one is out.
    fn load_pack_info(&mut self, cx: &mut Context<Self>) {
        let id = self.id.clone();
        let fetch = Backend::global(cx)
            .call(async move { theseus::instance::get_linked_modpack_info(&id, None).await });
        cx.spawn(async move |this, cx| {
            let result = fetch.await;
            let _ = this.update(cx, |this, cx| {
                this.pack = match result {
                    Ok(Some(info)) => Some(PackInfo {
                        title: info.project.title.clone().into(),
                        icon_url: info.project.icon_url.clone(),
                        version: info
                            .version
                            .as_ref()
                            .map(|v| v.version_number.clone().into()),
                        update: info
                            .has_update
                            .then(|| {
                                info.update_version
                                    .as_ref()
                                    .map(|v| v.version_number.clone().into())
                            })
                            .flatten(),
                        update_id: info
                            .has_update
                            .then(|| info.update_version_id.clone())
                            .flatten(),
                        project_id: info.project.id.clone(),
                    }),
                    _ => None,
                };
                cx.notify();
            });
        })
        .detach();
    }

    /// Move the modpack to its newest version (a snapshot comes first).
    fn update_pack(&mut self, cx: &mut Context<Self>) {
        let Some(pack) = &self.pack else {
            return;
        };
        let Some(version_id) = pack.update_id.clone() else {
            return;
        };
        let label = format!(
            "Before updating {} to {}",
            pack.title,
            pack.update.clone().unwrap_or_default()
        );
        if let Some(pack) = &mut self.pack {
            pack.update = None;
            pack.update_id = None;
        }
        let id = self.id.clone();
        let snapshots = crate::ui::pages::instance_tabs::snapshots_dir(&id);
        let call = Backend::global(cx).call(async move {
            if let Some(snaps) = snapshots
                && let Ok(dir) = theseus::instance::get_full_path(&id).await
            {
                let _ = tokio::task::spawn_blocking(move || {
                    let result = calcite_core::snapshots::take(&dir, &snaps, &label, true);
                    calcite_core::snapshots::prune_automatic(&snaps, 10);
                    result
                })
                .await;
            }
            theseus::instance::update_managed_modrinth_version(&id, &version_id).await
        });
        let state = self.state.clone();
        cx.spawn(async move |_, cx| {
            let result = call.await;
            state.update(cx, |state, cx| {
                match result {
                    Ok(job) => state.track_job(job, cx),
                    Err(error) => {
                        state.notify_error("Couldn't update the modpack", format!("{error:#}"))
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// The card at the top of Content for modpack instances.
    fn pack_card(&self, cx: &mut Context<Self>) -> Option<gpui_kit::AnyElement> {
        let pack = self.pack.clone()?;
        let p = cx.palette().clone();
        let source = pack
            .icon_url
            .as_deref()
            .and_then(|url| Images::get(url, 80, cx));
        let files = self.pack_rows.len();
        let mut detail = vec!["Modpack".to_string()];
        if let Some(v) = &pack.version {
            detail.push(v.to_string());
        }
        if files > 0 {
            detail.push(format!("{files} files from the pack"));
        }
        let page = cx.entity().downgrade();
        let project = pack.project_id.clone();
        let more = KitButton::new("pack-more")
            .ghost()
            .small()
            .child(icon("ellipsis", 14.0, p.text_secondary))
            .dropdown_menu(move |menu, _, _| {
                let (a, b, c) = (page.clone(), page.clone(), page.clone());
                let project = project.clone();
                menu.item(
                    PopupMenuItem::new("Open project page").on_click(move |_, _, cx| {
                        let _ = a.update(cx, |_, cx| {
                            cx.emit(InstancePageEvent::OpenProject(project.clone()))
                        });
                    }),
                )
                .separator()
                .item(
                    PopupMenuItem::new("Re-install modpack…").on_click(move |_, _, cx| {
                        let _ = b.update(cx, |this, cx| {
                            this.pack_confirm = Some(PackConfirm::Reinstall);
                            cx.notify();
                        });
                    }),
                )
                .item(
                    PopupMenuItem::new("Unlink modpack…").on_click(move |_, _, cx| {
                        let _ = c.update(cx, |this, cx| {
                            this.pack_confirm = Some(PackConfirm::Unlink);
                            cx.notify();
                        });
                    }),
                )
            });
        let confirm = self.pack_confirm.map(|which| {
            let (text, action, danger) = match which {
                PackConfirm::Reinstall => (
                    format!("Re-install {}? Its own files come back the way the pack ships them, and anything you added is removed. Calcite saves a snapshot first.", pack.title),
                    "Re-install",
                    true,
                ),
                PackConfirm::Unlink => (
                    format!("Unlink from {}? Everything stays installed and you can change the Minecraft version and loader, but this instance won't get the pack's updates anymore.", pack.title),
                    "Unlink",
                    false,
                ),
            };
            div()
                .flex()
                .items_center()
                .gap(space(3.0))
                .pt(space(3.0))
                .mt(space(3.0))
                .border_t_1()
                .border_color(p.border)
                .child(
                    div()
                        .flex_1()
                        .text_size(px(type_scale::SMALL))
                        .text_color(p.text_secondary)
                        .child(text),
                )
                .child(
                    Button::new("pack-confirm-no")
                        .label("Cancel")
                        .small()
                        .ghost()
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.pack_confirm = None;
                            cx.notify();
                        })),
                )
                .child({
                    let b = Button::new("pack-confirm-yes").label(action).small();
                    let b = if danger { b.danger() } else { b.primary() };
                    b.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.pack_confirm = None;
                        let id = this.id.clone();
                        match which {
                            PackConfirm::Reinstall => {
                                this.state.update(cx, |s, cx| s.reinstall_modpack(id, cx))
                            }
                            PackConfirm::Unlink => {
                                this.pack = None;
                                this.pack_rows.clear();
                                this.state.update(cx, |s, cx| s.unlink_modpack(id, cx))
                            }
                        }
                        cx.notify();
                    }))
                })
        });
        Some(
            div()
                .mx(space(8.0))
                .mt(space(3.0))
                .p(space(3.0))
                .rounded(px(radius::MD))
                .bg(p.surface)
                .border_1()
                .border_color(p.border)
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space(3.0))
                        .child(thumb(source, &pack.title, 40.0, radius::SM, cx))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w_0()
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(type_scale::BODY_LG))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(p.text)
                                        .child(pack.title.clone()),
                                )
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(type_scale::SMALL))
                                        .text_color(p.text_muted)
                                        .child(detail.join(" · ")),
                                ),
                        )
                        .when_some(pack.update.clone(), |d, v| {
                            d.child(
                                Button::new("pack-update")
                                    .icon("arrow-down-to-line")
                                    .label(format!("Update to {v}"))
                                    .small()
                                    .primary()
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.update_pack(cx)
                                    })),
                            )
                        })
                        .child(
                            Button::new("pack-switch")
                                .icon("arrow-left-right")
                                .label("Switch version")
                                .small()
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    cx.emit(InstancePageEvent::PickVersion(this.id.clone(), None))
                                })),
                        )
                        .child(more),
                )
                .children(confirm)
                .into_any_element(),
        )
    }

    /// Something changed this instance's files (a version switch, say).
    pub(crate) fn content_changed(&mut self, cx: &mut Context<Self>) {
        self.load_content(cx);
        self.check_updates(cx);
    }

    /// Do one thing to every selected row: turn on (Some(true)), turn off
    /// (Some(false)), update, or remove. Pack files are never removed.
    fn bulk(&mut self, action: Bulk, cx: &mut Context<Self>) {
        let Some(Ok(rows)) = &self.content else {
            return;
        };
        let mut all: Vec<Row> = rows.clone();
        all.extend(self.pack_rows.iter().cloned());
        let picked: Vec<Row> = all
            .into_iter()
            .filter(|r| self.selected.contains(&r.path))
            .collect();
        self.selected.clear();
        if picked.is_empty() {
            cx.notify();
            return;
        }
        if action == Bulk::Update {
            let updates: Vec<theseus::install::ContentUpdateSelection> = picked
                .iter()
                .filter(|r| r.has_update && !r.locked)
                .filter_map(|r| {
                    Some(theseus::install::ContentUpdateSelection {
                        project_path: r.path.clone(),
                        version_id: r.update_version.clone()?,
                    })
                })
                .collect();
            if updates.is_empty() {
                cx.notify();
                return;
            }
            self.busy_paths
                .extend(updates.iter().map(|u| u.project_path.clone()));
            let id = self.id.clone();
            let state = self.state.clone();
            let call = Backend::global(cx)
                .call(async move { theseus::install::bulk_update_content(id, updates).await });
            cx.spawn(async move |_, cx| {
                let result = call.await;
                state.update(cx, |state, cx| {
                    match result {
                        Ok(job) => state.track_job(job, cx),
                        Err(error) => {
                            state.notify_error("Couldn't start the update", format!("{error:#}"))
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
            cx.notify();
            return;
        }
        let targets: Vec<(String, bool)> = picked
            .iter()
            .filter(|r| match action {
                Bulk::Enable => !r.enabled,
                Bulk::Disable => r.enabled,
                Bulk::Remove => !r.from_pack,
                Bulk::Update => false,
            })
            .map(|r| (r.path.clone(), r.enabled))
            .collect();
        self.busy_paths
            .extend(targets.iter().map(|(p, _)| p.clone()));
        cx.notify();
        let id = self.id.clone();
        let call = Backend::global(cx).call(async move {
            for (path, _) in targets {
                match action {
                    Bulk::Remove => theseus::instance::remove_project(&id, &path).await?,
                    _ => {
                        theseus::instance::toggle_disable_project(
                            &id,
                            &path,
                            Some(action == Bulk::Enable),
                        )
                        .await?;
                    }
                }
            }
            Ok(())
        });
        self.after(call, "Couldn't change all of them", cx);
    }

    fn selection_bar(&self, visible: &[Row], cx: &mut Context<Self>) -> gpui_kit::Div {
        let p = cx.palette().clone();
        let count = self.selected.len();
        let all_shown = visible.iter().all(|r| self.selected.contains(&r.path));
        let paths: Vec<String> = visible.iter().map(|r| r.path.clone()).collect();
        let action = |key: &'static str,
                      glyph: &'static str,
                      label: &'static str,
                      bulk: Bulk,
                      cx: &mut Context<Self>| {
            Button::new(key)
                .icon(glyph)
                .label(label)
                .small()
                .ghost()
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.bulk(bulk, cx)))
        };
        div()
            .flex()
            .items_center()
            .gap(space(2.0))
            .mx(space(8.0))
            .my(space(3.0))
            .px(space(3.0))
            .h(px(40.0))
            .rounded(px(radius::MD))
            .bg(p.accent_soft)
            .child(
                div()
                    .id("select-all")
                    .flex()
                    .items_center()
                    .gap(space(2.0))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        if all_shown {
                            this.selected.clear();
                        } else {
                            this.selected.extend(paths.iter().cloned());
                        }
                        cx.notify();
                    }))
                    .child(crate::ui::pages::library::checkbox(all_shown, cx))
                    .child(
                        div()
                            .text_size(px(type_scale::BODY))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(p.text)
                            .child(format!("{count} selected")),
                    ),
            )
            .child(div().flex_1())
            .child(action(
                "bulk-enable",
                "circle-check",
                "Turn on",
                Bulk::Enable,
                cx,
            ))
            .child(action(
                "bulk-disable",
                "circle",
                "Turn off",
                Bulk::Disable,
                cx,
            ))
            .child(action(
                "bulk-update",
                "refresh-cw",
                "Update",
                Bulk::Update,
                cx,
            ))
            .child(action("bulk-remove", "trash", "Remove", Bulk::Remove, cx))
            .child(
                Button::new("bulk-clear")
                    .icon("x")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.selected.clear();
                        cx.notify();
                    })),
            )
    }

    fn set_locked(&mut self, path: String, locked: bool, cx: &mut Context<Self>) {
        let id = self.id.clone();
        self.busy_paths.push(path.clone());
        cx.notify();
        let call = Backend::global(cx)
            .call(async move { theseus::instance::set_project_locked(&id, &path, locked).await });
        self.after(call, "Couldn't change that", cx);
    }

    /// Show one of the instance's files in Finder / Explorer.
    fn reveal(&mut self, path: String, cx: &mut Context<Self>) {
        let id = self.id.clone();
        let fetch = Backend::global(cx)
            .call(async move { theseus::instance::get_mod_full_path(&id, &path).await });
        cx.spawn(async move |_, cx| {
            if let Ok(path) = fetch.await {
                cx.update(|cx| cx.reveal_path(&path));
            }
        })
        .detach();
    }

    /// The ⋮ / right-click menu for one row.
    fn row_menu(
        menu: PopupMenu,
        row: &Row,
        instance: &str,
        page: gpui_kit::WeakEntity<InstancePage>,
    ) -> PopupMenu {
        let mut menu = menu;
        if let Some(project) = row.project_id.clone()
            && !row.from_pack
        {
            let (page, file) = (
                page.clone(),
                crate::ui::version_picker::PickFile {
                    path: row.path.clone(),
                    project_id: project,
                    title: row.title.to_string(),
                    icon_url: row.icon_url.clone(),
                    current: row.version_id.clone(),
                    kind: row.kind,
                },
            );
            let id = instance.to_string();
            let locked = row.locked;
            menu = menu.item(
                PopupMenuItem::new("Switch version…")
                    .disabled(locked)
                    .on_click(move |_, _, cx| {
                        let _ = page.update(cx, |_, cx| {
                            cx.emit(InstancePageEvent::PickVersion(
                                id.clone(),
                                Some(file.clone()),
                            ))
                        });
                    }),
            );
        }
        if let Some(project) = row.project_id.clone() {
            let page = page.clone();
            menu = menu.item(
                PopupMenuItem::new("Open project page").on_click(move |_, _, cx| {
                    let _ = page.update(cx, |_, cx| {
                        cx.emit(InstancePageEvent::OpenProject(project.clone()))
                    });
                }),
            );
            let link = format!(
                "https://modrinth.com/project/{}",
                row.project_id.as_deref().unwrap_or_default()
            );
            menu = menu.item(PopupMenuItem::new("Copy link").on_click(move |_, _, cx| {
                cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(link.clone()));
            }));
        }
        {
            let (page, path) = (page.clone(), row.path.clone());
            menu = menu.item(
                PopupMenuItem::new(if cfg!(target_os = "macos") {
                    "Show in Finder"
                } else {
                    "Show in folder"
                })
                .on_click(move |_, _, cx| {
                    let _ = page.update(cx, |this, cx| this.reveal(path.clone(), cx));
                }),
            );
        }
        if !row.from_pack {
            let (freeze, path, locked) = (page.clone(), row.path.clone(), row.locked);
            menu = menu.separator().item(
                PopupMenuItem::new(if locked {
                    "Unfreeze version"
                } else {
                    "Freeze version"
                })
                .on_click(move |_, _, cx| {
                    let _ =
                        freeze.update(cx, |this, cx| this.set_locked(path.clone(), !locked, cx));
                }),
            );
            let (page, path) = (page.clone(), row.path.clone());
            menu = menu.item(PopupMenuItem::new("Remove").on_click(move |_, _, cx| {
                let _ = page.update(cx, |this, cx| this.remove(path.clone(), cx));
            }));
        }
        menu
    }

    /// The saved logs and crash reports, for the picker.
    fn load_log_files(&mut self, cx: &mut Context<Self>) {
        let id = self.id.clone();
        let fetch =
            Backend::global(cx).call(async move { theseus::logs::get_logs(&id, Some(true)).await });
        cx.spawn(async move |this, cx| {
            let files = fetch.await.unwrap_or_default();
            let _ = this.update(cx, |this, cx| {
                this.log_files = Some(
                    files
                        .into_iter()
                        .filter(|l| l.filename != "latest.log")
                        .map(|l| (l.log_type, l.filename))
                        .collect(),
                );
                cx.notify();
            });
        })
        .detach();
    }

    /// Show a saved log (or, with None, the live one).
    fn open_log(&mut self, file: Option<(theseus::logs::LogType, String)>, cx: &mut Context<Self>) {
        self.log_file = file.clone();
        self.old_log = None;
        cx.notify();
        let Some((kind, name)) = file else {
            return;
        };
        let id = self.id.clone();
        let fetch = Backend::global(cx)
            .call(async move { theseus::logs::get_output_by_filename(&id, kind, &name).await });
        cx.spawn(async move |this, cx| {
            let result = fetch.await;
            let _ = this.update(cx, |this, cx| {
                this.old_log = Some(
                    result
                        .map(|text| {
                            let text = serde_json::to_value(&text)
                                .ok()
                                .and_then(|v| v.as_str().map(str::to_string))
                                .unwrap_or_default();
                            // Stack traces and other untimed lines belong
                            // to the line above them.
                            let mut lines: Vec<LogLine> =
                                text.lines().map(parse_legacy_line).collect();
                            for ix in 1..lines.len() {
                                if lines[ix].time.is_empty() {
                                    lines[ix].level = lines[ix - 1].level.clone();
                                }
                            }
                            lines
                        })
                        .map_err(|e| format!("{e:#}")),
                );
                cx.notify();
            });
        })
        .detach();
    }

    /// The log being shown, as plain text.
    fn log_text(&self, cx: &App) -> String {
        let join = |lines: &mut dyn Iterator<Item = &LogLine>| {
            lines
                .map(|l| {
                    if l.time.is_empty() {
                        l.text.to_string()
                    } else {
                        format!("[{}] {}", l.time, l.text)
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        match (&self.log_file, &self.old_log) {
            (Some(_), Some(Ok(lines))) => join(&mut lines.iter()),
            (Some(_), _) => String::new(),
            (None, _) => self
                .state
                .read(cx)
                .logs
                .get(&self.id)
                .map(|b| join(&mut b.iter()))
                .unwrap_or_default(),
        }
    }

    /// Upload the log to mclo.gs and copy the link.
    fn share_log(&mut self, cx: &mut Context<Self>) {
        let text = self.log_text(cx);
        if text.trim().is_empty() || self.sharing_log {
            return;
        }
        self.sharing_log = true;
        cx.notify();
        let upload = Backend::global(cx).run(async move {
            let http = reqwest::Client::builder()
                .user_agent(concat!("Calcite/", env!("CARGO_PKG_VERSION")))
                .build()?;
            calcite_core::paste::mclogs(&http, &text).await
        });
        cx.spawn(async move |this, cx| {
            let result = upload.await.and_then(|r| r);
            let _ = this.update(cx, |this, cx| {
                this.sharing_log = false;
                this.state.update(cx, |state, cx| {
                    match result {
                        Ok(url) => {
                            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(url.clone()));
                            state.notice = Some(crate::state::Notice {
                                title: "Link copied".into(),
                                body: format!("The log is at {url}").into(),
                                is_error: false,
                            });
                        }
                        Err(error) => {
                            state.notify_error("Couldn't share the log", format!("{error:#}"))
                        }
                    }
                    cx.notify();
                });
                cx.notify();
            });
        })
        .detach();
    }

    fn open_logs_folder(&mut self, cx: &mut Context<Self>) {
        let id = self.id.clone();
        let crash = matches!(
            self.log_file,
            Some((theseus::logs::LogType::CrashReport, _))
        );
        let fetch =
            Backend::global(cx).call(async move { theseus::instance::get_full_path(&id).await });
        cx.spawn(async move |_, cx| {
            if let Ok(path) = fetch.await {
                let dir = path.join(if crash { "crash-reports" } else { "logs" });
                cx.update(|cx| cx.open_with_system(if dir.exists() { &dir } else { &path }));
            }
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
        // Share your own instances (or ones you already share), once signed in to Modrinth.
        let can_share = state.modrinth.is_some() && inst.shared.as_ref().is_none_or(|s| s.owner);
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
                            .when(can_share, |d| {
                                d.child(
                                    Button::new("instance-share")
                                        .icon("share-2")
                                        .ghost()
                                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                            cx.emit(InstancePageEvent::Share(this.id.clone()))
                                        })),
                                )
                            })
                            .child(self.more_menu(cx))
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
            .when_some(self.shared_update.filter(|_| !busy), |d, n| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space(3.0))
                        .p(space(3.0))
                        .rounded(px(radius::MD))
                        .bg(p.accent_soft)
                        .child(icon("refresh-cw", 16.0, p.accent))
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(type_scale::SMALL))
                                .text_color(p.text)
                                .child(format!(
                                    "The owner changed this instance ({n} change{}).",
                                    if n == 1 { "" } else { "s" }
                                )),
                        )
                        .child(
                            Button::new("shared-update")
                                .label("Update")
                                .small()
                                .primary()
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.apply_shared_update(cx)
                                })),
                        ),
                )
            })
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
            (Tab::Files, "Files", None),
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
            .filter(|r| r.has_update && r.update_version.is_some() && !r.locked)
            .count();
        let mut toolbar = div()
            .flex()
            .items_center()
            .gap(space(2.0))
            .px(space(8.0))
            .py(space(3.0))
            .child(filters)
            .child(div().flex_1())
            .child(
                div()
                    .w(px(200.0))
                    .child(Input::new(&self.query).small().cleanable(true)),
            )
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

        let query = self.query.read(cx).value().trim().to_lowercase();
        let pack_total = rows
            .iter()
            .filter(|r| r.from_pack && self.filter.matches(r.kind))
            .count();
        let show_pack = self.pack_open || !query.is_empty();
        let visible: Vec<Row> = rows
            .into_iter()
            .filter(|r| self.filter.matches(r.kind))
            .filter(|r| {
                query.is_empty()
                    || r.title.to_lowercase().contains(&query)
                    || r.file_name.to_lowercase().contains(&query)
                    || r.author
                        .as_ref()
                        .is_some_and(|a| a.to_lowercase().contains(&query))
            })
            .filter(|r| show_pack || !r.from_pack)
            .collect();
        let pack_card = self.pack_card(cx);
        if !self.selected.is_empty() {
            toolbar = self.selection_bar(&visible, cx);
        }
        if visible.is_empty() && !self.pack_loading && pack_total == 0 {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .children(pack_card)
                .child(toolbar)
                .child(empty_state(
                    "package",
                    if query.is_empty() { "Nothing installed yet" } else { "Nothing matches" },
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
        let has_pack = pack_total > 0;
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
        let pack_toggle = |open: bool, count: usize, cx: &mut Context<Self>| {
            div()
                .id("pack-files-toggle")
                .flex()
                .items_center()
                .gap(space(1.5))
                .pt(space(4.0))
                .pb(space(1.5))
                .px(space(3.0))
                .cursor_pointer()
                .text_size(px(type_scale::SMALL))
                .font_weight(FontWeight::MEDIUM)
                .text_color(p.text_muted)
                .hover(|s| s.text_color(p.text))
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.pack_open = !this.pack_open;
                    cx.notify();
                }))
                .child(icon(
                    if open {
                        "chevron-down"
                    } else {
                        "chevron-right"
                    },
                    12.0,
                    p.text_muted,
                ))
                .child(format!("From the modpack · {count}"))
        };
        if has_pack && has_added {
            list = list.child(heading("Added by you".into()));
        }
        let page = cx.entity().downgrade();
        for (ix, row) in visible.iter().enumerate() {
            if row.from_pack && (ix == 0 || !visible[ix - 1].from_pack) {
                list = list.child(pack_toggle(true, pack_total, cx));
            }
            let source = row
                .icon_url
                .as_deref()
                .and_then(|url| Images::get(url, 64, cx));
            let busy = self.busy_paths.contains(&row.path);
            let (toggle_path, remove_path, enabled) =
                (row.path.clone(), row.path.clone(), row.enabled);
            let picked = self.selected.contains(&row.path);
            let pick_path = row.path.clone();
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
                    .when(picked, |d| d.bg(p.hover))
                    .hover(|s| s.bg(p.surface))
                    .when(busy, |d| d.opacity(0.5))
                    .child(
                        div()
                            .id(SharedString::from(format!("pick-{ix}")))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                if !this.selected.remove(&pick_path) {
                                    this.selected.insert(pick_path.clone());
                                }
                                cx.notify();
                            }))
                            .child(crate::ui::pages::library::checkbox(picked, cx)),
                    )
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
                            .when(row.locked, |d| {
                                d.child(
                                    div()
                                        .id(SharedString::from(format!("locked-{ix}")))
                                        .child(icon("lock", 12.0, p.text_muted)),
                                )
                            })
                            .when(
                                row.has_update && row.update_version.is_some() && !row.locked,
                                |d| {
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
                                },
                            )
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
                    .when(!row.from_pack, |d| {
                        d.child(
                            Button::new(SharedString::from(format!("remove-{ix}")))
                                .icon("trash")
                                .ghost()
                                .small()
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.remove(remove_path.clone(), cx)
                                })),
                        )
                    })
                    .when(row.from_pack, |d| d.child(div().w(px(28.0))))
                    .child({
                        let (row, id, page) = (row.clone(), self.id.clone(), page.clone());
                        KitButton::new(SharedString::from(format!("row-more-{ix}")))
                            .ghost()
                            .small()
                            .child(icon("ellipsis-vertical", 14.0, p.text_muted))
                            .dropdown_menu(move |menu, _, _| {
                                InstancePage::row_menu(menu, &row, &id, page.clone())
                            })
                    })
                    .context_menu({
                        let (row, id, page) = (row.clone(), self.id.clone(), page.clone());
                        move |menu, _, _| InstancePage::row_menu(menu, &row, &id, page.clone())
                    }),
            );
        }
        if has_pack && !show_pack {
            list = list.child(pack_toggle(false, pack_total, cx));
        }

        if self.pack_loading && pack_total == 0 {
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
            .children(pack_card)
            .child(toolbar)
            .child(list)
            .into_any_element()
    }

    fn logs_tab(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let p = cx.palette().clone();
        let live = self.log_file.is_none();
        let all: Vec<LogLine> = match (&self.log_file, &self.old_log) {
            (None, _) => self
                .state
                .read(cx)
                .logs
                .get(&self.id)
                .map(|b| b.iter().cloned().collect())
                .unwrap_or_default(),
            (Some(_), Some(Ok(lines))) => lines.clone(),
            (Some(_), _) => Vec::new(),
        };
        let running = self.state.read(cx).running.contains_key(&self.id);
        let query = self.log_query.read(cx).value().trim().to_lowercase();
        let count = |level: &LogLevel| all.iter().filter(|l| l.level == *level).count();
        let (errors, warnings) = (count(&LogLevel::Error), count(&LogLevel::Warn));
        let lines: Vec<LogLine> = all
            .iter()
            .filter(|l| self.log_level.as_ref().is_none_or(|lv| l.level == *lv))
            .filter(|l| query.is_empty() || l.text.to_lowercase().contains(&query))
            .cloned()
            .collect();

        // Which log: live, or one of the saved files.
        let source_label: SharedString = match &self.log_file {
            None => if running { "Live" } else { "This session" }.into(),
            Some((kind, name)) => log_label(*kind, name).into(),
        };
        let files = self.log_files.clone().unwrap_or_default();
        let page = cx.entity().downgrade();
        let picker = KitButton::new("log-source")
            .small()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space(1.5))
                    .child(icon(
                        if live { "square-terminal" } else { "file-text" },
                        13.0,
                        p.text_secondary,
                    ))
                    .child(source_label)
                    .child(icon("chevron-down", 12.0, p.text_muted)),
            )
            .dropdown_menu(move |menu, _, _| {
                let open = page.clone();
                let mut menu = menu.item(PopupMenuItem::new("Live log").checked(false).on_click(
                    move |_, _, cx| {
                        let _ = open.update(cx, |this, cx| this.open_log(None, cx));
                    },
                ));
                if !files.is_empty() {
                    menu = menu.separator();
                }
                for (kind, name) in files.iter().take(40) {
                    let open = page.clone();
                    let file = (*kind, name.clone());
                    menu = menu.item(PopupMenuItem::new(log_label(*kind, name)).on_click(
                        move |_, _, cx| {
                            let file = file.clone();
                            let _ = open.update(cx, |this, cx| this.open_log(Some(file), cx));
                        },
                    ));
                }
                menu
            });

        let level_chip =
            |key: &'static str, label: String, level: Option<LogLevel>, cx: &mut Context<Self>| {
                let active = self.log_level == level;
                div()
                    .id(key)
                    .h(px(26.0))
                    .px(space(2.5))
                    .flex()
                    .items_center()
                    .rounded(px(radius::SM))
                    .text_size(px(type_scale::SMALL))
                    .cursor_pointer()
                    .when(active, |d| d.bg(p.raised).text_color(p.text))
                    .when(!active, |d| {
                        d.text_color(p.text_secondary)
                            .hover(|s| s.text_color(p.text))
                    })
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.log_level = level.clone();
                        cx.notify();
                    }))
                    .child(label)
            };

        let toolbar = div()
            .flex()
            .items_center()
            .gap(space(2.0))
            .px(space(8.0))
            .pt(space(4.0))
            .child(picker)
            .child(level_chip(
                "log-all",
                format!("All {}", all.len()),
                None,
                cx,
            ))
            .child(level_chip(
                "log-errors",
                format!("Errors {errors}"),
                Some(LogLevel::Error),
                cx,
            ))
            .child(level_chip(
                "log-warnings",
                format!("Warnings {warnings}"),
                Some(LogLevel::Warn),
                cx,
            ))
            .child(level_chip(
                "log-info",
                "Info".into(),
                Some(LogLevel::Info),
                cx,
            ))
            .child(div().flex_1())
            .child(
                div()
                    .w(px(200.0))
                    .child(Input::new(&self.log_query).small().cleanable(true)),
            )
            .when(live, |d| {
                d.child(
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
            })
            .child({
                let page = cx.entity().downgrade();
                KitButton::new("log-more")
                    .ghost()
                    .small()
                    .child(icon("ellipsis", 14.0, p.text_secondary))
                    .dropdown_menu(move |menu, _, _| {
                        let (a, b, c, d) = (page.clone(), page.clone(), page.clone(), page.clone());
                        let mut menu = menu
                            .item(PopupMenuItem::new("Copy").on_click(move |_, _, cx| {
                                let _ = a.update(cx, |this, cx| {
                                    let text = this.log_text(cx);
                                    cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(
                                        text,
                                    ));
                                });
                            }))
                            .item(PopupMenuItem::new("Share as a link (mclo.gs)").on_click(
                                move |_, _, cx| {
                                    let _ = b.update(cx, |this, cx| this.share_log(cx));
                                },
                            ))
                            .item(PopupMenuItem::new("Open logs folder").on_click(
                                move |_, _, cx| {
                                    let _ = c.update(cx, |this, cx| this.open_logs_folder(cx));
                                },
                            ));
                        if live {
                            menu = menu.separator().item(PopupMenuItem::new("Clear").on_click(
                                move |_, _, cx| {
                                    let _ = d.update(cx, |this, cx| {
                                        let id = this.id.clone();
                                        this.state.update(cx, |s, cx| {
                                            s.logs.remove(&id);
                                            cx.notify();
                                        });
                                    });
                                },
                            ));
                        }
                        menu
                    })
            });

        let empty: Option<(&'static str, String)> = match (&self.log_file, &self.old_log) {
            (Some(_), None) => Some(("Opening the log…", String::new())),
            (Some(_), Some(Err(error))) => Some(("Couldn't open that log", error.clone())),
            _ if all.is_empty() => Some((
                "No output yet",
                "Press Play and the game's log appears here as it runs. Older logs and crash reports are in the menu on the left.".into(),
            )),
            _ if lines.is_empty() => Some(("No lines match", "Try another search or level.".into())),
            _ => None,
        };
        if let Some((title, body)) = empty {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .child(toolbar)
                .child(empty_state("square-terminal", title, body, None, cx))
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
                        .w_full()
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

/// "Crash · Nov 15, 2026 14:02" / "Log · 2026-11-15 #2" style labels.
fn log_label(kind: theseus::logs::LogType, name: &str) -> String {
    let stem = name
        .trim_end_matches(".gz")
        .trim_end_matches(".log")
        .trim_end_matches(".txt");
    match kind {
        theseus::logs::LogType::CrashReport => {
            let when = stem
                .trim_start_matches("crash-")
                .trim_end_matches("-client")
                .trim_end_matches("-server")
                .replacen('_', " ", 1)
                .replace('.', ":");
            format!("Crash · {when}")
        }
        theseus::logs::LogType::InfoLog => match stem.rsplit_once('-') {
            Some((date, n)) if n.chars().all(|c| c.is_ascii_digit()) => {
                format!("Log · {date} #{n}")
            }
            _ => format!("Log · {stem}"),
        },
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
            Tab::Files => match &self.files {
                Some(files) => files.clone().into_any_element(),
                None => div().into_any_element(),
            },
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

pub(crate) fn toggle(on: bool, cx: &App) -> impl IntoElement {
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
