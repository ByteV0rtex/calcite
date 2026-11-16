//! Library: every instance, grouped, with Play one click away.

use std::rc::Rc;

use chrono::Utc;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, FontWeight, MouseDownEvent, SharedString, Window, div, px,
};
use theseus::data::ModLoader;

use crate::state::{InstanceInfo, Job, loader_name};
use crate::theme::{PaletteExt, radius, space, type_scale};
use crate::ui::components::{
    Button, chip, empty_state, icon, progress_bar, relative_time, short_duration, thumb,
};
use crate::ui::pages::page_header;
use crate::ui::shell::{
    KindFilter, LibraryGroup, LibrarySort, Route, SIDEBAR_WIDTH, Shell, instance_icon, running_chip,
};

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

/// What a dropdown item does when picked.
type MenuAction = Rc<dyn Fn(&mut Shell, &mut Window, &mut Context<Shell>)>;
/// A dropdown item: label, whether it's the current choice, action.
type MenuItem = (SharedString, bool, MenuAction);
/// A Library section: group id (for your own groups), title, instances.
type Section = (Option<String>, SharedString, Vec<InstanceInfo>);

/// Instances the current filters let through, sorted.
fn visible(shell: &Shell, cx: &App) -> Vec<InstanceInfo> {
    let state = shell.state().read(cx);
    let view = &shell.library;
    let hidden = &cx.global::<crate::prefs::Prefs>().hidden_instances;
    let text = shell.library_filter.read(cx).value().to_string();
    let mut list: Vec<InstanceInfo> = state
        .instances
        .iter()
        .filter(|i| matches_filter(i, text.trim()))
        .filter(|i| view.show_hidden || !hidden.contains(&i.id))
        .filter(|i| match view.kind {
            KindFilter::All => true,
            KindFilter::Modpacks => i.is_modpack,
            KindFilter::Custom => !i.is_modpack && i.shared.is_none(),
            KindFilter::Shared => i.shared.is_some(),
        })
        .filter(|i| view.loader.is_none_or(|l| i.loader == l))
        .cloned()
        .collect();
    match view.sort {
        // The backend already lists most recently played first.
        LibrarySort::LastPlayed => {}
        LibrarySort::Name => list.sort_by_key(|a| a.name.to_lowercase()),
        LibrarySort::Played => list.sort_by_key(|i| std::cmp::Reverse(i.played_secs)),
        LibrarySort::Created => list.sort_by_key(|i| std::cmp::Reverse(i.created)),
        LibrarySort::GameVersion => list.sort_by(|a, b| {
            calcite_core::health::compare_versions(&b.game_version, &a.game_version)
        }),
        LibrarySort::Loader => list.sort_by_key(|i| loader_name(i.loader)),
    }
    list
}

/// Split into titled sections per the "Group by" choice.
fn sections(shell: &Shell, list: Vec<InstanceInfo>, cx: &App) -> Vec<Section> {
    let state = shell.state().read(cx);
    let mut out: Vec<Section> = Vec::new();
    let by = |key: String, item: InstanceInfo, out: &mut Vec<Section>| match out
        .iter_mut()
        .find(|(_, k, _)| k.as_ref() == key)
    {
        Some((_, _, members)) => members.push(item),
        None => out.push((None, key.into(), vec![item])),
    };
    match shell.library.group {
        LibraryGroup::None => out.push((None, "".into(), list)),
        LibraryGroup::Loader => {
            for i in list {
                by(loader_name(i.loader).to_string(), i, &mut out);
            }
        }
        LibraryGroup::GameVersion => {
            for i in list {
                by(format!("Minecraft {}", i.game_version), i, &mut out);
            }
            out.sort_by(|a, b| calcite_core::health::compare_versions(&b.1, &a.1));
        }
        LibraryGroup::Kind => {
            for i in list {
                let kind = if i.shared.is_some() {
                    "Shared"
                } else if i.is_modpack {
                    "Modpacks"
                } else {
                    "Your own"
                };
                by(kind.to_string(), i, &mut out);
            }
        }
        LibraryGroup::Groups => {
            for (group_id, name) in &state.groups {
                let members: Vec<_> = list
                    .iter()
                    .filter(|i| i.group_ids.contains(group_id))
                    .cloned()
                    .collect();
                if !members.is_empty() {
                    out.push((Some(group_id.clone()), name.clone(), members));
                }
            }
            let ungrouped: Vec<_> = list
                .iter()
                .filter(|i| {
                    !i.group_ids
                        .iter()
                        .any(|g| state.groups.iter().any(|(id, _)| id == g))
                })
                .cloned()
                .collect();
            if !ungrouped.is_empty() {
                let title = if out.is_empty() {
                    ""
                } else {
                    "Other instances"
                };
                out.push((None, title.into(), ungrouped));
            }
        }
    }
    out
}

