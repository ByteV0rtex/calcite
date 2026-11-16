//! Screenshots from every instance in one place, newest first.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};
use gpui_kit::prelude::*;
use gpui_kit::{
    ClickEvent, Context, Entity, FontWeight, ObjectFit, SharedString, Window, div, img, px,
};

use crate::backend::Backend;
use crate::images::Images;
use crate::state::AppState;
use crate::theme::{PaletteExt, radius, space, type_scale};
use crate::ui::components::{empty_state, relative_time, section_heading, skeleton};

#[derive(Clone)]
struct Shot {
    instance_id: String,
    path: PathBuf,
    taken: DateTime<Utc>,
}

/// How many pictures to show before "Show more".
const PAGE: usize = 60;

pub struct ScreenshotsPage {
    state: Entity<AppState>,
    shots: Option<Vec<Shot>>,
    /// Only one instance's pictures, or all of them.
    only: Option<String>,
    shown: usize,
}

impl ScreenshotsPage {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut page = ScreenshotsPage {
            state: AppState::global(cx),
            shots: None,
            only: None,
            shown: PAGE,
        };
        page.load(cx);
        page
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let task = Backend::global(cx).call(async move {
            // Straight from the backend: the app's list may not be loaded yet.
            let ids: Vec<String> = theseus::instance::list()
                .await?
                .into_iter()
                .map(|m| m.instance.id)
                .collect();
            let mut all = Vec::new();
            for id in ids {
                if let Ok(list) = theseus::instance::list_screenshots(&id).await {
                    all.extend(list.into_iter().map(|s| Shot {
                        instance_id: id.clone(),
                        path: s.path,
                        taken: s.created_at,
                    }));
                }
            }
            all.sort_by_key(|s| std::cmp::Reverse(s.taken));
            Ok(all)
        });
        cx.spawn(async move |this, cx| {
            let shots = task.await.unwrap_or_default();
            let _ = this.update(cx, |this, cx| {
                this.shots = Some(shots);
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();
        let state = self.state.read(cx);
        let names: std::collections::HashMap<String, SharedString> = state
            .instances
            .iter()
            .map(|i| (i.id.clone(), i.name.clone()))
            .collect();

        let header = div()
            .flex()
            .flex_col()
            .gap(space(3.0))
            .px(space(8.0))
            .pt(space(7.0))
            .pb(space(4.0))
            .child(
                div()
                    .text_size(px(type_scale::TITLE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(p.text)
                    .child("Screenshots"),
            );

        let Some(shots) = &self.shots else {
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

        // Filter chips: which instance's pictures to show.
        let mut counts: Vec<(String, usize)> = Vec::new();
        for s in shots {
            match counts.iter_mut().find(|(id, _)| *id == s.instance_id) {
                Some((_, n)) => *n += 1,
                None => counts.push((s.instance_id.clone(), 1)),
            }
        }
        let chip = |id: SharedString,
                    label: String,
                    active: bool,
                    target: Option<String>,
                    cx: &mut Context<Self>| {
            div()
                .id(id)
                .h(px(26.0))
                .px(space(2.5))
                .flex()
                .items_center()
                .rounded(px(radius::SM))
                .text_size(px(type_scale::SMALL))
                .cursor_pointer()
                .when(active, |d| d.bg(p.accent_soft).text_color(p.accent_text))
                .when(!active, |d| {
                    d.text_color(p.text_secondary).hover(|s| s.bg(p.hover))
                })
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.only = target.clone();
                    this.shown = PAGE;
                    cx.notify();
                }))
                .child(label)
        };
        let mut filters = div()
            .flex()
            .flex_wrap()
            .gap(space(1.0))
            .px(space(8.0))
            .pb(space(3.0));
        filters = filters.child(chip(
            "shots-all".into(),
            format!("All · {}", shots.len()),
            self.only.is_none(),
            None,
            cx,
        ));
        for (ix, (id, n)) in counts.iter().enumerate() {
            let name = names.get(id).cloned().unwrap_or_else(|| "Instance".into());
            filters = filters.child(chip(
                SharedString::from(format!("shots-{ix}")),
                format!("{name} · {n}"),
                self.only.as_deref() == Some(id.as_str()),
                Some(id.clone()),
                cx,
            ));
        }

        let visible: Vec<Shot> = shots
            .iter()
            .filter(|s| self.only.as_ref().is_none_or(|o| *o == s.instance_id))
            .take(self.shown)
            .cloned()
            .collect();
        let total_visible = shots
            .iter()
            .filter(|s| self.only.as_ref().is_none_or(|o| *o == s.instance_id))
            .count();

        // Group by day: "Today", "Sep 12, 2026".
        let mut body = div()
            .flex()
            .flex_col()
            .gap(space(6.0))
            .px(space(8.0))
            .pb(space(8.0));
        let mut day: Option<chrono::NaiveDate> = None;
        let mut grid = div().flex().flex_wrap().gap(space(3.0));
        let mut sections: Vec<(String, gpui_kit::Div)> = Vec::new();
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
            let name = names.get(&shot.instance_id).cloned().unwrap_or_default();
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
                            .child(div().truncate().text_color(p.text_secondary).child(name))
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(p.text_muted)
                                    .child(relative_time(shot.taken)),
                            ),
                    )
                    .context_menu(move |menu, _, _| {
                        let (reveal, delete, weak) = (reveal.clone(), delete.clone(), weak.clone());
                        menu.item(
                            PopupMenuItem::new("Show in folder").on_click(move |_, _, cx| {
                                if let Some(dir) = reveal.parent() {
                                    cx.open_with_system(dir);
                                }
                            }),
                        )
                        .item(
                            PopupMenuItem::new("Move to Trash").on_click(move |_, _, cx| {
                                let path = delete.clone();
                                let _ = weak.update(cx, |page, cx| page.delete(path, cx));
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
