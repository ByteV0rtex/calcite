//! Library: every instance, grouped, with Play one click away.

use chrono::Utc;
use gpui_kit::component::input::Input;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, ClickEvent, Context, FontWeight, SharedString, Window, div, px};

use crate::state::{InstanceInfo, Job};
use crate::theme::{PaletteExt, radius, space, type_scale};
use crate::ui::components::{
    Button, chip, empty_state, icon, progress_bar, relative_time, short_duration, thumb,
};
use crate::ui::pages::page_header;
use crate::ui::shell::{LibrarySort, Route, SIDEBAR_WIDTH, Shell, instance_icon, running_chip};

/// Card width that fills the row exactly, given the space the page has.
fn card_width(available: f32) -> f32 {
    const MIN: f32 = 248.0;
    const GAP: f32 = 12.0;
    let cols = ((available + GAP) / (MIN + GAP)).floor().max(1.0);
    ((available - GAP * (cols - 1.0)) / cols).floor()
}

/// Case-insensitive match on name, loader or version.
fn matches_filter(inst: &InstanceInfo, filter: &str) -> bool {
    if filter.is_empty() {
        return true;
    }
    let filter = filter.to_lowercase();
    inst.name.to_lowercase().contains(&filter)
        || inst.loader_line().to_lowercase().contains(&filter)
}

pub fn render(shell: &mut Shell, window: &mut Window, cx: &mut Context<Shell>) -> AnyElement {
    let p = cx.palette().clone();
    let available = f32::from(window.viewport_size().width) - SIDEBAR_WIDTH - 2.0 * 32.0 - 12.0;
    let card_w = card_width(available);

    let state = shell.state().read(cx);
    let this_signed_in = state.modrinth.is_some();
    let filter = shell.library_filter.read(cx).value().to_string();
    let mut instances: Vec<InstanceInfo> = state
        .instances
        .iter()
        .filter(|i| matches_filter(i, filter.trim()))
        .cloned()
        .collect();
    match shell.library_sort {
        LibrarySort::LastPlayed => {}
        LibrarySort::Name => instances.sort_by_key(|a| a.name.to_lowercase()),
        LibrarySort::Created => instances.sort_by_key(|i| std::cmp::Reverse(i.created)),
    }
    let total = state.instances.len();
    let groups = state.groups.clone();
    let running = state.running.clone();
    let jobs: Vec<Job> = state.jobs.clone();

    // Group sections in the user's order, then everything ungrouped.
    let mut sections: Vec<(SharedString, Vec<InstanceInfo>)> = Vec::new();
    for (group_id, name) in &groups {
        let members: Vec<_> = instances
            .iter()
            .filter(|i| i.group_ids.contains(group_id))
            .cloned()
            .collect();
        if !members.is_empty() {
            sections.push((name.clone(), members));
        }
    }
    let ungrouped: Vec<_> = instances
        .iter()
        .filter(|i| {
            !i.group_ids
                .iter()
                .any(|g| groups.iter().any(|(id, _)| id == g))
        })
        .cloned()
        .collect();
    if !ungrouped.is_empty() {
        let title = if sections.is_empty() {
            ""
        } else {
            "Other instances"
        };
        sections.push((title.into(), ungrouped));
    }

    let toolbar = div()
        .flex()
        .items_center()
        .gap(space(2.0))
        .child(
            div().w(px(260.0)).child(
                Input::new(&shell.library_filter)
                    .prefix(icon("search", 14.0, p.text_muted))
                    .cleanable(true),
            ),
        )
        .child(
            Button::new("sort")
                .icon("arrow-up-down")
                .label(shell.library_sort.label())
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.cycle_sort(cx))),
        )
        .child(div().flex_1())
        .when(this_signed_in, |d| {
            d.child(
                Button::new("join-shared")
                    .icon("link")
                    .label("Join")
                    .on_click(
                        cx.listener(|this, _: &ClickEvent, window, cx| this.open_join(window, cx)),
                    ),
            )
        })
        .child(
            Button::new("move-in")
                .icon("arrow-down-to-line")
                .label("Move in")
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.open_settings(crate::ui::pages::settings::Section::MoveIn, window, cx)
                })),
        )
        .child(
            Button::new("new-instance")
                .icon("plus")
                .label("New instance")
                .primary()
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.open_create_dialog(window, cx)
                })),
        );

    let body: AnyElement = if total == 0 {
        empty_state(
            "layout-grid",
            "No instances yet",
            "Install a modpack from Discover or create an instance with the loader and version you want.",
            Some(
                div()
                    .flex()
                    .gap(space(2.0))
                    .child(
                        Button::new("empty-new")
                            .icon("plus")
                            .label("New instance")
                            .primary()
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.open_create_dialog(window, cx)
                            })),
                    )
                    .child(
                        Button::new("empty-move-in")
                            .icon("arrow-down-to-line")
                            .label("Move in from another launcher")
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.open_settings(crate::ui::pages::settings::Section::MoveIn, window, cx)
                            })),
                    )
                    .child(
                        Button::new("empty-discover")
                            .icon("compass")
                            .label("Browse modpacks")
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.navigate(Route::Discover, window, cx)
                            })),
                    )
                    .into_any_element(),
            ),
            cx,
        )
        .into_any_element()
    } else if instances.is_empty() {
        empty_state(
            "search",
            "No instances match",
            "Try a different name, loader or version.",
            None,
            cx,
        )
        .into_any_element()
    } else {
        let mut list = div().flex().flex_col().gap(space(8.0));
        for (title, members) in sections {
            let mut grid = div().flex().flex_wrap().gap(space(3.0));
            let count = members.len();
            for inst in members {
                let run = running
                    .get(&inst.id)
                    .map(|r| (Utc::now() - r.started).num_seconds().max(0) as u64);
                let job = jobs
                    .iter()
                    .find(|j| j.is_active() && j.instance_id.as_deref() == Some(&inst.id))
                    .cloned();
                grid = grid.child(instance_card(&inst, run, job, card_w, cx));
            }
            list = list.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(space(3.0))
                    .when(!title.is_empty(), |d| {
                        d.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(space(2.0))
                                .child(
                                    div()
                                        .text_size(px(type_scale::BODY_LG))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(p.text)
                                        .child(title.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(type_scale::SMALL))
                                        .text_color(p.text_muted)
                                        .child(format!("{count}")),
                                ),
                        )
                    })
                    .child(grid),
            );
        }
        list.into_any_element()
    };

    div()
        .size_full()
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .flex_col()
                .gap(space(5.0))
                .px(space(8.0))
                .pt(space(7.0))
                .pb(space(6.0))
                .child(page_header(
                    "Library",
                    (total > 0)
                        .then(|| format!("{total} instance{}", if total == 1 { "" } else { "s" })),
                    cx,
                ))
                .when(total > 0, |d| d.child(toolbar)),
        )
        .child(
            div()
                .id("library-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .px(space(8.0))
                .pb(space(10.0))
                .child(body),
        )
        .into_any_element()
}

