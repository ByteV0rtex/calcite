//! Settings › Storage: where the disk space goes, and safe ways to get it back.

use std::collections::HashMap;

use calcite_core::storage::{self, Breakdown, OldFiles};
use calcite_core::transfer::human_bytes;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Entity, EventEmitter, FontWeight, Hsla, SharedString,
    Window, div, px, relative,
};
use theseus::settings::StoreUsage;

use crate::backend::{self, Backend};
use crate::state::{AppState, InstanceInfo, Notice};
use crate::theme::{PaletteExt, radius, space, type_scale};
use crate::ui::components::{Button, icon, panel, section_heading, skeleton, thumb};
use crate::ui::shell::instance_icon;

pub enum StorageEvent {
    OpenInstance(String),
}

#[derive(Clone, Copy, PartialEq)]
enum Busy {
    Scanning,
    CleaningStore,
    DeletingLogs,
}

pub struct StoragePage {
    state: Entity<AppState>,
    breakdown: Option<Breakdown>,
    old_logs: Option<OldFiles>,
    store: Option<Result<StoreUsage, String>>,
    /// Instance folder name → instance id and name.
    folders: HashMap<String, (String, SharedString)>,
    busy: Option<Busy>,
    show_all: bool,
}

impl EventEmitter<StorageEvent> for StoragePage {}

/// Instances shown before "Show all".
const TOP: usize = 8;

