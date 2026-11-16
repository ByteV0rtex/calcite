//! Worlds, Screenshots and Snapshots tabs of the instance page.

use std::path::PathBuf;

use calcite_core::snapshots::{self, Snapshot};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, FontWeight, ImageSource, ObjectFit, SharedString, div,
    img, px,
};

use crate::backend::{self, Backend};
use crate::images::Images;
use crate::state::AppState;
use crate::theme::{PaletteExt, radius, space, type_scale};
use crate::ui::components::{
    Button, chip, empty_state, icon, icon_chip, relative_time, skeleton, state_chip,
};
use crate::ui::pages::instance::{InstancePage, InstancePageEvent};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};

#[derive(Clone, Debug)]
pub struct WorldRow {
    pub name: SharedString,
    /// Folder name inside `saves/` (singleplayer) or the address (server).
    pub key: String,
    pub last_played: Option<chrono::DateTime<chrono::Utc>>,
    pub icon: Option<String>,
    pub server: bool,
    pub detail: SharedString,
    /// Position in the server list (servers only), for edit and remove.
    pub index: Option<usize>,
    /// Resource pack setting as the backend names it (servers only).
    pub pack: Option<serde_json::Value>,
}

/// What a server said when pinged.
#[derive(Clone, Debug)]
pub enum ServerPing {
    Waiting,
    Online {
        motd: Vec<calcite_core::motd::Span>,
        online: i32,
        max: i32,
        ping_ms: Option<i64>,
        version: Option<String>,
        favicon: Option<String>,
    },
    Offline,
}

#[derive(Clone, Debug)]
pub struct Shot {
    pub path: PathBuf,
    pub created: chrono::DateTime<chrono::Utc>,
}

#[derive(Default)]
pub struct Extra {
    pub instance_dir: Option<PathBuf>,
    pub worlds: Option<Result<Vec<WorldRow>, String>>,
    pub shots: Option<Result<Vec<Shot>, String>>,
    pub snapshots: Option<Vec<Snapshot>>,
    pub busy: Option<SharedString>,
    pub health: Option<Result<crate::ui::pages::health_tab::HealthReport, String>>,
    pub bisect: Option<(calcite_core::bisect::Bisect, calcite_core::bisect::Step)>,
    /// Live status by server address.
    pub servers: std::collections::HashMap<String, ServerPing>,
    /// A world waiting for "Delete forever?" confirmation (its folder).
    pub confirm_world: Option<String>,
}

pub fn snapshots_dir(instance_id: &str) -> Option<PathBuf> {
    backend::data_dir().map(|d| snapshots::dir_for(&d.join("calcite-snapshots"), instance_id))
}