/// A small dropdown under a toolbar button: a list of choices, the current
/// one checked. Closes on pick or a click elsewhere.
fn menu_panel(id: &'static str, items: Vec<MenuItem>, cx: &mut Context<Shell>) -> AnyElement {
    let p = cx.palette().clone();
    let mut panel = div()
        .id(SharedString::from(format!("{id}-menu")))
        .absolute()
        .top(px(34.0))
        .left(px(0.0))
        .min_w(px(200.0))
        .p(space(1.0))
        .rounded(px(radius::MD))
        .bg(p.overlay)
        .border_1()
        .border_color(p.border_strong)
        .shadow_lg()
        .flex()
        .flex_col()
        .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.library.menu = None;
            cx.notify();
        }));
    for (ix, (label, checked, pick)) in items.into_iter().enumerate() {
        panel = panel.child(
            div()
                .id(SharedString::from(format!("{id}-item-{ix}")))
                .flex()
                .items_center()
                .gap(space(2.0))
                .h(px(30.0))
                .px(space(2.5))
                .rounded(px(radius::SM))
                .text_size(px(type_scale::BODY))
                .text_color(if checked { p.text } else { p.text_secondary })
                .cursor_pointer()
                .hover(|s| s.bg(p.hover))
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.library.menu = None;
                    pick(this, window, cx);
                    cx.notify();
                }))
                .child(
                    div()
                        .w(px(14.0))
                        .when(checked, |d| d.child(icon("check", 14.0, p.accent))),
                )
                .child(label),
        );
    }
    gpui_kit::deferred(panel)
        .with_priority(1)
        .into_any_element()
}

fn menu_button(
    id: &'static str,
    glyph: &'static str,
    label: String,
    menu: Option<AnyElement>,
    cx: &mut Context<Shell>,
) -> AnyElement {
    div()
        .relative()
        .child(
            Button::new(id)
                .icon(glyph)
                .label(label)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.library.menu = if this.library.menu == Some(id) {
                        None
                    } else {
                        Some(id)
                    };
                    cx.notify();
                })),
        )
        .children(menu)
        .into_any_element()
}

fn chip_toggle(
    id: String,
    label: String,
    active: bool,
    on: impl Fn(&mut Shell, &mut Context<Shell>) + 'static,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let p = cx.palette().clone();
    div()
        .id(SharedString::from(id))
        .h(px(26.0))
        .px(space(2.5))
        .flex()
        .items_center()
        .rounded(px(radius::SM))
        .text_size(px(type_scale::SMALL))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .when(active, |d| d.bg(p.accent_soft).text_color(p.accent_text))
        .when(!active, |d| {
            d.text_color(p.text_secondary).hover(|s| s.bg(p.hover))
        })
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            on(this, cx);
            cx.notify();
        }))
        .child(label)
        .into_any_element()
}