impl StoragePage {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut page = StoragePage {
            state: AppState::global(cx),
            breakdown: None,
            old_logs: None,
            store: None,
            folders: HashMap::new(),
            busy: None,
            show_all: false,
        };
        page.scan(cx);
        page
    }

    fn scan(&mut self, cx: &mut Context<Self>) {
        let Some(data_dir) = backend::data_dir() else {
            return;
        };
        self.busy = Some(Busy::Scanning);
        let walk = cx.background_spawn(async move {
            let breakdown = storage::breakdown(&data_dir);
            let old = storage::old_logs(&data_dir.join("profiles"), std::time::SystemTime::now());
            (breakdown, old)
        });
        let store = Backend::global(cx).call(async move {
            let usage = theseus::settings::store_usage().await?;
            let mut folders = HashMap::new();
            for meta in theseus::instance::list().await? {
                let (id, name) = (meta.instance.id, meta.instance.name);
                if let Ok(path) = theseus::instance::get_full_path(&id).await
                    && let Some(folder) = path.file_name()
                {
                    folders.insert(
                        folder.to_string_lossy().into_owned(),
                        (id, SharedString::from(name)),
                    );
                }
            }
            Ok::<_, theseus::Error>((usage, folders))
        });
        cx.spawn(async move |this, cx| {
            let (breakdown, old) = walk.await;
            let store = store.await;
            let _ = this.update(cx, |this, cx| {
                this.breakdown = Some(breakdown);
                this.old_logs = Some(old);
                match store {
                    Ok((usage, folders)) => {
                        this.store = Some(Ok(usage));
                        this.folders = folders;
                    }
                    Err(error) => this.store = Some(Err(format!("{error:#}"))),
                }
                this.busy = None;
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn tell(
        &self,
        title: impl Into<SharedString>,
        body: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        let (title, body) = (title.into(), body.into());
        self.state.update(cx, |s, cx| {
            s.notice = Some(Notice {
                title,
                body,
                is_error: false,
            });
            cx.notify();
        });
    }

    pub(crate) fn clean_store(&mut self, cx: &mut Context<Self>) {
        self.busy = Some(Busy::CleaningStore);
        let task = Backend::global(cx).call(async { theseus::settings::store_cleanup().await });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(freed) => this.tell(
                        format!("Freed {}", human_bytes(freed)),
                        "Unused downloads and game files are gone. Calcite fetches anything again if an instance needs it.",
                        cx,
                    ),
                    Err(error) => this.state.update(cx, |s, cx| {
                        s.notify_error("Couldn't clean up", format!("{error:#}"));
                        cx.notify();
                    }),
                }
                this.scan(cx);
            });
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn delete_logs(&mut self, cx: &mut Context<Self>) {
        let Some(old) = self.old_logs.clone() else {
            return;
        };
        self.busy = Some(Busy::DeletingLogs);
        let task = cx.background_spawn(async move { storage::remove(&old.files) });
        cx.spawn(async move |this, cx| {
            let freed = task.await;
            let _ = this.update(cx, |this, cx| {
                this.tell(
                    format!("Freed {}", human_bytes(freed)),
                    "Old logs and crash reports are deleted. Each instance keeps its latest log.",
                    cx,
                );
                this.scan(cx);
            });
        })
        .detach();
        cx.notify();
    }

    /// The instance in `folder`: its id, name and (once loaded) details.
    fn instance_for(
        &self,
        folder: &str,
        cx: &App,
    ) -> Option<(String, SharedString, Option<InstanceInfo>)> {
        let (id, name) = self.folders.get(folder)?;
        Some((
            id.clone(),
            name.clone(),
            self.state.read(cx).instance(id).cloned(),
        ))
    }

    fn overview(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette().clone();
        let Some(b) = &self.breakdown else {
            return panel(cx)
                .p(space(5.0))
                .flex()
                .flex_col()
                .gap(space(3.0))
                .child(skeleton(px(140.0), px(28.0), cx))
                .child(skeleton(relative(1.0), px(10.0), cx))
                .child(skeleton(px(360.0), px(14.0), cx))
                .into_any_element();
        };
        let total = b.total().max(1);
        let parts: [(&str, u64, Hsla); 5] = [
            ("Instances", b.instances_total(), p.accent),
            ("Game files and Java", b.game_files, p.accent.opacity(0.6)),
            ("Shared mod store", b.store, p.accent.opacity(0.38)),
            ("Snapshots", b.snapshots, p.accent.opacity(0.22)),
            ("Caches and the rest", b.other, p.border_strong),
        ];
        let bar =
            div()
                .flex()
                .w_full()
                .h(px(10.0))
                .rounded(px(radius::SM))
                .overflow_hidden()
                .bg(p.raised)
                .children(parts.iter().filter(|(_, bytes, _)| *bytes > 0).map(
                    |(_, bytes, color)| {
                        div()
                            .h_full()
                            .w(relative((*bytes as f32 / total as f32).max(0.004)))
                            .bg(*color)
                    },
                ));
        let legend = div()
            .flex()
            .flex_wrap()
            .gap_x(space(5.0))
            .gap_y(space(2.0))
            .children(parts.iter().map(|(label, bytes, color)| {
                div()
                    .flex()
                    .items_center()
                    .gap(space(1.5))
                    .text_size(px(type_scale::SMALL))
                    .child(div().size(px(8.0)).rounded(px(2.0)).bg(*color))
                    .child(div().text_color(p.text_secondary).child(*label))
                    .child(div().text_color(p.text_muted).child(human_bytes(*bytes)))
            }));

        panel(cx)
            .p(space(5.0))
            .flex()
            .flex_col()
            .gap(space(4.0))
            .child(
                div()
                    .flex()
                    .items_end()
                    .gap(space(2.0))
                    .child(
                        div()
                            .text_size(px(type_scale::DISPLAY))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.text)
                            .child(human_bytes(b.total())),
                    )
                    .child(
                        div()
                            .pb(space(1.5))
                            .flex_1()
                            .min_w_0()
                            .text_size(px(type_scale::SMALL))
                            .text_color(p.text_muted)
                            .child("in Calcite's folder"),
                    )
                    .child(
                        Button::new("storage-open-folder")
                            .icon("folder-open")
                            .label("Open folder")
                            .small()
                            .ghost()
                            .on_click(|_: &ClickEvent, _, cx| {
                                if let Some(dir) = backend::data_dir() {
                                    cx.open_with_system(&dir);
                                }
                            }),
                    ),
            )
            .child(bar)
            .child(legend)
            .into_any_element()
    }

    fn cleanup(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette().clone();
        let busy = self.busy;
        let unused = self
            .store
            .as_ref()
            .and_then(|s| s.as_ref().ok())
            .map(|u| u.unused_cache_bytes);
        let logs = self.old_logs.as_ref().map(|o| (o.files.len(), o.bytes));
        let running = !self.state.read(cx).running.is_empty();

        let row = |ix: usize,
                   icon_name: &'static str,
                   title: String,
                   body: String,
                   action: AnyElement| {
            div()
                .flex()
                .items_center()
                .gap(space(3.0))
                .px(space(4.0))
                .py(space(3.5))
                .when(ix > 0, |d| d.border_t_1().border_color(p.border))
                .child(icon(icon_name, 16.0, p.text_muted))
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
                .child(action)
        };

        let store_row = row(
            0,
            "package",
            match unused {
                Some(0) => "Unused downloads and game files · nothing to clean".into(),
                Some(b) => format!("Unused downloads and game files · {}", human_bytes(b)),
                None => "Unused downloads and game files".into(),
            },
            if running {
                "Mods no instance uses anymore, and game versions, libraries and Java that no instance needs. Close the game first; files in use can't be cleaned.".into()
            } else {
                "Mods no instance uses anymore, and game versions, libraries and Java that no instance needs. Calcite downloads them again if an instance ever does.".into()
            },
            Button::new("storage-clean-store")
                .label(if busy == Some(Busy::CleaningStore) {
                    "Cleaning…"
                } else {
                    "Clean up"
                })
                .small()
                .disabled(busy.is_some() || unused.is_none_or(|b| b == 0))
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.clean_store(cx)))
                .into_any_element(),
        );
        let logs_row = row(
            1,
            "file-text",
            match logs {
                Some((n, b)) if n > 0 => format!(
                    "Old logs and crash reports · {} in {n} file{}",
                    human_bytes(b),
                    if n == 1 { "" } else { "s" }
                ),
                Some(_) => "Old logs and crash reports · nothing to clean".into(),
                None => "Old logs and crash reports".into(),
            },
            "Logs older than two weeks and crash reports older than a month. Each instance keeps its latest log.".into(),
            Button::new("storage-delete-logs")
                .label(if busy == Some(Busy::DeletingLogs) { "Deleting…" } else { "Delete" })
                .small()
                .disabled(busy.is_some() || logs.is_none_or(|(n, _)| n == 0))
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.delete_logs(cx)))
                .into_any_element(),
        );
        panel(cx)
            .overflow_hidden()
            .child(store_row)
            .child(logs_row)
            .into_any_element()
    }

    fn instances(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette().clone();
        let Some(b) = &self.breakdown else {
            return panel(cx)
                .p(space(4.0))
                .flex()
                .flex_col()
                .gap(space(3.0))
                .children((0..4).map(|_| skeleton(relative(1.0), px(24.0), cx)))
                .into_any_element();
        };
        let biggest = b.instances.first().map(|(_, s)| *s).unwrap_or(1).max(1);
        let shown: Vec<(String, u64)> = if self.show_all {
            b.instances.clone()
        } else {
            b.instances.iter().take(TOP).cloned().collect()
        };
        let mut list = panel(cx).overflow_hidden();
        for (ix, (folder, bytes)) in shown.into_iter().enumerate() {
            let found = self.instance_for(&folder, cx);
            let name: SharedString = found
                .as_ref()
                .map(|(_, name, _)| name.clone())
                .unwrap_or_else(|| folder.clone().into());
            let source = found
                .as_ref()
                .and_then(|(_, _, info)| info.as_ref().and_then(instance_icon));
            let id = found.map(|(id, _, _)| id);
            list = list.child(
                div()
                    .id(SharedString::from(format!("storage-inst-{ix}")))
                    .flex()
                    .items_center()
                    .gap(space(3.0))
                    .h(px(48.0))
                    .px(space(4.0))
                    .when(ix > 0, |d| d.border_t_1().border_color(p.border))
                    .when_some(id, |d, id| {
                        d.cursor_pointer()
                            .hover(|s| s.bg(p.hover))
                            .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                                cx.emit(StorageEvent::OpenInstance(id.clone()))
                            }))
                    })
                    .child(thumb(source, &name, 28.0, radius::SM, cx))
                    .child(
                        div()
                            .w(px(220.0))
                            .flex_none()
                            .truncate()
                            .text_size(px(type_scale::BODY))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(p.text)
                            .child(name),
                    )
                    .child(
                        div()
                            .flex_1()
                            .h(px(6.0))
                            .rounded(px(3.0))
                            .bg(p.raised)
                            .child(
                                div()
                                    .h_full()
                                    .rounded(px(3.0))
                                    .w(relative((bytes as f32 / biggest as f32).max(0.01)))
                                    .bg(p.accent.opacity(0.7)),
                            ),
                    )
                    .child(
                        div()
                            .w(px(72.0))
                            .flex_none()
                            .text_right()
                            .text_size(px(type_scale::SMALL))
                            .text_color(p.text_secondary)
                            .child(human_bytes(bytes)),
                    ),
            );
        }
        let more = b.instances.len().saturating_sub(TOP);
        div()
            .flex()
            .flex_col()
            .gap(space(2.0))
            .child(list)
            .when(more > 0, |d| {
                d.child(
                    div().child(
                        Button::new("storage-show-all")
                            .label(if self.show_all {
                                "Show fewer".to_string()
                            } else {
                                format!("Show {more} more")
                            })
                            .small()
                            .ghost()
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.show_all = !this.show_all;
                                cx.notify();
                            })),
                    ),
                )
            })
            .into_any_element()
    }
}

impl Render for StoragePage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();
        let overview = self.overview(cx);
        let cleanup = self.cleanup(cx);
        let instances = self.instances(cx);
        let group = |title: &'static str, hint: Option<&'static str>, body: AnyElement| {
            div()
                .flex()
                .flex_col()
                .gap(space(3.0))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(space(1.0))
                        .child(section_heading(title, cx))
                        .when_some(hint, |d, h| {
                            d.child(
                                div()
                                    .text_size(px(type_scale::SMALL))
                                    .text_color(p.text_muted)
                                    .child(h),
                            )
                        }),
                )
                .child(body)
        };
        div()
            .id("storage-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(space(8.0))
                    .px(space(8.0))
                    .py(space(8.0))
                    .max_w(px(760.0))
                    .child(group("On disk", None, overview))
                    .child(group(
                        "Clean up",
                        Some("Only removes things Calcite can get back. Worlds, settings and screenshots are never touched."),
                        cleanup,
                    ))
                    .child(group("Instances by size", None, instances)),
            )
    }
}
