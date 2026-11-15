//! A single instance: header with Play, then tabs for its content, logs and
//! the tools that keep it healthy.

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, ClickEvent, Context, FontWeight, SharedString, Window, div, px};

use crate::images::Images;
use crate::theme::{self, PaletteExt, radius, space, type_scale};
use crate::ui::components::{
    Button, ButtonSize, chip, icon, icon_chip, short_duration, state_chip, thumb,
};
use crate::ui::sample::{self, SampleInstance};
use crate::ui::shell::{Shell, running_chip};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Content,
    Worlds,
    Logs,
    Health,
    Snapshots,
    Files,
}

impl Tab {
    const ALL: [Tab; 6] = [
        Tab::Content,
        Tab::Worlds,
        Tab::Logs,
        Tab::Health,
        Tab::Snapshots,
        Tab::Files,
    ];

    fn label(self) -> &'static str {
        match self {
            Tab::Content => "Content",
            Tab::Worlds => "Worlds",
            Tab::Logs => "Logs",
            Tab::Health => "Health",
            Tab::Snapshots => "Snapshots",
            Tab::Files => "Files",
        }
    }
}

pub fn render(
    _shell: &mut Shell,
    id: &'static str,
    tab: Tab,
    _window: &mut Window,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let Some(inst) = sample::INSTANCES.iter().find(|i| i.id == id) else {
        return div().into_any_element();
    };
    let p = cx.palette().clone();

    let tabs = {
        let mut row = div()
            .flex()
            .items_end()
            .gap(space(5.0))
            .h(px(40.0))
            .px(space(8.0))
            .border_b_1()
            .border_color(p.border);
        for t in Tab::ALL {
            let active = t == tab;
            let badge = match t {
                Tab::Content => Some(format!("{}", inst.mods)),
                Tab::Health if inst.issues > 0 => Some(format!("{}", inst.issues)),
                _ => None,
            };
            row =
                row.child(
                    div()
                        .id(SharedString::from(format!("tab-{}", t.label())))
                        .flex()
                        .items_center()
                        .gap(space(1.5))
                        .h_full()
                        .border_b_2()
                        .border_color(if active {
                            p.accent
                        } else {
                            gpui_kit::Hsla::transparent_black()
                        })
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.set_instance_tab(t, cx)
                        }))
                        .child(
                            div()
                                .text_size(px(type_scale::BODY))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(if active { p.text } else { p.text_secondary })
                                .child(t.label()),
                        )
                        .when_some(badge, |d, b| {
                            let warn = t == Tab::Health;
                            d.child(
                                div()
                                    .px(space(1.5))
                                    .rounded(px(radius::SM))
                                    .bg(if warn { p.warning_soft } else { p.raised })
                                    .text_size(px(type_scale::CAPTION))
                                    .text_color(if warn { p.warning } else { p.text_muted })
                                    .child(b),
                            )
                        }),
                );
        }
        row
    };

    let body: AnyElement = match tab {
        Tab::Content => content_tab(inst, cx),
        Tab::Logs => logs_tab(cx),
        Tab::Health => health_tab(inst, cx),
        Tab::Worlds | Tab::Snapshots | Tab::Files => div()
            .p(space(8.0))
            .text_color(p.text_muted)
            .child("Arrives in milestone 5.")
            .into_any_element(),
    };

    div()
        .size_full()
        .flex()
        .flex_col()
        .child(header(inst, cx))
        .child(tabs)
        .child(div().flex_1().min_h_0().child(body))
        .into_any_element()
}

fn header(inst: &'static SampleInstance, cx: &mut Context<Shell>) -> impl IntoElement {
    let p = cx.palette().clone();
    let source = inst.icon.and_then(|url| Images::get(url, 128, cx));
    let running = inst.running_secs;

    div()
        .flex()
        .items_center()
        .gap(space(4.0))
        .px(space(8.0))
        .pt(space(7.0))
        .pb(space(5.0))
        .child(thumb(source, inst.name, 64.0, radius::LG, cx))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(space(2.0))
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .text_size(px(type_scale::TITLE))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(p.text)
                        .truncate()
                        .child(inst.name),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space(1.5))
                        .child(chip(format!("{} {}", inst.loader, inst.game_version), cx))
                        .child(icon_chip("clock", short_duration(inst.played_secs), cx))
                        .when_some(running, |d, secs| d.child(running_chip(secs, cx)))
                        .when(inst.update_available, |d| {
                            d.child(state_chip("Modpack update", p.accent_text, p.accent_soft))
                        }),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(space(2.0))
                .child(Button::new("instance-folder").icon("folder-open").ghost())
                .child(Button::new("instance-settings").icon("settings").ghost())
                .child(if running.is_some() {
                    Button::new("instance-stop")
                        .icon("square")
                        .label("Stop")
                        .danger()
                        .size(ButtonSize::Large)
                } else {
                    Button::new("instance-play")
                        .icon("play")
                        .label("Play")
                        .primary()
                        .size(ButtonSize::Large)
                }),
        )
}

