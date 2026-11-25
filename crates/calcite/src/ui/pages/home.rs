//! Home: get back into the game fast.

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, ClickEvent, Context, FontWeight, SharedString, Window, div, px};

use crate::theme::{PaletteExt, radius, space, type_scale};
use crate::ui::components::{
    Button, empty_state, relative_time, section_heading, short_duration, thumb,
};
use crate::ui::shell::{Route, Shell, instance_icon};

pub fn render(shell: &mut Shell, window: &mut Window, cx: &mut Context<Shell>) -> AnyElement {
    let p = cx.palette().clone();
    // 9/IB SMP first: join from here without switching to 9/IB mode.
    let ibsmp_card = crate::ui::pages::ibsmp::home_card(shell, cx);
    let state = shell.state().read(cx);
    let instances = state.instances.clone();
    let running = state.running.clone();
    let accounts = state.accounts.len();

    if instances.is_empty() {
        let popular = popular_section(shell, cx);
        return div()
            .id("home-scroll")
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(space(8.0))
            .px(space(8.0))
            .py(space(7.0))
            .child(div().w_full().max_w(px(1120.0)).child(ibsmp_card))
            .child(empty_state(
                "house",
                "Welcome to Calcite",
                if accounts == 0 {
                    "Bring your instances over from Modrinth App or another launcher, install a modpack, or make your own instance."
                } else {
                    "Install a modpack from Discover or make your own instance to get started."
                },
                Some(
                    div()
                        .flex()
                        .gap(space(2.0))
                        .child(
                            Button::new("home-discover")
                                .icon("compass")
                                .label("Browse modpacks")
                                .primary()
                                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                    this.navigate(Route::Discover, window, cx)
                                })),
                        )
                        .child(
                            Button::new("home-move-in")
                                .icon("arrow-down-to-line")
                                .label("Move in from another launcher")
                                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                    this.open_settings(crate::ui::pages::settings::Section::MoveIn, window, cx)
                                })),
                        )
                        .child(
                            Button::new("home-new")
                                .icon("plus")
                                .label("New instance")
                                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                    this.open_create_dialog(window, cx)
                                })),
                        )
                        .into_any_element(),
                ),
                cx,
            ))
            .children(popular.map(|p| div().w_full().max_w(px(1120.0)).child(p)))
            .into_any_element();
    }

    let mut jump = div().flex().gap(space(3.0));
    for inst in instances.iter().take(3) {
        let source = instance_icon(inst);
        let id = inst.id.clone();
        let play_id = inst.id.clone();
        let is_running = running.contains_key(&inst.id);
        let detail = match inst.last_played {
            Some(t) => format!("{} · {}", inst.loader_line(), relative_time(t)),
            None => inst.loader_line(),
        };
        jump = jump.child(
            div()
                .id(SharedString::from(format!("jump-{id}")))
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap(space(3.0))
                .p(space(3.0))
                .rounded(px(radius::LG))
                .bg(p.surface)
                .border_1()
                .border_color(p.border)
                .cursor_pointer()
                .hover(|s| s.border_color(p.border_strong))
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.navigate(Route::Instance(id.clone()), window, cx)
                }))
                .child(thumb(source, &inst.name, 44.0, radius::MD, cx))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(space(1.0))
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .truncate()
                                .text_size(px(type_scale::BODY))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(p.text)
                                .child(inst.name.clone()),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(px(type_scale::SMALL))
                                .text_color(p.text_muted)
                                .child(detail),
                        ),
                )
                .child(if is_running {
                    Button::new(SharedString::from(format!("jump-stop-{play_id}")))
                        .icon("square")
                        .danger()
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            cx.stop_propagation();
                            let id = play_id.clone();
                            this.state().update(cx, |s, cx| s.stop(&id, cx));
                        }))
                } else {
                    Button::new(SharedString::from(format!("jump-play-{play_id}")))
                        .icon("play")
                        .primary()
                        .disabled(!inst.installed)
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            cx.stop_propagation();
                            let id = play_id.clone();
                            this.state().update(cx, |s, cx| s.play(&id, cx));
                        }))
                }),
        );
    }

    // Worlds and servers you played lately: one click puts you back in.
    let recent = shell.state().read(cx).recent_worlds.clone();
    // Three per row, filling the page like the instance cards above.
    let page_w = (f32::from(window.viewport_size().width) - crate::ui::shell::SIDEBAR_WIDTH - 64.0)
        .min(1120.0 - 64.0);
    let world_w = ((page_w - 2.0 * 12.0) / 3.0).floor().max(220.0);
    let mut worlds_row = div().flex().flex_wrap().gap(space(3.0));
    for (ix, w) in recent.iter().take(6).enumerate() {
        let inst_name = instances
            .iter()
            .find(|i| i.id == w.instance_id)
            .map(|i| i.name.clone())
            .unwrap_or_default();
        let icon_src = w.icon.as_deref().and_then(|i| {
            if i.starts_with("http") || i.starts_with("data:") {
                crate::images::Images::get(i, 96, cx)
            } else {
                crate::images::Images::get_file(std::path::Path::new(i), 96, cx)
            }
        });
        let (id, key, server) = (w.instance_id.clone(), w.key.clone(), w.server);
        worlds_row = worlds_row.child(
            div()
                .id(SharedString::from(format!("recent-world-{ix}")))
                .w(px(world_w))
                .flex()
                .items_center()
                .gap(space(3.0))
                .p(space(3.0))
                .rounded(px(radius::LG))
                .bg(p.surface)
                .border_1()
                .border_color(p.border)
                .child(match icon_src {
                    Some(src) => gpui_kit::img(src)
                        .size(px(40.0))
                        .rounded(px(radius::SM))
                        .into_any_element(),
                    None => div()
                        .size(px(40.0))
                        .flex_none()
                        .rounded(px(radius::SM))
                        .bg(p.raised)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(crate::ui::components::icon(
                            if server { "server" } else { "globe" },
                            18.0,
                            p.text_muted,
                        ))
                        .into_any_element(),
                })
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
                                .child(w.name.clone()),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(px(type_scale::SMALL))
                                .text_color(p.text_muted)
                                .child(format!(
                                    "{} · {}",
                                    inst_name,
                                    w.last_played.map(relative_time).unwrap_or_default()
                                )),
                        ),
                )
                .child(
                    Button::new(SharedString::from(format!("recent-world-play-{ix}")))
                        .icon("play")
                        .ghost()
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            let (id, key) = (id.clone(), key.clone());
                            this.state().update(cx, |s, cx| {
                                if server {
                                    s.play_server(&id, key, cx)
                                } else {
                                    s.play_world(&id, key, cx)
                                }
                            });
                        })),
                ),
        );
    }

    let total: u64 = instances.iter().map(|i| i.played_secs).sum();
    let modded = instances
        .iter()
        .filter(|i| i.loader != theseus::data::ModLoader::Vanilla)
        .count();
    let stat = |label: &'static str, value: String| {
        div()
            .flex()
            .flex_col()
            .gap(space(1.0))
            .flex_1()
            .p(space(3.5))
            .rounded(px(radius::LG))
            .bg(p.surface)
            .border_1()
            .border_color(p.border)
            .child(
                div()
                    .text_size(px(type_scale::SMALL))
                    .text_color(p.text_muted)
                    .child(label),
            )
            .child(
                div()
                    .text_size(px(type_scale::TITLE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(p.text)
                    .child(value),
            )
    };

    div()
        .id("home-scroll")
        .size_full()
        .overflow_y_scroll()
        .child(
            div()
                .flex()
                .flex_col()
                .gap(space(8.0))
                .px(space(8.0))
                .py(space(7.0))
                .max_w(px(1120.0))
                .child(ibsmp_card)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(space(3.0))
                        .child(section_heading("Jump back in", cx))
                        .child(jump),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(space(3.0))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(section_heading("Worlds and servers", cx))
                                .child(
                                    Button::new("home-find-server")
                                        .icon("server")
                                        .label("Find a server")
                                        .small()
                                        .ghost()
                                        .on_click(cx.listener(
                                            |this, _: &ClickEvent, window, cx| {
                                                this.open_servers(window, cx)
                                            },
                                        )),
                                ),
                        )
                        .when(!recent.is_empty(), |d| d.child(worlds_row)),
                )
                .child(
                    div()
                        .flex()
                        .gap(space(3.0))
                        .child(stat(
                            "Total playtime",
                            if total > 0 {
                                short_duration(total)
                            } else {
                                "—".into()
                            },
                        ))
                        .child(stat("Instances", instances.len().to_string()))
                        .child(stat("Modded", modded.to_string()))
                        .child(stat("Running now", running.len().to_string())),
                )
                .children(popular_section(shell, cx)),
        )
        .into_any_element()
}