pub fn render(shell: &mut Shell, window: &mut Window, cx: &mut Context<Shell>) -> AnyElement {
    let p = cx.palette().clone();
    let available = f32::from(window.viewport_size().width) - SIDEBAR_WIDTH - 2.0 * 32.0 - 12.0;
    let card_w = card_width(available);
    let compact = cx.global::<crate::prefs::Prefs>().library_compact;
    let hidden_count = {
        let hidden = &cx.global::<crate::prefs::Prefs>().hidden_instances;
        shell
            .state()
            .read(cx)
            .instances
            .iter()
            .filter(|i| hidden.contains(&i.id))
            .count()
    };

    // The "New group" name field needs the window.
    if shell.library.menu == Some("new-group") && shell.library.new_group.is_none() {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Group name"));
        input.update(cx, |s, cx| s.focus(window, cx));
        cx.subscribe(&input, |this, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                let name = input.read(cx).value().trim().to_string();
                let ids: Vec<String> = this.library.selected.iter().cloned().collect();
                if !name.is_empty() {
                    this.state()
                        .update(cx, |s, cx| s.group_into_new(name, ids, cx));
                    this.library.selecting = false;
                    this.library.selected.clear();
                }
                this.library.menu = None;
                this.library.new_group = None;
                cx.notify();
            }
        })
        .detach();
        shell.library.new_group = Some(input);
    }

    let instances = visible(shell, cx);
    let state = shell.state().read(cx);
    let this_signed_in = state.modrinth.is_some();
    let total = state.instances.len();
    let running = state.running.clone();
    let jobs: Vec<Job> = state.jobs.clone();
    let groups = state.groups.clone();
    let loaders: Vec<ModLoader> = {
        let mut seen = Vec::new();
        for i in &state.instances {
            if !seen.contains(&i.loader) {
                seen.push(i.loader);
            }
        }
        seen
    };
    let shown_ids: Vec<String> = instances.iter().map(|i| i.id.clone()).collect();
    let sections = sections(shell, instances, cx);

    let sort_menu = (shell.library.menu == Some("sort")).then(|| {
        let current = shell.library.sort;
        let items = LibrarySort::ALL
            .iter()
            .map(|s| {
                let s = *s;
                let pick: MenuAction = Rc::new(move |this, _, _| this.library.sort = s);
                (SharedString::from(s.label()), s == current, pick)
            })
            .collect();
        menu_panel("sort", items, cx)
    });
    let group_menu = (shell.library.menu == Some("group")).then(|| {
        let current = shell.library.group;
        let items = LibraryGroup::ALL
            .iter()
            .map(|g| {
                let g = *g;
                let pick: MenuAction = Rc::new(move |this, _, _| this.library.group = g);
                (SharedString::from(g.label()), g == current, pick)
            })
            .collect();
        menu_panel("group", items, cx)
    });

    let toolbar = div()
        .flex()
        .items_center()
        .gap(space(2.0))
        .child(
            div().w(px(240.0)).child(
                Input::new(&shell.library_filter)
                    .prefix(icon("search", 14.0, p.text_muted))
                    .cleanable(true),
            ),
        )
        .child(menu_button(
            "sort",
            "arrow-up-down",
            shell.library.sort.label().to_string(),
            sort_menu,
            cx,
        ))
        .child(menu_button(
            "group",
            "layers",
            format!("Group: {}", shell.library.group.label()),
            group_menu,
            cx,
        ))
        .child(
            Button::new("library-view")
                .icon(if compact { "layout-grid" } else { "list" })
                .ghost()
                .on_click(move |_, _, cx| crate::prefs::Prefs::set_compact(!compact, cx)),
        )
        .child(
            Button::new("library-select")
                .icon("check")
                .label(if shell.library.selecting {
                    "Done"
                } else {
                    "Select"
                })
                .ghost()
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.library.selecting = !this.library.selecting;
                    this.library.selected.clear();
                    this.library.confirm_delete = false;
                    cx.notify();
                })),
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

    // Filter chips: kind, then the loaders you actually have, then hidden.
    let view_kind = shell.library.kind;
    let view_loader = shell.library.loader;
    let mut filters = div().flex().flex_wrap().items_center().gap(space(1.0));
    for (kind, label) in [
        (KindFilter::All, "All"),
        (KindFilter::Modpacks, "Modpacks"),
        (KindFilter::Custom, "Your own"),
        (KindFilter::Shared, "Shared"),
    ] {
        filters = filters.child(chip_toggle(
            format!("kind-{label}"),
            label.into(),
            view_kind == kind,
            move |this, _| this.library.kind = kind,
            cx,
        ));
    }
    if loaders.len() > 1 {
        filters = filters.child(div().w(px(1.0)).h(px(16.0)).mx(space(1.5)).bg(p.border));
        for loader in loaders {
            let active = view_loader == Some(loader);
            filters = filters.child(chip_toggle(
                format!("loader-{}", loader_name(loader)),
                loader_name(loader).into(),
                active,
                move |this, _| this.library.loader = if active { None } else { Some(loader) },
                cx,
            ));
        }
    }
    if hidden_count > 0 {
        let show = shell.library.show_hidden;
        filters = filters
            .child(div().w(px(1.0)).h(px(16.0)).mx(space(1.5)).bg(p.border))
            .child(chip_toggle(
                "hidden".into(),
                format!("{hidden_count} hidden"),
                show,
                move |this, _| this.library.show_hidden = !show,
                cx,
            ));
    }

    let selecting = shell.library.selecting;
    let selected = shell.library.selected.clone();
    let hidden = cx.global::<crate::prefs::Prefs>().hidden_instances.clone();
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
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.open_create_dialog(window, cx))),
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
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.navigate(Route::Discover, window, cx))),
                    )
                    .into_any_element(),
            ),
            cx,
        )
        .into_any_element()
    } else if sections.iter().all(|(_, _, m)| m.is_empty()) {
        empty_state(
            "search",
            "No instances match",
            "Try a different name, filter or loader.",
            None,
            cx,
        )
        .into_any_element()
    } else {
        let mut list = div().flex().flex_col().gap(space(8.0));
        for (group_id, title, members) in sections {
            let count = members.len();
            let mut grid = if compact {
                div()
                    .flex()
                    .flex_col()
                    .rounded(px(radius::LG))
                    .border_1()
                    .border_color(p.border)
                    .overflow_hidden()
            } else {
                div().flex().flex_wrap().gap(space(3.0))
            };
            for (ix, inst) in members.into_iter().enumerate() {
                let run = running
                    .get(&inst.id)
                    .map(|r| (Utc::now() - r.started).num_seconds().max(0) as u64);
                let job = jobs
                    .iter()
                    .find(|j| j.is_active() && j.instance_id.as_deref() == Some(&inst.id))
                    .cloned();
                let pick = Pick {
                    selecting,
                    selected: selected.contains(&inst.id),
                    hidden: hidden.contains(&inst.id),
                };
                grid = grid.child(if compact {
                    instance_row(&inst, run, job, pick, ix, cx).into_any_element()
                } else {
                    instance_card(&inst, run, job, card_w, pick, cx).into_any_element()
                });
            }
            let removable_group = group_id.clone();
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
                                )
                                .when_some(
                                    removable_group
                                        .filter(|g| g != theseus::instance::FAVORITES_GROUP_ID),
                                    |d, gid| {
                                        d.child(
                                            Button::new(SharedString::from(format!(
                                                "group-delete-{gid}"
                                            )))
                                            .icon("x")
                                            .ghost()
                                            .small()
                                            .on_click(cx.listener(
                                                move |this, _: &ClickEvent, _, cx| {
                                                    let gid = gid.clone();
                                                    this.state().update(cx, |s, cx| {
                                                        s.delete_group(gid, cx)
                                                    });
                                                },
                                            )),
                                        )
                                    },
                                ),
                        )
                    })
                    .child(grid),
            );
        }
        list.into_any_element()
    };

    // Selection bar: what to do with the picked instances.
    let selection_bar = selecting.then(|| {
        let n = selected.len();
        let ids: Vec<String> = selected.iter().cloned().collect();
        let add_menu = (shell.library.menu == Some("add-to-group")).then(|| {
            let mut items: Vec<MenuItem> = groups
                .iter()
                .map(|(gid, name)| {
                    let (gid, ids) = (gid.clone(), ids.clone());
                    let pick: MenuAction = Rc::new(move |this, _, cx| {
                        let (gid, ids) = (gid.clone(), ids.clone());
                        this.state()
                            .update(cx, |s, cx| s.regroup(ids, Some(gid), None, cx));
                        this.library.selecting = false;
                        this.library.selected.clear();
                    });
                    (name.clone(), false, pick)
                })
                .collect();
            let new_pick: MenuAction = Rc::new(|this, _, _| {
                this.library.menu = Some("new-group");
            });
            items.push(("New group…".into(), false, new_pick));
            menu_panel("add-to-group", items, cx)
        });
        let hide_ids = ids.clone();
        let all_hidden = n > 0 && ids.iter().all(|i| hidden.contains(i));
        let delete_ids = ids.clone();
        let fav_ids = ids.clone();
        div()
            .flex()
            .items_center()
            .gap(space(2.0))
            .mx(space(8.0))
            .mb(space(4.0))
            .px(space(3.0))
            .py(space(2.5))
            .rounded(px(radius::LG))
            .bg(p.overlay)
            .border_1()
            .border_color(p.border_strong)
            .shadow_lg()
            .child(
                div()
                    .text_size(px(type_scale::BODY))
                    .font_weight(FontWeight::MEDIUM)
                    .child(format!("{n} selected")),
            )
            .child(
                Button::new("select-all")
                    .label("Select all")
                    .ghost()
                    .small()
                    .on_click({
                        let all = shown_ids.clone();
                        cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.library.selected = all.iter().cloned().collect();
                            cx.notify();
                        })
                    }),
            )
            .child(div().flex_1())
            .when_some(
                shell
                    .library
                    .new_group
                    .clone()
                    .filter(|_| shell.library.menu == Some("new-group")),
                |d, input| d.child(div().w(px(180.0)).child(Input::new(&input).small())),
            )
            .when(n > 0 && !shell.library.confirm_delete, |d| {
                d.child(menu_button(
                    "add-to-group",
                    "layers",
                    "Add to group".into(),
                    add_menu,
                    cx,
                ))
                .child(
                    Button::new("bulk-favorite")
                        .icon("star")
                        .label("Favorite")
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            let ids = fav_ids.clone();
                            this.state().update(cx, |s, cx| {
                                s.regroup(
                                    ids,
                                    Some(theseus::instance::FAVORITES_GROUP_ID.to_string()),
                                    None,
                                    cx,
                                )
                            });
                        })),
                )
                .child(
                    Button::new("bulk-hide")
                        .icon(if all_hidden { "eye" } else { "eye-off" })
                        .label(if all_hidden { "Show" } else { "Hide" })
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            crate::prefs::Prefs::set_hidden(&hide_ids, !all_hidden, cx);
                            this.library.selected.clear();
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("bulk-delete")
                        .icon("trash")
                        .label("Delete")
                        .danger()
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.library.confirm_delete = true;
                            cx.notify();
                        })),
                )
            })
            .when(shell.library.confirm_delete, |d| {
                d.child(
                    div()
                        .text_size(px(type_scale::SMALL))
                        .text_color(p.danger)
                        .child(format!(
                            "Delete {n} instance{} and their worlds? This can't be undone.",
                            if n == 1 { "" } else { "s" }
                        )),
                )
                .child(
                    Button::new("bulk-delete-yes")
                        .label("Delete")
                        .danger()
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            let ids = delete_ids.clone();
                            this.state().update(cx, |s, cx| s.delete_instances(ids, cx));
                            this.library.confirm_delete = false;
                            this.library.selecting = false;
                            this.library.selected.clear();
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("bulk-delete-no")
                        .label("Keep")
                        .ghost()
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.library.confirm_delete = false;
                            cx.notify();
                        })),
                )
            })
    });

    div()
        .size_full()
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .flex_col()
                .gap(space(4.0))
                .px(space(8.0))
                .pt(space(7.0))
                .pb(space(5.0))
                .child(page_header(
                    "Library",
                    (total > 0)
                        .then(|| format!("{total} instance{}", if total == 1 { "" } else { "s" })),
                    cx,
                ))
                .when(total > 0, |d| d.child(toolbar).child(filters)),
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
        .children(selection_bar)
        .into_any_element()
}

