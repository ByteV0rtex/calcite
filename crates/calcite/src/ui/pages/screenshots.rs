//! Screenshots from every instance in one place, with albums.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};
use gpui_kit::prelude::*;
use gpui_kit::{
    ClickEvent, Context, Entity, FontWeight, ObjectFit, SharedString, Window, div, img, px,
};

use crate::backend::Backend;
use crate::images::Images;
use crate::state::AppState;
use crate::theme::{PaletteExt, radius, space, type_scale};
use crate::ui::components::{empty_state, icon, relative_time, section_heading, skeleton};

#[derive(Clone)]
struct Shot {
    id: String,
    instance_id: String,
    instance_name: SharedString,
    path: PathBuf,
    taken: DateTime<Utc>,
    album: Option<String>,
}

/// Which pictures to show.
#[derive(Clone, PartialEq)]
enum Show {
    All,
    Instance(String),
    Album(String),
}

/// How many pictures to show before "Show more".
const PAGE: usize = 60;

pub struct ScreenshotsPage {
    state: gpui_kit::Entity<AppState>,
    shots: Option<Vec<Shot>>,
    albums: Vec<(String, SharedString)>,
    show: Show,
    shown: usize,
    oldest_first: bool,
    /// Made on first render (inputs need the window).
    search: Option<Entity<InputState>>,
    /// "New album" asked for; the name box appears on the next render.
    pending_album: bool,
    new_album: Option<Entity<InputState>>,
}

impl ScreenshotsPage {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut page = ScreenshotsPage {
            state: AppState::global(cx),
            shots: None,
            albums: Vec::new(),
            show: Show::All,
            shown: PAGE,
            oldest_first: false,
            search: None,
            pending_album: false,
            new_album: None,
        };
        page.load(cx);
        page
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let task = Backend::global(cx).call(async move {
            let shots = theseus::instance::list_all_screenshots().await?;
            let albums = theseus::instance::list_screenshot_groups()
                .await
                .unwrap_or_default();
            Ok((shots, albums))
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                let (shots, albums) = result.unwrap_or_default();
                this.shots = Some(
                    shots
                        .into_iter()
                        .map(|s| Shot {
                            id: s.id,
                            instance_id: s.instance_id,
                            instance_name: s.instance_name.into(),
                            path: s.path,
                            taken: s.created_at,
                            album: s.group_id,
                        })
                        .collect(),
                );
                this.albums = albums.into_iter().map(|a| (a.id, a.name.into())).collect();
                if let Show::Album(id) = &this.show
                    && !this.albums.iter().any(|(a, _)| a == id)
                {
                    this.show = Show::All;
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn delete(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if let Some(shots) = &mut self.shots {
            shots.retain(|s| s.path != path);
        }
        // Into the system trash, so a slip can be undone.
        let task = cx.background_spawn(async move { trash_file(&path) });
        cx.spawn(async move |this, cx| {
            if let Err(error) = task.await {
                let _ = this.update(cx, |this, cx| {
                    this.state.update(cx, |s, cx| {
                        s.notify_error("Couldn't delete that screenshot", format!("{error:#}"));
                        cx.notify();
                    });
                    this.load(cx);
                });
            }
        })
        .detach();
        cx.notify();
    }

    /// Put a picture in an album (or, with None, take it out).
    fn set_album(&mut self, shot: String, album: Option<String>, cx: &mut Context<Self>) {
        if let Some(s) = self
            .shots
            .as_mut()
            .and_then(|shots| shots.iter_mut().find(|s| s.id == shot))
        {
            s.album = album.clone();
        }
        cx.notify();
        let task = Backend::global(cx).call(async move {
            theseus::instance::set_screenshot_group_memberships(vec![
                theseus::instance::ScreenshotGroupMembershipUpdate {
                    screenshot_id: shot,
                    group_id: album,
                },
            ])
            .await
        });
        self.after(task, "Couldn't change the album", cx);
    }

    fn create_album(&mut self, cx: &mut Context<Self>) {
        let Some(input) = self.new_album.take() else {
            return;
        };
        let name = input.read(cx).value().trim().to_string();
        if name.is_empty() {
            cx.notify();
            return;
        }
        let task = Backend::global(cx).call(async move {
            theseus::instance::create_screenshot_group(name, Vec::new())
                .await
                .map(|_| ())
        });
        self.after(task, "Couldn't make the album", cx);
    }

    fn delete_album(&mut self, id: String, cx: &mut Context<Self>) {
        self.show = Show::All;
        let task = Backend::global(cx)
            .call(async move { theseus::instance::delete_screenshot_group(id).await });
        self.after(task, "Couldn't delete the album", cx);
    }

    fn after(
        &mut self,
        task: impl std::future::Future<Output = anyhow::Result<()>> + 'static,
        failure: &'static str,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = result {
                    this.state.update(cx, |s, cx| {
                        s.notify_error(failure, format!("{error:#}"));
                        cx.notify();
                    });
                }
                this.load(cx);
            });
        })
        .detach();
    }

    fn make_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search.is_none() {
            let input = cx.new(|cx| {
                InputState::new(window, cx).placeholder("Search by instance or file name")
            });
            cx.subscribe(&input, |this: &mut Self, _, event: &InputEvent, cx| {
                if let InputEvent::Change = event {
                    this.shown = PAGE;
                    cx.notify();
                }
            })
            .detach();
            self.search = Some(input);
        }
        if self.pending_album {
            self.pending_album = false;
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Album name"));
            input.update(cx, |s, cx| s.focus(window, cx));
            cx.subscribe(&input, |this: &mut Self, _, event: &InputEvent, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    this.create_album(cx);
                }
            })
            .detach();
            self.new_album = Some(input);
        }
    }
}