impl InstancePage {
    pub(crate) fn load_worlds(&mut self, cx: &mut Context<Self>) {
        let id = self.id().to_string();
        let fetch = Backend::global(cx).call(async move {
            let path = theseus::instance::get_full_path(&id).await?;
            let worlds = theseus::worlds::get_instance_worlds(&id).await?;
            Ok((path, worlds))
        });
        cx.spawn(async move |this, cx| {
            let result = fetch.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok((path, worlds)) => {
                        let mut rows: Vec<WorldRow> = worlds
                            .into_iter()
                            .map(|w| {
                                let value = serde_json::to_value(&w).unwrap_or_default();
                                let server =
                                    value.get("type").and_then(|t| t.as_str()) == Some("server");
                                let key = if server {
                                    value
                                        .get("address")
                                        .and_then(|a| a.as_str())
                                        .unwrap_or_default()
                                        .to_string()
                                } else {
                                    value
                                        .get("path")
                                        .and_then(|a| a.as_str())
                                        .unwrap_or_default()
                                        .to_string()
                                };
                                let detail = if server {
                                    key.clone()
                                } else {
                                    let mode = value
                                        .get("game_mode")
                                        .and_then(|m| m.as_str())
                                        .unwrap_or("survival");
                                    let hardcore = value
                                        .get("hardcore")
                                        .and_then(|h| h.as_bool())
                                        .unwrap_or(false);
                                    let mut mode = title(mode);
                                    if hardcore {
                                        mode = "Hardcore".into();
                                    }
                                    mode
                                };
                                let icon = value
                                    .get("icon")
                                    .and_then(|i| i.as_str())
                                    .map(str::to_string);
                                WorldRow {
                                    name: w.name.clone().into(),
                                    key,
                                    last_played: w.last_played,
                                    icon,
                                    server,
                                    detail: detail.into(),
                                    index: value
                                        .get("index")
                                        .and_then(|i| i.as_u64())
                                        .map(|i| i as usize),
                                    pack: value.get("pack_status").cloned(),
                                }
                            })
                            .collect();
                        rows.sort_by_key(|r| std::cmp::Reverse(r.last_played));
                        this.extra.instance_dir = Some(path);
                        this.extra.worlds = Some(Ok(rows));
                        this.ping_servers(cx);
                    }
                    Err(error) => this.extra.worlds = Some(Err(format!("{error:#}"))),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn delete_world(&mut self, dir: PathBuf, folder: String, cx: &mut Context<Self>) {
        let task = Backend::global(cx)
            .call(async move { theseus::worlds::delete_world(&dir, &folder).await });
        self.after_world_change(task, "Couldn't delete that world", cx);
    }

    pub(crate) fn reset_world_icon(
        &mut self,
        dir: PathBuf,
        folder: String,
        cx: &mut Context<Self>,
    ) {
        let task = Backend::global(cx)
            .call(async move { theseus::worlds::reset_world_icon(&dir, &folder).await });
        self.after_world_change(task, "Couldn't reset the icon", cx);
    }

    fn after_world_change(
        &mut self,
        task: impl Future<Output = anyhow::Result<()>> + 'static,
        failure: &'static str,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = result {
                    this.state().update(cx, |s, cx| {
                        s.notify_error(failure, format!("{error:#}"));
                        cx.notify();
                    });
                }
                this.load_worlds(cx);
            });
        })
        .detach();
    }

    pub(crate) fn remove_server(&mut self, index: usize, cx: &mut Context<Self>) {
        let id = self.id().to_string();
        let task = Backend::global(cx)
            .call(async move { theseus::worlds::remove_server_from_instance(&id, index).await });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = result {
                    this.state().update(cx, |s, cx| {
                        s.notify_error("Couldn't remove that server", format!("{error:#}"));
                        cx.notify();
                    });
                }
                this.load_worlds(cx);
            });
        })
        .detach();
    }

    /// Ping every server in the list at once (6 s each at most).
    pub(crate) fn ping_servers(&mut self, cx: &mut Context<Self>) {
        let Some(Ok(rows)) = &self.extra.worlds else {
            return;
        };
        let addresses: Vec<String> = rows
            .iter()
            .filter(|w| w.server)
            .map(|w| w.key.clone())
            .collect();
        if addresses.is_empty() {
            return;
        }
        for address in &addresses {
            self.extra
                .servers
                .insert(address.clone(), ServerPing::Waiting);
        }
        let id = self.id().to_string();
        let task = Backend::global(cx).run(async move {
            let protocol = theseus::worlds::get_instance_protocol_version(&id)
                .await
                .ok()
                .flatten();
            let pings = addresses.into_iter().map(|address| async move {
                let status = tokio::time::timeout(
                    std::time::Duration::from_secs(6),
                    theseus::worlds::get_server_status(&address, protocol),
                )
                .await;
                let ping = match status {
                    Ok(Ok(status)) => ServerPing::Online {
                        motd: status
                            .description
                            .as_ref()
                            .and_then(|raw| {
                                serde_json::from_str::<serde_json::Value>(raw.get()).ok()
                            })
                            .map(|v| calcite_core::motd::parse(&v))
                            .unwrap_or_default(),
                        online: status.players.as_ref().map(|p| p.online).unwrap_or(0),
                        max: status.players.as_ref().map(|p| p.max).unwrap_or(0),
                        ping_ms: status.ping,
                        version: status.version.as_ref().map(|v| v.name.clone()),
                        favicon: status.favicon.as_ref().map(|u| u.to_string()),
                    },
                    _ => ServerPing::Offline,
                };
                (address, ping)
            });
            futures::future::join_all(pings).await
        });
        cx.spawn(async move |this, cx| {
            let Ok(results) = task.await else { return };
            let _ = this.update(cx, |this, cx| {
                this.extra.servers.extend(results);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn load_shots(&mut self, cx: &mut Context<Self>) {
        let id = self.id().to_string();
        let fetch =
            Backend::global(cx).call(async move { theseus::instance::list_screenshots(&id).await });
        cx.spawn(async move |this, cx| {
            let result = fetch.await;
            let _ = this.update(cx, |this, cx| {
                this.extra.shots = Some(
                    result
                        .map(|list| {
                            let mut shots: Vec<Shot> = list
                                .into_iter()
                                .map(|s| Shot {
                                    path: s.path,
                                    created: s.created_at,
                                })
                                .collect();
                            shots.sort_by_key(|s| std::cmp::Reverse(s.created));
                            shots
                        })
                        .map_err(|e| format!("{e:#}")),
                );
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn load_snapshots(&mut self, cx: &mut Context<Self>) {
        let Some(dir) = snapshots_dir(self.id()) else {
            return;
        };
        let list = Backend::global(cx).run(async move {
            tokio::task::spawn_blocking(move || snapshots::list(&dir))
                .await
                .unwrap_or_default()
        });
        cx.spawn(async move |this, cx| {
            let list = list.await.unwrap_or_default();
            let _ = this.update(cx, |this, cx| {
                this.extra.snapshots = Some(list);
                cx.notify();
            });
        })
        .detach();
    }

    fn snapshot_action(
        &mut self,
        busy: &'static str,
        done: impl Fn(&Snapshot) -> String + 'static,
        action: impl FnOnce(PathBuf, PathBuf) -> anyhow::Result<Snapshot> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some(snaps) = snapshots_dir(self.id()) else {
            return;
        };
        let id = self.id().to_string();
        self.extra.busy = Some(busy.into());
        cx.notify();
        let state = AppState::global(cx);
        let task = Backend::global(cx).run(async move {
            let dir = theseus::instance::get_full_path(&id)
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let result = tokio::task::spawn_blocking(move || action(dir, snaps)).await??;
            // Make the backend notice the swapped files.
            let _ = theseus::instance::sync_content_files(&id).await;
            Ok::<_, anyhow::Error>(result)
        });
        cx.spawn(async move |this, cx| {
            let result = task.await.and_then(|r| r);
            state.update(cx, |state, cx| {
                match &result {
                    Ok(snapshot) => {
                        state.notice = Some(crate::state::Notice {
                            title: done(snapshot).into(),
                            body: format!(
                                "{} files saved. Restoring later takes a moment and can itself be undone.",
                                snapshot.files
                            )
                            .into(),
                            is_error: false,
                        })
                    }
                    Err(error) => state.notify_error("Snapshot didn't finish", format!("{error:#}")),
                }
                cx.notify();
            });
            let _ = this.update(cx, |this, cx| {
                this.extra.busy = None;
                this.load_snapshots(cx);
                this.load_content(cx);
            });
        })
        .detach();
    }

    pub(crate) fn take_snapshot(&mut self, cx: &mut Context<Self>) {
        let label = format!("Saved {}", chrono::Local::now().format("%b %-d, %H:%M"));
        self.snapshot_action(
            "Saving a snapshot…",
            |_| "Snapshot saved".into(),
            move |dir, snaps| snapshots::take(&dir, &snaps, &label, false),
            cx,
        );
    }

    fn restore_snapshot(&mut self, snapshot: Snapshot, cx: &mut Context<Self>) {
        let label = snapshot.label.clone();
        self.snapshot_action(
            "Restoring…",
            move |_| format!("Restored \u{201c}{label}\u{201d}"),
            move |dir, snaps| snapshots::restore(&dir, &snaps, &snapshot.id),
            cx,
        );
    }

    fn delete_snapshot(&mut self, id: String, cx: &mut Context<Self>) {
        let Some(dir) = snapshots_dir(self.id()) else {
            return;
        };
        let task = Backend::global(cx).run(async move {
            tokio::task::spawn_blocking(move || snapshots::delete(&dir, &id)).await
        });
        cx.spawn(async move |this, cx| {
            let _ = task.await;
            let _ = this.update(cx, |this, cx| this.load_snapshots(cx));
        })
        .detach();
    }

    pub(crate) fn backup_world(&mut self, world: WorldRow, cx: &mut Context<Self>) {
        let Some(dir) = self.extra.instance_dir.clone() else {
            return;
        };
        self.extra.busy = Some(format!("Backing up {}…", world.name).into());
        cx.notify();
        let key = world.key.clone();
        let task = Backend::global(cx)
            .call(async move { theseus::worlds::backup_world(&dir, &key).await });
        let state = AppState::global(cx);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            state.update(cx, |state, cx| {
                match result {
                    Ok(bytes) => {
                        state.notice = Some(crate::state::Notice {
                            title: format!("Backed up {}", world.name).into(),
                            body: format!(
                                "{} saved to this instance's backups folder.",
                                calcite_core::transfer::human_bytes(bytes)
                            )
                            .into(),
                            is_error: false,
                        })
                    }
                    Err(error) => state.notify_error("Backup didn't finish", format!("{error:#}")),
                }
                cx.notify();
            });
            let _ = this.update(cx, |this, cx| {
                this.extra.busy = None;
                cx.notify();
            });
        })
        .detach();
    }

    // ------------------------------------------------------------ render

    pub(crate) fn worlds_tab(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette().clone();
        let worlds = match &self.extra.worlds {
            None => return loading_rows(cx),
            Some(Err(e)) => {
                return empty_state("circle-alert", "Couldn't read worlds", e.clone(), None, cx)
                    .into_any_element();
            }
            Some(Ok(w)) if w.is_empty() => {
                let add = Button::new("worlds-add-server-empty")
                    .icon("plus")
                    .label("Add server")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        cx.emit(InstancePageEvent::Server(this.id().to_string(), None))
                    }))
                    .into_any_element();
                return empty_state(
                    "globe",
                    "No worlds yet",
                    "Worlds you create in the game show up here. Add a server to join it straight from Calcite.",
                    Some(add),
                    cx,
                )
                .into_any_element();
            }
            Some(Ok(w)) => w.clone(),
        };
        let instance_id = self.id().to_string();
        let dir = self.extra.instance_dir.clone();
        let mut list = div()
            .flex()
            .flex_col()
            .px(space(8.0))
            .pt(space(3.0))
            .pb(space(8.0));
        for (ix, world) in worlds.into_iter().enumerate() {
            let ping = world
                .server
                .then(|| self.extra.servers.get(&world.key).cloned())
                .flatten();
            let favicon = match &ping {
                Some(ServerPing::Online {
                    favicon: Some(f), ..
                }) => Some(f.clone()),
                _ => None,
            };
            let icon_src: Option<ImageSource> =
                favicon.or(world.icon.clone()).as_deref().and_then(|i| {
                    if i.starts_with("http") || i.starts_with("data:") {
                        Images::get(i, 96, cx)
                    } else {
                        Images::get_file(std::path::Path::new(i), 96, cx)
                    }
                });
            let when = world.last_played.map(relative_time).unwrap_or_default();
            let (play_world, backup, open_dir) = (world.clone(), world.clone(), dir.clone());
            let play_id = instance_id.clone();
            let menu = world_menu(&world, dir.clone(), cx.entity().downgrade());
            let confirming =
                !world.server && self.extra.confirm_world.as_deref() == Some(world.key.as_str());
            if confirming {
                let (delete_key, delete_dir) = (world.key.clone(), dir.clone());
                list = list.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space(3.0))
                        .h(px(60.0))
                        .px(space(3.0))
                        .border_b_1()
                        .border_color(p.border)
                        .bg(p.danger_soft)
                        .child(icon("trash", 16.0, p.danger))
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(type_scale::BODY))
                                .text_color(p.text)
                                .child(format!(
                                    "Delete \"{}\" forever? Back it up first if you might want it.",
                                    world.name
                                )),
                        )
                        .child(
                            Button::new(SharedString::from(format!("world-delete-yes-{ix}")))
                                .label("Delete")
                                .danger()
                                .small()
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.extra.confirm_world = None;
                                    if let Some(dir) = delete_dir.clone() {
                                        this.delete_world(dir, delete_key.clone(), cx);
                                    }
                                })),
                        )
                        .child(
                            Button::new(SharedString::from(format!("world-delete-no-{ix}")))
                                .label("Keep")
                                .ghost()
                                .small()
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.extra.confirm_world = None;
                                    cx.notify();
                                })),
                        ),
                );
                continue;
            }
            list = list.child(
                div()
                    .id(SharedString::from(format!("world-row-{ix}")))
                    .flex()
                    .items_center()
                    .gap(space(3.0))
                    .h(px(60.0))
                    .px(space(3.0))
                    .border_b_1()
                    .border_color(p.border)
                    .hover(|s| s.bg(p.surface))
                    .child(
                        div()
                            .size(px(40.0))
                            .flex_none()
                            .rounded(px(radius::SM))
                            .overflow_hidden()
                            .bg(p.raised)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(match icon_src {
                                Some(src) => img(src)
                                    .size_full()
                                    .object_fit(ObjectFit::Cover)
                                    .into_any_element(),
                                None => icon(
                                    if world.server { "server" } else { "globe" },
                                    18.0,
                                    p.text_muted,
                                )
                                .into_any_element(),
                            }),
                    )
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
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.text)
                                    .child(world.name.clone()),
                            )
                            .child(match &ping {
                                Some(ServerPing::Online { motd, .. }) if !motd.is_empty() => {
                                    motd_line(motd, cx)
                                }
                                _ => div()
                                    .truncate()
                                    .text_size(px(type_scale::SMALL))
                                    .text_color(p.text_muted)
                                    .child(if when.is_empty() {
                                        world.detail.to_string()
                                    } else {
                                        format!("{} · {when}", world.detail)
                                    })
                                    .into_any_element(),
                            }),
                    )
                    .when(world.server, |d| {
                        let status: AnyElement = match &ping {
                            Some(ServerPing::Online {
                                online,
                                max,
                                ping_ms,
                                version,
                                ..
                            }) => div()
                                .flex()
                                .items_center()
                                .gap(space(1.5))
                                .child(icon_chip(
                                    "users",
                                    format!(
                                        "{} / {}",
                                        crate::ui::components::compact_number(
                                            (*online).max(0) as u64
                                        ),
                                        crate::ui::components::compact_number((*max).max(0) as u64)
                                    ),
                                    cx,
                                ))
                                .when_some(*ping_ms, |d, ms| d.child(latency_chip(ms, cx)))
                                .when_some(version.clone().filter(|v| v.len() <= 24), |d, v| {
                                    d.child(chip(v, cx))
                                })
                                .into_any_element(),
                            Some(ServerPing::Offline) => {
                                state_chip("Can't reach it", p.text_muted, p.raised)
                                    .into_any_element()
                            }
                            _ => crate::ui::components::skeleton(px(90.0), px(20.0), cx)
                                .into_any_element(),
                        };
                        let edit = crate::ui::server_dialog::Existing {
                            index: world.index.unwrap_or(0),
                            name: world.name.to_string(),
                            address: world.key.clone(),
                            pack: world.pack.clone(),
                        };
                        let (join_id, join_address) = (instance_id.clone(), world.key.clone());
                        let remove_index = world.index;
                        d.child(status)
                            .child(
                                Button::new(SharedString::from(format!("server-edit-{ix}")))
                                    .icon("pencil")
                                    .ghost()
                                    .small()
                                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                        cx.emit(InstancePageEvent::Server(
                                            this.id().to_string(),
                                            Some(edit.clone()),
                                        ))
                                    })),
                            )
                            .child(
                                Button::new(SharedString::from(format!("server-remove-{ix}")))
                                    .icon("trash")
                                    .ghost()
                                    .small()
                                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                        if let Some(index) = remove_index {
                                            this.remove_server(index, cx)
                                        }
                                    })),
                            )
                            .child(
                                Button::new(SharedString::from(format!("server-join-{ix}")))
                                    .icon("play")
                                    .label("Join")
                                    .small()
                                    .primary()
                                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                                        let (id, address) = (join_id.clone(), join_address.clone());
                                        AppState::global(cx)
                                            .update(cx, |s, cx| s.play_server(&id, address, cx));
                                    })),
                            )
                    })
                    .when(!world.server, |d| {
                        d.child(
                            Button::new(SharedString::from(format!("world-folder-{ix}")))
                                .icon("folder-open")
                                .ghost()
                                .small()
                                .on_click(move |_, _, cx| {
                                    if let Some(dir) = &open_dir {
                                        cx.open_with_system(&dir.join("saves").join(&backup.key));
                                    }
                                }),
                        )
                        .child(
                            Button::new(SharedString::from(format!("world-backup-{ix}")))
                                .icon("archive")
                                .label("Back up")
                                .small()
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.backup_world(play_world.clone(), cx)
                                })),
                        )
                        .child(
                            Button::new(SharedString::from(format!("world-play-{ix}")))
                                .icon("play")
                                .label("Play")
                                .small()
                                .primary()
                                .on_click({
                                    let key = world.key.clone();
                                    cx.listener(move |_, _: &ClickEvent, _, cx| {
                                        let (id, key) = (play_id.clone(), key.clone());
                                        AppState::global(cx)
                                            .update(cx, |s, cx| s.play_world(&id, key, cx));
                                    })
                                }),
                        )
                    })
                    .context_menu(menu),
            );
        }
        let add_bar = div()
            .flex()
            .justify_end()
            .gap(space(2.0))
            .px(space(8.0))
            .pt(space(4.0))
            .child(
                Button::new("worlds-refresh")
                    .icon("refresh-cw")
                    .label("Refresh")
                    .ghost()
                    .small()
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.load_worlds(cx))),
            )
            .child(
                Button::new("worlds-add-server")
                    .icon("plus")
                    .label("Add server")
                    .small()
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        cx.emit(InstancePageEvent::Server(this.id().to_string(), None))
                    })),
            );
        div()
            .size_full()
            .flex()
            .flex_col()
            .children(self.busy_bar(cx))
            .child(add_bar)
            .child(
                div()
                    .id("worlds-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(list),
            )
            .into_any_element()
    }

    pub(crate) fn shots_tab(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette().clone();
        let shots = match &self.extra.shots {
            None => return loading_rows(cx),
            Some(Err(e)) => {
                return empty_state(
                    "circle-alert",
                    "Couldn't read screenshots",
                    e.clone(),
                    None,
                    cx,
                )
                .into_any_element();
            }
            Some(Ok(s)) if s.is_empty() => {
                return empty_state(
                    "image",
                    "No screenshots yet",
                    "Press F2 in the game and your screenshots appear here.",
                    None,
                    cx,
                )
                .into_any_element();
            }
            Some(Ok(s)) => s.clone(),
        };
        let mut grid = div()
            .flex()
            .flex_wrap()
            .gap(space(3.0))
            .px(space(8.0))
            .py(space(4.0));
        for (ix, shot) in shots.into_iter().take(120).enumerate() {
            let src = Images::get_file(&shot.path, 480, cx);
            let path = shot.path.clone();
            grid = grid.child(
                div()
                    .id(ix)
                    .w(px(240.0))
                    .flex()
                    .flex_col()
                    .gap(space(1.5))
                    .cursor_pointer()
                    .on_click(move |_, _, cx| cx.open_with_system(&path))
                    .child(
                        div()
                            .w_full()
                            .h(px(135.0))
                            .rounded(px(radius::MD))
                            .overflow_hidden()
                            .bg(p.raised)
                            .border_1()
                            .border_color(p.border)
                            .hover(|s| s.border_color(p.border_strong))
                            .children(src.map(|s| img(s).size_full().object_fit(ObjectFit::Cover))),
                    )
                    .child(
                        div()
                            .text_size(px(type_scale::CAPTION))
                            .text_color(p.text_muted)
                            .child(relative_time(shot.created)),
                    ),
            );
        }
        div()
            .id("shots-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(grid)
            .into_any_element()
    }

    pub(crate) fn snapshots_tab(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette().clone();
        let toolbar = div()
            .flex()
            .items_center()
            .gap(space(2.0))
            .px(space(8.0))
            .py(space(3.0))
            .child(
                div()
                    .flex_1()
                    .text_size(px(type_scale::SMALL))
                    .text_color(p.text_muted)
                    .child("Mods, configs and packs, saved without using extra disk space. Calcite also saves one automatically before Update all."),
            )
            .child(
                Button::new("take-snapshot")
                    .icon("archive")
                    .label("Take snapshot")
                    .primary()
                    .small()
                    .disabled(self.extra.busy.is_some())
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.take_snapshot(cx))),
            );
        let list: AnyElement = match &self.extra.snapshots {
            None => loading_rows(cx),
            Some(s) if s.is_empty() => empty_state(
                "archive",
                "No snapshots yet",
                "Take one before trying new mods, so you can go back in one click.",
                None,
                cx,
            )
            .into_any_element(),
            Some(snapshots) => {
                let mut list = div().flex().flex_col().px(space(8.0)).pb(space(8.0));
                for (ix, snap) in snapshots.clone().into_iter().enumerate() {
                    let (restore, delete_id) = (snap.clone(), snap.id.clone());
                    list = list.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(space(3.0))
                            .h(px(56.0))
                            .px(space(3.0))
                            .border_b_1()
                            .border_color(p.border)
                            .child(icon(
                                "archive",
                                18.0,
                                if snap.automatic {
                                    p.text_muted
                                } else {
                                    p.accent
                                },
                            ))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(space(2.0))
                                            .child(
                                                div()
                                                    .truncate()
                                                    .text_size(px(type_scale::BODY))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(p.text)
                                                    .child(snap.label.clone()),
                                            )
                                            .when(snap.automatic, |d| {
                                                d.child(chip("Automatic", cx))
                                            }),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(type_scale::SMALL))
                                            .text_color(p.text_muted)
                                            .child(format!(
                                                "{} · {} files",
                                                relative_time(snap.created),
                                                snap.files
                                            )),
                                    ),
                            )
                            .child(
                                Button::new(SharedString::from(format!("snap-delete-{ix}")))
                                    .icon("trash")
                                    .ghost()
                                    .small()
                                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                        this.delete_snapshot(delete_id.clone(), cx)
                                    })),
                            )
                            .child(
                                Button::new(SharedString::from(format!("snap-restore-{ix}")))
                                    .icon("rotate-ccw")
                                    .label("Restore")
                                    .small()
                                    .disabled(self.extra.busy.is_some())
                                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                        this.restore_snapshot(restore.clone(), cx)
                                    })),
                            ),
                    );
                }
                list.into_any_element()
            }
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(toolbar)
            .children(self.busy_bar(cx))
            .child(
                div()
                    .id("snaps-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(list),
            )
            .into_any_element()
    }

    fn busy_bar(&self, cx: &App) -> Option<AnyElement> {
        let p = cx.palette();
        self.extra.busy.clone().map(|text| {
            div()
                .mx(space(8.0))
                .mb(space(2.0))
                .child(state_chip(text, p.info, p.info_soft))
                .into_any_element()
        })
    }
}