/// How a card should look in selection mode.
#[derive(Clone, Copy)]
struct Pick {
    selecting: bool,
    selected: bool,
    hidden: bool,
}

/// Right-click menu for an instance.
fn instance_menu(
    inst: &InstanceInfo,
    shell: gpui_kit::WeakEntity<Shell>,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    let id = inst.id.clone();
    move |menu, _window, cx| {
        let hidden = cx
            .global::<crate::prefs::Prefs>()
            .hidden_instances
            .contains(&id);
        let state = crate::state::AppState::global(cx);
        let groups = state.read(cx).groups.clone();
        let in_groups = state
            .read(cx)
            .instance(&id)
            .map(|i| i.group_ids.clone())
            .unwrap_or_default();
        let (play, open, settings, hide, fav, delete, folder) = (
            id.clone(),
            id.clone(),
            id.clone(),
            id.clone(),
            id.clone(),
            id.clone(),
            id.clone(),
        );
        let fav_on = in_groups
            .iter()
            .any(|g| g == theseus::instance::FAVORITES_GROUP_ID);
        let (s1, s2, s3, s4) = (shell.clone(), shell.clone(), shell.clone(), shell.clone());
        let mut menu = menu
            .item(PopupMenuItem::new("Play").on_click(move |_, _, cx| {
                let id = play.clone();
                crate::state::AppState::global(cx).update(cx, |s, cx| s.play(&id, cx));
            }))
            .item(PopupMenuItem::new("Open").on_click(move |_, window, cx| {
                let id = open.clone();
                let _ = s1.update(cx, |sh, cx| sh.navigate(Route::Instance(id), window, cx));
            }))
            .item(
                PopupMenuItem::new("Settings").on_click(move |_, window, cx| {
                    let id = settings.clone();
                    let _ = s2.update(cx, |sh, cx| sh.open_instance_settings(id, window, cx));
                }),
            )
            .item(PopupMenuItem::new("Open folder").on_click(move |_, _, cx| {
                let id = folder.clone();
                let task = crate::backend::Backend::global(cx)
                    .call(async move { theseus::instance::get_full_path(&id).await });
                cx.spawn(async move |cx| {
                    if let Ok(path) = task.await {
                        cx.update(|cx| cx.open_with_system(&path));
                    }
                })
                .detach();
            }))
            .separator()
            .item(
                PopupMenuItem::new(if fav_on {
                    "Remove from Favorites"
                } else {
                    "Add to Favorites"
                })
                .on_click(move |_, _, cx| {
                    let id = fav.clone();
                    let fav_group = theseus::instance::FAVORITES_GROUP_ID.to_string();
                    crate::state::AppState::global(cx).update(cx, |s, cx| {
                        if fav_on {
                            s.regroup(vec![id], None, Some(fav_group), cx)
                        } else {
                            s.regroup(vec![id], Some(fav_group), None, cx)
                        }
                    });
                }),
            );
        for (gid, name) in groups
            .into_iter()
            .filter(|(g, _)| g != theseus::instance::FAVORITES_GROUP_ID)
        {
            let member = in_groups.contains(&gid);
            let id = id.clone();
            menu = menu.item(
                PopupMenuItem::new(format!("Group: {name}"))
                    .checked(member)
                    .on_click(move |_, _, cx| {
                        let (id, gid) = (id.clone(), gid.clone());
                        crate::state::AppState::global(cx).update(cx, |s, cx| {
                            if member {
                                s.regroup(vec![id], None, Some(gid), cx)
                            } else {
                                s.regroup(vec![id], Some(gid), None, cx)
                            }
                        });
                    }),
            );
        }
        menu.separator()
            .item(
                PopupMenuItem::new(if hidden {
                    "Show in Library"
                } else {
                    "Hide from Library"
                })
                .on_click(move |_, _, cx| {
                    crate::prefs::Prefs::set_hidden(std::slice::from_ref(&hide), !hidden, cx);
                    let _ = s3.update(cx, |_, cx| cx.notify());
                }),
            )
            .item(
                PopupMenuItem::new("Delete…").on_click(move |_, window, cx| {
                    // Deleting goes through settings, which asks first.
                    let id = delete.clone();
                    let _ = s4.update(cx, |sh, cx| sh.open_instance_settings(id, window, cx));
                }),
            )
    }
}