fn instance_card(
    inst: &InstanceInfo,
    running_secs: Option<u64>,
    job: Option<Job>,
    width: f32,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let p = cx.palette().clone();
    let source = instance_icon(inst);
    let id = inst.id.clone();
    let installing = job.is_some() || !inst.installed;

    let footer_left: AnyElement = if let Some(secs) = running_secs {
        running_chip(secs, cx).into_any_element()
    } else if let Some(job) = &job {
        div()
            .flex()
            .flex_col()
            .gap(space(1.5))
            .w(px(width * 0.55))
            .child(
                div()
                    .text_size(px(type_scale::CAPTION))
                    .text_color(p.text_muted)
                    .child(job.phase),
            )
            .child(progress_bar(job.fraction(), cx))
            .into_any_element()
    } else if !inst.installed {
        div()
            .text_size(px(type_scale::SMALL))
            .text_color(p.warning)
            .child("Not fully installed")
            .into_any_element()
    } else {
        let played = if inst.played_secs > 0 {
            short_duration(inst.played_secs)
        } else {
            "Not played yet".into()
        };
        let when = inst
            .last_played
            .map(relative_time)
            .map(|t| format!(" · {t}"))
            .unwrap_or_default();
        div()
            .text_size(px(type_scale::SMALL))
            .text_color(p.text_muted)
            .child(format!("{played}{when}"))
            .into_any_element()
    };

    let action = if running_secs.is_some() {
        let id = id.clone();
        div()
            .id(SharedString::from(format!("stop-{id}")))
            .size(px(32.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(radius::MD))
            .bg(p.danger_soft)
            .cursor_pointer()
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                cx.stop_propagation();
                this.state().update(cx, |state, cx| state.stop(&id, cx));
            }))
            .child(icon("square", 14.0, p.danger))
    } else {
        let id = id.clone();
        div()
            .id(SharedString::from(format!("play-{id}")))
            .size(px(32.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(radius::MD))
            .bg(p.raised)
            .when(installing, |d| d.opacity(0.4))
            .when(!installing, |d| {
                d.cursor_pointer()
                    .hover(|s| s.bg(p.accent))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        this.state().update(cx, |state, cx| state.play(&id, cx));
                    }))
            })
            .child(icon("play", 14.0, p.text))
    };

    div()
        .id(SharedString::from(format!("card-{}", inst.id)))
        .w(px(width))
        .flex()
        .flex_col()
        .rounded(px(radius::LG))
        .bg(p.surface)
        .border_1()
        .border_color(if running_secs.is_some() {
            p.running_soft
        } else {
            p.border
        })
        .cursor_pointer()
        .hover(|s| s.border_color(p.border_strong))
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            this.navigate(Route::Instance(id.clone()), window, cx)
        }))
        .child(
            div()
                .flex()
                .gap(space(3.0))
                .p(space(3.5))
                .child(thumb(source, &inst.name, 48.0, radius::MD, cx))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .min_w_0()
                        .flex_1()
                        .gap(space(1.5))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_size(px(type_scale::BODY_LG))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(p.text)
                                .child(inst.name.clone()),
                        )
                        .child(
                            div()
                                .flex()
                                .gap(space(1.5))
                                .overflow_hidden()
                                .child(
                                    chip(inst.loader_line(), cx)
                                        .flex_shrink(1.0)
                                        .min_w_0()
                                        .truncate(),
                                )
                                .when(inst.is_modpack, |d| d.child(chip("Modpack", cx))),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(space(2.0))
                .h(px(48.0))
                .px(space(3.5))
                .border_t_1()
                .border_color(p.border)
                .child(footer_left)
                .child(action),
        )
}

#[cfg(test)]
mod tests {
    use super::card_width;

    #[test]
    fn cards_fill_the_row() {
        // 3 columns of 276px fit in 852px with 12px gaps.
        assert_eq!(card_width(852.0), 276.0);
        // Narrow windows fall back to one full-width column.
        assert_eq!(card_width(200.0), 200.0);
    }
}
