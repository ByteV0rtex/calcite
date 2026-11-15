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
    skeleton, thumb,
};
use crate::ui::shell::{instance_icon, running_chip};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Content,
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
    kind: ProjectType,
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
            kind: item.project_type,
            path: item.file_path,
            file_name: file_name.into(),
        }
    }
}

pub struct InstancePage {
    id: String,
    tab: Tab,
    filter: Filter,
    content: Option<Result<Vec<Row>, String>>,
    busy_paths: Vec<String>,
    log_scroll: UniformListScrollHandle,
    follow_logs: bool,
    state: Entity<AppState>,
    _subscriptions: Vec<Subscription>,
}

impl InstancePage {
    pub fn new(id: String, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let subscriptions = vec![cx.observe(&state, |this, _, cx| {
            if this.follow_logs && this.tab == Tab::Logs {
                this.log_scroll
                    .scroll_to_item(usize::MAX, gpui_kit::ScrollStrategy::Bottom);
            }
            cx.notify()
        })];
        let mut page = InstancePage {
            id,
            tab: Tab::Content,
            filter: Filter::All,
            content: None,
            busy_paths: Vec::new(),
            log_scroll: UniformListScrollHandle::new(),
            follow_logs: true,
            state,
            _subscriptions: subscriptions,
        };
        page.load_content(cx);
        page.load_log_buffer(cx);
        page
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.tab = tab;
        cx.notify();
    }

    fn load_content(&mut self, cx: &mut Context<Self>) {
        let id = self.id.clone();
        let fetch = Backend::global(cx).call(async move {
            // Pick up files added outside Calcite (dropped into the mods folder).
            if let Err(error) = theseus::instance::sync_content_files(&id).await {
                tracing::warn!("content sync for {id} failed: {error}");
            }
            theseus::instance::get_content_items(&id, None).await
        });
        cx.spawn(async move |this, cx| {
            let result = fetch.await;
            let _ = this.update(cx, |this, cx| {
                this.content = Some(
                    result
                        .map(|items| {
                            let mut rows: Vec<Row> =
                                items.into_iter().map(Row::from_item).collect();
                            rows.sort_by_key(|r| r.title.to_lowercase());
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
                                    .when_some(inst.last_played, |d, t| {
                                        d.child(
                                            div()
                                                .text_size(px(type_scale::SMALL))
                                                .text_color(p.text_muted)
                                                .child(format!("Played {}", relative_time(t))),
                                        )
                                    })
                                    .when_some(running, |d, secs| d.child(running_chip(secs, cx))),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(space(2.0))
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
            .map(Vec::len);
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
            Some(Ok(rows)) => rows.clone(),
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

        let toolbar = div()
            .flex()
            .items_center()
            .gap(space(2.0))
            .px(space(8.0))
            .py(space(3.0))
            .child(filters)
            .child(div().flex_1())
            .child(
                Button::new("add-files")
                    .icon("plus")
                    .label("Add files")
                    .small()
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.add_files(cx))),
            );

        let visible: Vec<Row> = rows
            .into_iter()
            .filter(|r| self.filter.matches(r.kind))
            .collect();
        if visible.is_empty() {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .child(toolbar)
                .child(empty_state(
                    "package",
                    "Nothing installed yet",
                    "Add mods, resource packs or shaders as files from your computer. Installing from Modrinth into an instance comes with the next update.",
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
        for (ix, row) in visible.iter().enumerate() {
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
                                            .truncate()
                                            .text_size(px(type_scale::BODY))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(p.text)
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
                            .when(row.has_update, |d| {
                                d.child(div().size(px(6.0)).rounded_full().bg(p.accent))
                            })
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(type_scale::SMALL))
                                    .text_color(p.text_secondary)
                                    .child(row.version.clone()),
                            ),
                    )
                    .child(
                        Button::new(SharedString::from(format!("remove-{ix}")))
                            .icon("trash")
                            .ghost()
                            .small()
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.remove(remove_path.clone(), cx)
                            })),
                    ),
            );
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
