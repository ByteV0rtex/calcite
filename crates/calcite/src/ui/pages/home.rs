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
    let state = shell.state().read(cx);
    let instances = state.instances.clone();
    let running = state.running.clone();
    let accounts = state.accounts.len();

    if instances.is_empty() {
        return div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
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
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(space(3.0))
                        .child(section_heading("Jump back in", cx))
                        .child(jump),
                )
                .when(!recent.is_empty(), |d| {
                    d.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(space(3.0))
                            .child(section_heading("Worlds and servers", cx))
                            .child(worlds_row),
                    )
                })
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
                ),
        )
        .into_any_element()
}