fn content_tab(inst: &'static SampleInstance, cx: &mut Context<Shell>) -> AnyElement {
    let p = cx.palette().clone();
    let updates = sample::MODS.iter().filter(|m| m.update.is_some()).count();

    let filter = |label: &'static str, count: Option<usize>, active: bool| {
        div()
            .flex()
            .items_center()
            .gap(space(1.5))
            .h(px(28.0))
            .px(space(2.5))
            .rounded(px(radius::SM))
            .text_size(px(type_scale::SMALL))
            .font_weight(FontWeight::MEDIUM)
            .when(active, |d| d.bg(p.raised).text_color(p.text))
            .when(!active, |d| d.text_color(p.text_secondary))
            .child(label)
            .when_some(count, |d, c| {
                d.child(div().text_color(p.text_muted).child(format!("{c}")))
            })
    };

    let toolbar = div()
        .flex()
        .items_center()
        .gap(space(1.0))
        .px(space(8.0))
        .py(space(3.0))
        .child(filter("All", Some(inst.mods as usize), true))
        .child(filter("Mods", Some(inst.mods as usize - 9), false))
        .child(filter("Resource packs", Some(6), false))
        .child(filter("Shaders", Some(3), false))
        .child(div().flex_1())
        .child(
            div()
                .flex()
                .items_center()
                .gap(space(2.0))
                .h(px(28.0))
                .w(px(200.0))
                .px(space(2.5))
                .rounded(px(radius::SM))
                .bg(p.surface)
                .border_1()
                .border_color(p.border)
                .child(icon("search", 14.0, p.text_muted))
                .child(
                    div()
                        .text_size(px(type_scale::SMALL))
                        .text_color(p.text_muted)
                        .child("Search content"),
                ),
        )
        .when(updates > 0, |d| {
            d.child(
                Button::new("update-all")
                    .icon("refresh-cw")
                    .label(format!("Update {updates}"))
                    .small(),
            )
        })
        .child(
            Button::new("add-content")
                .icon("plus")
                .label("Add content")
                .primary()
                .small(),
        );

    let mut list = div()
        .id("content-list")
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .px(space(8.0))
        .pb(space(8.0));

    for (ix, m) in sample::MODS.iter().enumerate() {
        let source = m.icon.and_then(|url| Images::get(url, 64, cx));
        list = list.child(
            div()
                .id(ix)
                .flex()
                .items_center()
                .gap(space(3.0))
                .h(px(52.0))
                .px(space(3.0))
                .border_b_1()
                .border_color(p.border)
                .hover(|s| s.bg(p.surface))
                .child(toggle(m.enabled, cx))
                .child(div().when(!m.enabled, |d| d.opacity(0.45)).child(thumb(
                    source,
                    m.title,
                    32.0,
                    radius::SM,
                    cx,
                )))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .when(!m.enabled, |d| d.opacity(0.55))
                        .child(
                            div()
                                .flex()
                                .items_baseline()
                                .gap(space(2.0))
                                .child(
                                    div()
                                        .text_size(px(type_scale::BODY))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(p.text)
                                        .child(m.title),
                                )
                                .child(
                                    div()
                                        .text_size(px(type_scale::SMALL))
                                        .text_color(p.text_muted)
                                        .child(format!("by {}", m.author)),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(type_scale::CAPTION))
                                .font_family(theme::fonts::MONO)
                                .text_color(p.text_muted)
                                .truncate()
                                .child(m.file),
                        ),
                )
                .child(
                    div()
                        .w(px(120.0))
                        .flex()
                        .justify_end()
                        .child(match m.update {
                            Some(next) => state_chip(
                                format!("{} → {}", m.version, next),
                                p.accent_text,
                                p.accent_soft,
                            )
                            .into_any_element(),
                            None => div()
                                .text_size(px(type_scale::SMALL))
                                .text_color(p.text_secondary)
                                .child(m.version)
                                .into_any_element(),
                        }),
                )
                .child(
                    Button::new(SharedString::from(format!("more-{ix}")))
                        .icon("ellipsis")
                        .ghost()
                        .small(),
                ),
        );
    }

    div()
        .size_full()
        .flex()
        .flex_col()
        .child(toolbar)
        .child(list)
        .into_any_element()
}

fn toggle(on: bool, cx: &Context<Shell>) -> impl IntoElement {
    let p = cx.palette();
    div()
        .w(px(28.0))
        .h(px(16.0))
        .flex_none()
        .rounded_full()
        .p(px(2.0))
        .bg(if on { p.accent } else { p.border_strong })
        .flex()
        .when(on, |d| d.justify_end())
        .child(div().size(px(12.0)).rounded_full().bg(if on {
            p.on_accent
        } else {
            p.text_secondary
        }))
}

