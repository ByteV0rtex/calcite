//! Home: get back into the game fast.

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, ClickEvent, Context, FontWeight, SharedString, Window, div, px};

use crate::images::Images;
use crate::theme::{PaletteExt, radius, space, type_scale};
use crate::ui::components::{Button, chip, icon, section_heading, short_duration, thumb};
use crate::ui::sample;
use crate::ui::shell::{Route, Shell};

pub fn render(_shell: &mut Shell, _window: &mut Window, cx: &mut Context<Shell>) -> AnyElement {
    let p = cx.palette().clone();

    let mut jump = div().flex().gap(space(3.0));
    for inst in sample::INSTANCES.iter().take(3) {
        let source = inst.icon.and_then(|url| Images::get(url, 96, cx));
        let id = inst.id;
        jump = jump.child(
            div()
                .id(SharedString::from(format!("jump-{id}")))
                .flex_1()
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
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.navigate(Route::Instance(id), cx)
                }))
                .child(thumb(source, inst.name, 44.0, radius::MD, cx))
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
                                .child(inst.name),
                        )
                        .child(
                            div()
                                .text_size(px(type_scale::SMALL))
                                .text_color(p.text_muted)
                                .child(format!(
                                    "{} {} · {}",
                                    inst.loader, inst.game_version, inst.last_played
                                )),
                        ),
                )
                .child(
                    Button::new(SharedString::from(format!("jump-play-{id}")))
                        .icon("play")
                        .primary(),
                ),
        );
    }

    let worlds: &[(&str, &str, &str)] = &[
        (
            "New Horizons",
            "Survival · Fabulously Optimized",
            "2 hours ago",
        ),
        ("Brass Factory", "Creative · Create Survival", "Now"),
        ("play.example.net", "Server · PvP Practice", "3 days ago"),
    ];
    let mut world_list = crate::ui::components::panel(cx).overflow_hidden();
    for (ix, (name, detail, when)) in worlds.iter().enumerate() {
        let is_server = detail.starts_with("Server");
        world_list = world_list.child(
            div()
                .id(SharedString::from(format!("world-{ix}")))
                .flex()
                .items_center()
                .gap(space(3.0))
                .h(px(52.0))
                .px(space(3.5))
                .when(ix > 0, |d| d.border_t_1().border_color(p.border))
                .hover(|s| s.bg(p.hover))
                .cursor_pointer()
                .child(
                    div()
                        .size(px(32.0))
                        .rounded(px(radius::SM))
                        .bg(p.raised)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon(
                            if is_server { "server" } else { "globe" },
                            16.0,
                            p.text_muted,
                        )),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .child(
                            div()
                                .text_size(px(type_scale::BODY))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.text)
                                .child(*name),
                        )
                        .child(
                            div()
                                .text_size(px(type_scale::SMALL))
                                .text_color(p.text_muted)
                                .child(*detail),
                        ),
                )
                .child(
                    div()
                        .text_size(px(type_scale::SMALL))
                        .text_color(p.text_muted)
                        .child(*when),
                )
                .child(
                    Button::new(SharedString::from(format!("world-join-{ix}")))
                        .label("Join")
                        .small(),
                ),
        );
    }

    let total: u64 = sample::INSTANCES.iter().map(|i| i.played_secs).sum();
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
                .child(
                    div()
                        .flex()
                        .gap(space(3.0))
                        .child(stat("Played this week", short_duration(9 * 3600 + 40 * 60)))
                        .child(stat("All-time playtime", short_duration(total)))
                        .child(stat("Updates waiting", "3".into()))
                        .child(stat("Disk saved by sharing files", "4.2 GB".into())),
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
                                .child(section_heading("Continue in a world", cx))
                                .child(chip("Quick Play", cx)),
                        )
                        .child(world_list),
                ),
        )
        .into_any_element()
}