/// The most downloaded modpacks on Modrinth, four across.
fn popular_section(shell: &mut Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let p = cx.palette().clone();
    let hits = shell.state().read(cx).popular.clone()?;
    if hits.is_empty() {
        return None;
    }
    let mut row = div().flex().gap(space(3.0));
    for (ix, hit) in hits.into_iter().take(4).enumerate() {
        let source = hit
            .icon_url
            .as_deref()
            .and_then(|url| crate::images::Images::get(url, 96, cx));
        let project = hit.project_id.clone();
        let install = hit.latest_version.clone().map(|version| {
            (
                hit.project_id.clone(),
                version,
                hit.title.clone(),
                hit.icon_url.clone(),
            )
        });
        row = row.child(
            div()
                .id(SharedString::from(format!("popular-{ix}")))
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(space(3.0))
                .p(space(4.0))
                .rounded(px(radius::LG))
                .bg(p.surface)
                .border_1()
                .border_color(p.border)
                .cursor_pointer()
                .hover(|s| s.border_color(p.border_strong))
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.navigate(Route::Project(project.clone(), None), window, cx)
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space(3.0))
                        .child(thumb(source, &hit.title, 44.0, radius::MD, cx))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .min_w_0()
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(type_scale::BODY))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(p.text)
                                        .child(hit.title.clone()),
                                )
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(type_scale::SMALL))
                                        .text_color(p.text_muted)
                                        .child(format!("by {}", hit.author)),
                                ),
                        ),
                )
                .child(
                    div()
                        .h(px(34.0))
                        .overflow_hidden()
                        .text_size(px(type_scale::SMALL))
                        .line_height(px(17.0))
                        .text_color(p.text_secondary)
                        .child(hit.description.clone()),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space(2.0))
                        .child(crate::ui::components::icon("download", 13.0, p.text_muted))
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(type_scale::SMALL))
                                .text_color(p.text_muted)
                                .child(crate::ui::components::compact_number(hit.downloads)),
                        )
                        .children(install.map(|(project, version, title, icon_url)| {
                            Button::new(SharedString::from(format!("popular-install-{ix}")))
                                .icon("arrow-down-to-line")
                                .label("Install")
                                .small()
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    cx.stop_propagation();
                                    let (project, version, title, icon_url) = (
                                        project.clone(),
                                        version.clone(),
                                        title.clone(),
                                        icon_url.clone(),
                                    );
                                    this.state().update(cx, |s, cx| {
                                        s.install_modpack(project, version, title, icon_url, cx)
                                    });
                                }))
                        })),
                ),
        );
    }
    Some(
        div()
            .flex()
            .flex_col()
            .gap(space(3.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(section_heading("Popular modpacks", cx))
                    .child(
                        Button::new("home-popular-more")
                            .label("See all")
                            .small()
                            .ghost()
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.navigate(Route::Discover, window, cx)
                            })),
                    ),
            )
            .child(row)
            .into_any_element(),
    )
}