fn logs_tab(cx: &mut Context<Shell>) -> AnyElement {
    let p = cx.palette().clone();
    let lines: &[(&str, &str, &str)] = &[
        (
            "14:02:11",
            "INFO",
            "Loading Minecraft 26.3 with Fabric Loader 0.17.2",
        ),
        (
            "14:02:12",
            "INFO",
            "Loading 87 mods: fabric-api 0.131.0, sodium 0.7.3, iris 1.10.2, lithium 0.17.1 …",
        ),
        (
            "14:02:14",
            "WARN",
            "Reference map 'continuity-refmap.json' for continuity.mixins.json could not be read",
        ),
        (
            "14:02:16",
            "INFO",
            "[Sodium] Using OpenGL 4.1 Metal - 90.5 on Apple M3",
        ),
        (
            "14:02:19",
            "INFO",
            "[Iris] Shaderpack 'ComplementaryReimagined_r5.5' loaded in 412ms",
        ),
        ("14:02:21", "INFO", "Backend library: LWJGL version 3.3.6"),
        (
            "14:02:23",
            "INFO",
            "Created: 4096x2048x4 minecraft:textures/atlas/blocks.png-atlas",
        ),
        (
            "14:02:24",
            "ERROR",
            "Failed to load optional resource pack 'Fresh Animations' (missing pack.mcmeta)",
        ),
        ("14:02:26", "INFO", "Connecting to play.example.net, 25565"),
        ("14:02:27", "INFO", "Joined world as Steve"),
    ];
    let mut list = div()
        .id("log-lines")
        .flex()
        .flex_col()
        .flex_1()
        .overflow_y_scroll()
        .m(space(8.0))
        .mt(space(4.0))
        .p(space(3.0))
        .rounded(px(radius::LG))
        .bg(p.surface)
        .border_1()
        .border_color(p.border)
        .font_family(theme::fonts::MONO)
        .text_size(px(type_scale::SMALL));
    for (ix, (time, level, msg)) in lines.iter().enumerate() {
        let (color, bg) = match *level {
            "WARN" => (p.warning, Some(p.warning_soft)),
            "ERROR" => (p.danger, Some(p.danger_soft)),
            _ => (p.text_muted, None),
        };
        list = list.child(
            div()
                .id(ix)
                .flex()
                .gap(space(3.0))
                .px(space(2.0))
                .py(space(0.75))
                .rounded(px(4.0))
                .when_some(bg, |d, b| d.bg(b))
                .child(div().flex_none().text_color(p.text_disabled).child(*time))
                .child(
                    div()
                        .flex_none()
                        .w(px(44.0))
                        .text_color(color)
                        .child(*level),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_color(p.text_secondary)
                        .child(*msg),
                ),
        );
    }
    div()
        .size_full()
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .items_center()
                .gap(space(2.0))
                .px(space(8.0))
                .pt(space(4.0))
                .child(chip("latest.log", cx))
                .child(div().flex_1())
                .child(Button::new("log-copy").icon("copy").label("Copy").small())
                .child(
                    Button::new("log-share")
                        .icon("share-2")
                        .label("Share log")
                        .small(),
                ),
        )
        .child(list)
        .into_any_element()
}

fn health_tab(inst: &'static SampleInstance, cx: &mut Context<Shell>) -> AnyElement {
    let p = cx.palette().clone();
    let check = |icon_name: &'static str, ok: bool, title: SharedString, detail: SharedString| {
        div()
            .flex()
            .items_center()
            .gap(space(3.0))
            .p(space(3.5))
            .border_b_1()
            .border_color(p.border)
            .child(icon(
                icon_name,
                18.0,
                if ok { p.running } else { p.warning },
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .gap(space(0.5))
                    .child(
                        div()
                            .text_size(px(type_scale::BODY))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(p.text)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(type_scale::SMALL))
                            .text_color(p.text_secondary)
                            .child(detail),
                    ),
            )
    };
    let has_issue = inst.issues > 0;
    div()
        .p(space(8.0))
        .pt(space(5.0))
        .child(
            crate::ui::components::panel(cx)
                .overflow_hidden()
                .child(
                    check(
                        if has_issue { "triangle-alert" } else { "circle-check" },
                        !has_issue,
                        if has_issue {
                            "Missing dependency: Architectury API".into()
                        } else {
                            "All dependencies present".into()
                        },
                        if has_issue {
                            "Cobblemon needs Architectury API 13.0 or newer. One click installs the right version.".into()
                        } else {
                            "Every mod has what it needs to load.".into()
                        },
                    )
                    .when(has_issue, |d| {
                        d.child(Button::new("fix-dep").label("Install it").primary().small())
                    }),
                )
                .child(check(
                    "circle-check",
                    true,
                    "No duplicate or conflicting mods".into(),
                    format!(
                        "Checked {} jars against each other and Modrinth's compatibility data.",
                        inst.mods
                    )
                    .into(),
                ))
                .child(check(
                    "circle-check",
                    true,
                    "Java 21 matches this version".into(),
                    format!(
                        "Minecraft {} needs Java 21. Calcite manages it for you.",
                        inst.game_version
                    )
                    .into(),
                ))
                .child(check(
                    "circle-check",
                    true,
                    "Memory: 6 GB of 16 GB".into(),
                    format!(
                        "A good fit for {} mods on this machine. Change it in instance settings.",
                        inst.mods
                    )
                    .into(),
                )),
        )
        .into_any_element()
}