/// A one-line row for the compact view.
fn instance_row(
    inst: &InstanceInfo,
    running_secs: Option<u64>,
    job: Option<Job>,
    pick: Pick,
    ix: usize,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let p = cx.palette().clone();
    let id = inst.id.clone();
    let play_id = inst.id.clone();
    let installing = job.is_some() || !inst.installed;
    let menu = instance_menu(inst, cx.entity().downgrade());
    div()
        .id(SharedString::from(format!("row-{}", inst.id)))
        .flex()
        .items_center()
        .gap(space(3.0))
        .h(px(48.0))
        .px(space(3.5))
        .when(ix > 0, |d| d.border_t_1().border_color(p.border))
        .when(pick.hidden, |d| d.opacity(0.5))
        .when(pick.selected, |d| d.bg(p.accent_soft))
        .cursor_pointer()
        .hover(|s| s.bg(p.hover))
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            if pick.selecting {
                if !this.library.selected.remove(&id) {
                    this.library.selected.insert(id.clone());
                }
                cx.notify();
            } else {
                this.navigate(Route::Instance(id.clone()), window, cx)
            }
        }))
        .when(pick.selecting, |d| d.child(checkbox(pick.selected, cx)))
        .child(thumb(instance_icon(inst), &inst.name, 28.0, radius::SM, cx))
        .child(
            div()
                .w(px(260.0))
                .truncate()
                .text_size(px(type_scale::BODY))
                .font_weight(FontWeight::MEDIUM)
                .text_color(p.text)
                .child(inst.name.clone()),
        )
        .child(chip(inst.loader_line(), cx))
        .when(inst.is_modpack, |d| d.child(chip("Modpack", cx)))
        .child(div().flex_1())
        .child(match (running_secs, &job) {
            (Some(secs), _) => running_chip(secs, cx).into_any_element(),
            (None, Some(job)) => div()
                .w(px(140.0))
                .child(progress_bar(job.fraction(), cx))
                .into_any_element(),
            _ => div()
                .text_size(px(type_scale::SMALL))
                .text_color(p.text_muted)
                .child(if inst.played_secs > 0 {
                    short_duration(inst.played_secs)
                } else {
                    "Not played yet".into()
                })
                .into_any_element(),
        })
        .child(
            div()
                .w(px(110.0))
                .text_right()
                .text_size(px(type_scale::SMALL))
                .text_color(p.text_muted)
                .child(inst.last_played.map(relative_time).unwrap_or_default()),
        )
        .when(!pick.selecting && running_secs.is_none(), |d| {
            d.child(
                Button::new(SharedString::from(format!("row-play-{ix}")))
                    .icon("play")
                    .ghost()
                    .small()
                    .disabled(installing)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        let id = play_id.clone();
                        this.state().update(cx, |s, cx| s.play(&id, cx));
                    })),
            )
        })
        .context_menu(menu)
}