fn loading_rows(cx: &App) -> AnyElement {
    let mut list = div()
        .flex()
        .flex_col()
        .gap(space(3.0))
        .px(space(8.0))
        .pt(space(4.0));
    for _ in 0..4 {
        list = list.child(skeleton(gpui_kit::relative(1.0), px(44.0), cx));
    }
    list.into_any_element()
}

fn title(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// The first line of a server's MOTD in its own colours, kept readable on
/// the current theme.
fn motd_line(spans: &[calcite_core::motd::Span], cx: &App) -> AnyElement {
    let p = cx.palette();
    let dark = p.surface.l < 0.5;
    let mut first = calcite_core::motd::lines(spans)
        .into_iter()
        .next()
        .unwrap_or_default();
    // Servers pad MOTDs with spaces to centre them in game; drop that here.
    if let Some(lead) = first.first_mut() {
        lead.text = lead.text.trim_start().to_string();
    }
    div()
        .flex()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_size(px(type_scale::SMALL))
        .children(first.into_iter().map(|span| {
            let color = match span.color {
                Some(rgb) => {
                    let mut c: gpui_kit::Hsla = gpui_kit::rgb(rgb).into();
                    // Dark blues on a dark theme (or white on light) vanish.
                    c.l = if dark { c.l.max(0.62) } else { c.l.min(0.42) };
                    c
                }
                None => p.text_secondary,
            };
            div()
                .flex_none()
                .text_color(color)
                .when(span.bold, |d| d.font_weight(FontWeight::SEMIBOLD))
                .when(span.italic, |d| d.italic())
                .child(span.text)
        }))
        .into_any_element()
}

/// Latency with a colour hint: green under 100 ms, amber under 250.
fn latency_chip(ms: i64, cx: &App) -> gpui_kit::Div {
    let p = cx.palette();
    let (fg, bg) = if ms < 100 {
        (p.running, p.running_soft)
    } else if ms < 250 {
        (p.warning, p.warning_soft)
    } else {
        (p.danger, p.danger_soft)
    };
    state_chip(format!("{ms} ms"), fg, bg)
}

/// Right-click menu for a world or server row.
fn world_menu(
    world: &WorldRow,
    dir: Option<PathBuf>,
    page: gpui_kit::WeakEntity<InstancePage>,
) -> impl Fn(PopupMenu, &mut gpui_kit::Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    let world = world.clone();
    move |menu, _window, _cx| {
        let (w, d, pg) = (world.clone(), dir.clone(), page.clone());
        if w.server {
            let address = w.key.clone();
            let (edit_page, remove_page) = (pg.clone(), pg.clone());
            let edit = crate::ui::server_dialog::Existing {
                index: w.index.unwrap_or(0),
                name: w.name.to_string(),
                address: w.key.clone(),
                pack: w.pack.clone(),
            };
            let remove_index = w.index;
            return menu
                .item(
                    PopupMenuItem::new("Copy address").on_click(move |_, _, cx| {
                        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(address.clone()));
                    }),
                )
                .item(PopupMenuItem::new("Edit…").on_click(move |_, _, cx| {
                    let edit = edit.clone();
                    let _ = edit_page.update(cx, |p, cx| {
                        cx.emit(InstancePageEvent::Server(p.id().to_string(), Some(edit)))
                    });
                }))
                .separator()
                .item(
                    PopupMenuItem::new("Remove from list").on_click(move |_, _, cx| {
                        if let Some(index) = remove_index {
                            let _ = remove_page.update(cx, |p, cx| p.remove_server(index, cx));
                        }
                    }),
                );
        }
        let (rename_page, icon_page, backup_page, delete_page) =
            (pg.clone(), pg.clone(), pg.clone(), pg.clone());
        let (rename_w, icon_w, backup_w, delete_w, folder_w) =
            (w.clone(), w.clone(), w.clone(), w.clone(), w.clone());
        let (rename_d, icon_d, folder_d) = (d.clone(), d.clone(), d.clone());
        menu.item(PopupMenuItem::new("Rename…").on_click(move |_, _, cx| {
            if let Some(dir) = rename_d.clone() {
                let (key, name) = (rename_w.key.clone(), rename_w.name.to_string());
                let _ = rename_page.update(cx, |_, cx| {
                    cx.emit(InstancePageEvent::RenameWorld(dir, key, name))
                });
            }
        }))
        .item(PopupMenuItem::new("Reset icon").on_click(move |_, _, cx| {
            if let Some(dir) = icon_d.clone() {
                let key = icon_w.key.clone();
                let _ = icon_page.update(cx, |p, cx| p.reset_world_icon(dir, key, cx));
            }
        }))
        .item(PopupMenuItem::new("Open folder").on_click(move |_, _, cx| {
            if let Some(dir) = &folder_d {
                cx.open_with_system(&dir.join("saves").join(&folder_w.key));
            }
        }))
        .item(PopupMenuItem::new("Back up now").on_click(move |_, _, cx| {
            let w = backup_w.clone();
            let _ = backup_page.update(cx, |p, cx| p.backup_world(w, cx));
        }))
        .separator()
        .item(PopupMenuItem::new("Delete…").on_click(move |_, _, cx| {
            let key = delete_w.key.clone();
            let _ = delete_page.update(cx, |p, cx| {
                p.extra.confirm_world = Some(key);
                cx.notify();
            });
        }))
    }
}
