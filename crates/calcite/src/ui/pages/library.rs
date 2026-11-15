//! Library: every instance, grouped, with Play one click away.

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, ClickEvent, Context, FontWeight, SharedString, Window, div, px};

use crate::images::Images;
use crate::theme::{PaletteExt, radius, space, type_scale};
use crate::ui::components::{Button, chip, icon, short_duration, state_chip, thumb};
use crate::ui::pages::page_header;
use crate::ui::sample::{self, SampleInstance};
use crate::ui::shell::{Route, SIDEBAR_WIDTH, Shell, running_chip};

/// Card width that fills the row exactly, given the space the page has.
fn card_width(available: f32) -> f32 {
    const MIN: f32 = 248.0;
    const GAP: f32 = 12.0;
    let cols = ((available + GAP) / (MIN + GAP)).floor().max(1.0);
    ((available - GAP * (cols - 1.0)) / cols).floor()
}

pub fn render(_shell: &mut Shell, window: &mut Window, cx: &mut Context<Shell>) -> AnyElement {
    let p = cx.palette().clone();
    let available = f32::from(window.viewport_size().width) - SIDEBAR_WIDTH - 2.0 * 32.0 - 12.0;
    let card_w = card_width(available);

    let mut groups: Vec<(&str, Vec<&SampleInstance>)> = Vec::new();
    for inst in sample::INSTANCES {
        match groups.iter_mut().find(|(g, _)| *g == inst.group) {
            Some((_, list)) => list.push(inst),
            None => groups.push((inst.group, vec![inst])),
        }
    }

    let toolbar = div()
        .flex()
        .items_center()
        .gap(space(2.0))
        .child(
            div()
                .flex()
                .items_center()
                .gap(space(2.0))
                .h(px(32.0))
                .w(px(240.0))
                .px(space(2.5))
                .rounded(px(radius::MD))
                .bg(p.surface)
                .border_1()
                .border_color(p.border)
                .child(icon("search", 14.0, p.text_muted))
                .child(
                    div()
                        .text_size(px(type_scale::BODY))
                        .text_color(p.text_muted)
                        .child("Filter instances"),
                ),
        )
        .child(
            Button::new("sort")
                .icon("arrow-up-down")
                .label("Last played"),
        )
        .child(Button::new("view-grid").icon("layout-grid").ghost())
        .child(Button::new("view-list").icon("list").ghost())
        .child(div().flex_1())
        .child(
            Button::new("import")
                .icon("arrow-down-to-line")
                .label("Import"),
        )
        .child(
            Button::new("new-instance")
                .icon("plus")
                .label("New instance")
                .primary(),
        );

    let mut body = div()
        .id("library-scroll")
        .flex()
        .flex_col()
        .gap(space(8.0))
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .px(space(8.0))
        .pb(space(10.0));

    for (group, instances) in groups {
        let mut grid = div().flex().flex_wrap().gap(space(3.0));
        for inst in instances {
            grid = grid.child(instance_card(inst, card_w, cx));
        }
        body = body.child(
            div()
                .flex()
                .flex_col()
                .gap(space(3.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space(2.0))
                        .child(
                            div()
                                .text_size(px(type_scale::BODY_LG))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(p.text)
                                .child(group),
                        )
                        .child(
                            div()
                                .text_size(px(type_scale::SMALL))
                                .text_color(p.text_muted)
                                .child(format!(
                                    "{}",
                                    sample::INSTANCES
                                        .iter()
                                        .filter(|i| i.group == group)
                                        .count()
                                )),
                        ),
                )
                .child(grid),
        );
    }

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
                    Some(format!("{} instances", sample::INSTANCES.len())),
                    cx,
                ))
                .child(toolbar),
        )
        .child(body)
        .into_any_element()
}

fn instance_card(
    inst: &'static SampleInstance,
    width: f32,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let p = cx.palette().clone();
    let source = inst.icon.and_then(|url| Images::get(url, 96, cx));
    let id = inst.id;
    let running = inst.running_secs;

    let loader_line = if inst.loader == "Vanilla" {
        format!("Vanilla {}", inst.game_version)
    } else {
        format!("{} {}", inst.loader, inst.game_version)
    };

    let footer_left: AnyElement = if let Some(secs) = running {
        running_chip(secs, cx).into_any_element()
    } else if inst.issues > 0 {
        state_chip(format!("{} issue", inst.issues), p.warning, p.warning_soft).into_any_element()
    } else {
        div()
            .text_size(px(type_scale::SMALL))
            .text_color(p.text_muted)
            .child(format!(
                "{} · {}",
                short_duration(inst.played_secs),
                inst.last_played
            ))
            .into_any_element()
    };

    let action = if running.is_some() {
        div()
            .id(SharedString::from(format!("stop-{id}")))
            .size(px(32.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(radius::MD))
            .bg(p.danger_soft)
            .cursor_pointer()
            .child(icon("square", 14.0, p.danger))
    } else {
        div()
            .id(SharedString::from(format!("play-{id}")))
            .size(px(32.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(radius::MD))
            .bg(p.raised)
            .cursor_pointer()
            .hover(|s| s.bg(p.accent))
            .child(icon("play", 14.0, p.text))
    };

    div()
        .id(SharedString::from(format!("card-{id}")))
        .w(px(width))
        .flex()
        .flex_col()
        .rounded(px(radius::LG))
        .bg(p.surface)
        .border_1()
        .border_color(if running.is_some() {
            p.running_soft
        } else {
            p.border
        })
        .cursor_pointer()
        .hover(|s| s.border_color(p.border_strong))
        .on_click(
            cx.listener(move |this, _: &ClickEvent, _, cx| this.navigate(Route::Instance(id), cx)),
        )
        .child(
            div()
                .flex()
                .gap(space(3.0))
                .p(space(3.5))
                .child(thumb(source, inst.name, 48.0, radius::MD, cx))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .min_w_0()
                        .flex_1()
                        .gap(space(1.5))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(space(1.5))
                                .child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(type_scale::BODY_LG))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(p.text)
                                        .child(inst.name),
                                )
                                .when(inst.update_available, |d| {
                                    d.child(
                                        div().flex_none().size(px(6.0)).rounded_full().bg(p.accent),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .flex()
                                .gap(space(1.5))
                                .overflow_hidden()
                                .child(chip(loader_line, cx).flex_shrink(1.0).min_w_0().truncate())
                                .when(inst.mods > 0, |d| {
                                    d.child(chip(format!("{} mods", inst.mods), cx))
                                }),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
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
        // 3 columns of ~276px fit in 852px with 12px gaps.
        let w = card_width(852.0);
        assert_eq!(w, 276.0);
        // Narrow windows fall back to one full-width column.
        assert_eq!(card_width(200.0), 200.0);
    }
}