pub(crate) fn checkbox(on: bool, cx: &App) -> gpui_kit::Div {
    let p = cx.palette();
    div()
        .size(px(16.0))
        .flex_none()
        .rounded(px(4.0))
        .border_1()
        .border_color(if on { p.accent } else { p.border_strong })
        .bg(if on { p.accent } else { p.surface })
        .flex()
        .items_center()
        .justify_center()
        .when(on, |d| d.child(icon("check", 12.0, p.on_accent)))
}

fn instance_card(
    inst: &InstanceInfo,
    running_secs: Option<u64>,
    job: Option<Job>,
    width: f32,
    pick: Pick,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let p = cx.palette().clone();
    let menu = instance_menu(inst, cx.entity().downgrade());
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
        .border_color(if pick.selected {
            p.accent
        } else if running_secs.is_some() {
            p.running_soft
        } else {
            p.border
        })
        .when(pick.selected, |d| d.bg(p.accent_soft))
        .when(pick.hidden, |d| d.opacity(0.5))
        .cursor_pointer()
        .hover(|s| s.border_color(p.border_strong))
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            if pick.selecting {
                if !this.library.selected.remove(&id) {
                    this.library.selected.insert(id.clone());
                }
                cx.notify();
            } else {
                this.navigate(Route::Instance(id.clone()), window, cx)
            }
        }))
        .context_menu(menu)
        .child(
            div()
                .flex()
                .gap(space(3.0))
                .p(space(3.5))
                .when(pick.selecting, |d| d.child(checkbox(pick.selected, cx)))
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