/// Move a file to the trash where the system has one.
fn trash_file(path: &std::path::Path) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("osascript")
            .arg("-e")
            .arg(format!(
                "tell application \"Finder\" to delete POSIX file \"{}\"",
                path.display().to_string().replace('"', "\\\"")
            ))
            .output()?;
        anyhow::ensure!(
            status.status.success(),
            "Finder couldn't move it to the Trash"
        );
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::fs::remove_file(path)?;
        Ok(())
    }
}

impl Render for ScreenshotsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.make_inputs(window, cx);
        let p = cx.palette().clone();

        let header = div()
            .flex()
            .items_center()
            .gap(space(3.0))
            .px(space(8.0))
            .pt(space(7.0))
            .pb(space(4.0))
            .child(
                div()
                    .flex_1()
                    .text_size(px(type_scale::TITLE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(p.text)
                    .child("Screenshots"),
            )
            .children(self.search.as_ref().map(|s| {
                div()
                    .w(px(280.0))
                    .child(Input::new(s).small().cleanable(true))
            }))
            .child(
                crate::ui::components::Button::new("shots-order")
                    .icon(if self.oldest_first {
                        "arrow-up"
                    } else {
                        "arrow-down"
                    })
                    .label(if self.oldest_first {
                        "Oldest first"
                    } else {
                        "Newest first"
                    })
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.oldest_first = !this.oldest_first;
                        cx.notify();
                    })),
            );

        let Some(shots) = self.shots.clone() else {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .child(header)
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(space(3.0))
                        .px(space(8.0))
                        .children((0..9).map(|_| skeleton(px(240.0), px(135.0), cx))),
                )
                .into_any_element();
        };
        if shots.is_empty() {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .child(header)
                .child(empty_state(
                    "image",
                    "No screenshots yet",
                    "Press F2 in the game. Screenshots from every instance show up here.",
                    None,
                    cx,
                ))
                .into_any_element();
        }

        // Filter chips: everything, one instance, or one album.
        let mut counts: Vec<(String, SharedString, usize)> = Vec::new();
        for s in &shots {
            match counts.iter_mut().find(|(id, _, _)| *id == s.instance_id) {
                Some((_, _, n)) => *n += 1,
                None => counts.push((s.instance_id.clone(), s.instance_name.clone(), 1)),
            }
        }
        let chip =
            |id: SharedString, label: String, active: bool, show: Show, cx: &mut Context<Self>| {
                div()
                    .id(id)
                    .h(px(26.0))
                    .px(space(2.5))
                    .flex()
                    .items_center()
                    .gap(space(1.5))
                    .rounded(px(radius::SM))
                    .text_size(px(type_scale::SMALL))
                    .cursor_pointer()
                    .when(active, |d| d.bg(p.accent_soft).text_color(p.accent_text))
                    .when(!active, |d| {
                        d.text_color(p.text_secondary).hover(|s| s.bg(p.hover))
                    })
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.show = show.clone();
                        this.shown = PAGE;
                        cx.notify();
                    }))
                    .child(label)
            };
        let mut filters = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(space(1.0))
            .px(space(8.0))
            .pb(space(2.0));
        filters = filters.child(chip(
            "shots-all".into(),
            format!("All · {}", shots.len()),
            self.show == Show::All,
            Show::All,
            cx,
        ));
        for (ix, (id, name, n)) in counts.iter().enumerate() {
            filters = filters.child(chip(
                SharedString::from(format!("shots-{ix}")),
                format!("{name} · {n}"),
                self.show == Show::Instance(id.clone()),
                Show::Instance(id.clone()),
                cx,
            ));
        }

        // Albums row.
        let mut albums = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(space(1.0))
            .px(space(8.0))
            .pb(space(3.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space(1.5))
                    .pr(space(1.0))
                    .text_size(px(type_scale::SMALL))
                    .text_color(p.text_muted)
                    .child(icon("library-big", 13.0, p.text_muted))
                    .child("Albums"),
            );
        for (ix, (id, name)) in self.albums.iter().enumerate() {
            let n = shots
                .iter()
                .filter(|s| s.album.as_ref() == Some(id))
                .count();
            let active = self.show == Show::Album(id.clone());
            let delete = id.clone();
            let weak = cx.entity().downgrade();
            albums = albums.child(
                chip(
                    SharedString::from(format!("album-{ix}")),
                    format!("{name} · {n}"),
                    active,
                    Show::Album(id.clone()),
                    cx,
                )
                .context_menu(move |menu, _, _| {
                    let (delete, weak) = (delete.clone(), weak.clone());
                    menu.item(
                        PopupMenuItem::new("Delete album").on_click(move |_, _, cx| {
                            let id = delete.clone();
                            let _ = weak.update(cx, |page, cx| page.delete_album(id, cx));
                        }),
                    )
                }),
            );
        }
        albums = match &self.new_album {
            Some(input) => albums.child(
                div()
                    .flex()
                    .items_center()
                    .gap(space(1.0))
                    .child(div().w(px(180.0)).child(Input::new(input).small()))
                    .child(
                        crate::ui::components::Button::new("album-create")
                            .label("Make album")
                            .small()
                            .on_click(
                                cx.listener(|this, _: &ClickEvent, _, cx| this.create_album(cx)),
                            ),
                    ),
            ),
            None => albums.child(
                div()
                    .id("album-new")
                    .h(px(26.0))
                    .px(space(2.5))
                    .flex()
                    .items_center()
                    .gap(space(1.0))
                    .rounded(px(radius::SM))
                    .border_1()
                    .border_color(p.border)
                    .text_size(px(type_scale::SMALL))
                    .text_color(p.text_secondary)
                    .cursor_pointer()
                    .hover(|s| s.bg(p.hover))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.pending_album = true;
                        cx.notify();
                    }))
                    .child(icon("plus", 12.0, p.text_muted))
                    .child("New album"),
            ),
        };

        let query = self
            .search
            .as_ref()
            .map(|s| s.read(cx).value().trim().to_lowercase())
            .unwrap_or_default();
        let mut matching: Vec<Shot> = shots
            .iter()
            .filter(|s| match &self.show {
                Show::All => true,
                Show::Instance(id) => s.instance_id == *id,
                Show::Album(id) => s.album.as_ref() == Some(id),
            })
            .filter(|s| {
                query.is_empty()
                    || s.instance_name.to_lowercase().contains(&query)
                    || s.path
                        .file_name()
                        .is_some_and(|n| n.to_string_lossy().to_lowercase().contains(&query))
            })
            .cloned()
            .collect();
        matching.sort_by_key(|s| s.taken);
        if !self.oldest_first {
            matching.reverse();
        }
        let total_visible = matching.len();
        let visible: Vec<Shot> = matching.into_iter().take(self.shown).collect();

        // Group by day: "Today", "Sep 12, 2026".
        let mut body = div()
            .flex()
            .flex_col()
            .gap(space(6.0))
            .px(space(8.0))
            .pb(space(8.0));
        if visible.is_empty() {
            body = body.child(empty_state(
                "image",
                "Nothing here",
                match &self.show {
                    Show::Album(_) => "Right-click a screenshot and add it to this album.",
                    _ => "No screenshots match.",
                },
                None,
                cx,
            ));
        }
        let mut day: Option<chrono::NaiveDate> = None;
        let mut grid = div().flex().flex_wrap().gap(space(3.0));
        let mut sections: Vec<(String, gpui_kit::Div)> = Vec::new();
        let albums_list = self.albums.clone();
        for (ix, shot) in visible.into_iter().enumerate() {
            let date = shot.taken.with_timezone(&chrono::Local).date_naive();
            if day != Some(date) {
                if let Some(previous) = day {
                    sections.push((
                        label_for(previous),
                        std::mem::replace(&mut grid, div().flex().flex_wrap().gap(space(3.0))),
                    ));
                }
                day = Some(date);
            }
            let src = Images::get_file(&shot.path, 480, cx);
            let (open, reveal, delete) = (shot.path.clone(), shot.path.clone(), shot.path.clone());
            let weak = cx.entity().downgrade();
            let (shot_id, in_album) = (shot.id.clone(), shot.album.clone());
            let albums_list = albums_list.clone();
            grid = grid.child(
                div()
                    .id(SharedString::from(format!("shot-{ix}")))
                    .w(px(240.0))
                    .flex()
                    .flex_col()
                    .gap(space(1.5))
                    .cursor_pointer()
                    .on_click(move |_, _, cx| cx.open_with_system(&open))
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
                            .flex()
                            .justify_between()
                            .gap(space(2.0))
                            .text_size(px(type_scale::CAPTION))
                            .child(
                                div()
                                    .truncate()
                                    .text_color(p.text_secondary)
                                    .child(shot.instance_name.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(p.text_muted)
                                    .child(relative_time(shot.taken)),
                            ),
                    )
                    .context_menu(move |menu, _, _| {
                        let (reveal, delete, weak) = (reveal.clone(), delete.clone(), weak.clone());
                        let mut menu = menu.item(PopupMenuItem::new("Show in folder").on_click(
                            move |_, _, cx| {
                                if let Some(dir) = reveal.parent() {
                                    cx.open_with_system(dir);
                                }
                            },
                        ));
                        if !albums_list.is_empty() {
                            menu = menu.separator();
                        }
                        for (album, name) in &albums_list {
                            let (weak, id, album) = (weak.clone(), shot_id.clone(), album.clone());
                            let here = in_album.as_ref() == Some(&album);
                            menu = menu.item(
                                PopupMenuItem::new(format!("In \u{201c}{name}\u{201d}"))
                                    .checked(here)
                                    .on_click(move |_, _, cx| {
                                        let (id, album) = (id.clone(), album.clone());
                                        let _ = weak.update(cx, |page, cx| {
                                            page.set_album(id, (!here).then_some(album), cx)
                                        });
                                    }),
                            );
                        }
                        let weak_delete = weak.clone();
                        menu.separator()
                            .item(
                                PopupMenuItem::new("Move to Trash").on_click(move |_, _, cx| {
                                    let path = delete.clone();
                                    let _ =
                                        weak_delete.update(cx, |page, cx| page.delete(path, cx));
                                }),
                            )
                    }),
            );
        }
        if let Some(last) = day {
            sections.push((label_for(last), grid));
        }
        for (title, grid) in sections {
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(space(3.0))
                    .child(section_heading(title, cx))
                    .child(grid),
            );
        }
        if total_visible > self.shown {
            body = body.child(
                div().child(
                    crate::ui::components::Button::new("shots-more")
                        .label(format!(
                            "Show {} more",
                            (total_visible - self.shown).min(PAGE)
                        ))
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.shown += PAGE;
                            cx.notify();
                        })),
                ),
            );
        }

        div()
            .id("screenshots-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(header)
            .child(filters)
            .child(albums)
            .child(body)
            .into_any_element()
    }
}

fn label_for(date: chrono::NaiveDate) -> String {
    let today = chrono::Local::now().date_naive();
    match (today - date).num_days() {
        0 => "Today".into(),
        1 => "Yesterday".into(),
        _ => date.format("%b %-d, %Y").to_string(),
    }
}
