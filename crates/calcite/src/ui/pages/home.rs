//! Home: get back into the game fast.

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, ClickEvent, Context, FontWeight, SharedString, Window, div, px};

use crate::theme::{PaletteExt, radius, space, type_scale};
use crate::ui::components::{
    Button, empty_state, relative_time, section_heading, short_duration, thumb,
};
use crate::ui::shell::{Route, Shell, instance_icon};

pub fn render(shell: &mut Shell, _window: &mut Window, cx: &mut Context<Shell>) -> AnyElement {
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
                    "Sign in with your Microsoft account (bottom left), then install a modpack or make your own instance."
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
