//! The design system on one page: palette, type, controls, states.
//! Used for design review; becomes Settings › Appearance in milestone 5.

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, ClickEvent, Context, FontWeight, Hsla, SharedString, Window, div, px};

use crate::theme::{Accent, Appearance, PaletteExt, radius, space, type_scale};
use crate::ui::components::{
    Button, ButtonSize, chip, empty_state, icon, icon_chip, panel, progress_bar, section_heading,
    skeleton, state_chip, thumb,
};
use crate::ui::shell::{Shell, set_look};

pub fn render(_shell: &mut Shell, _window: &mut Window, cx: &mut Context<Shell>) -> AnyElement {
    let p = cx.palette().clone();

    let appearance_picker = {
        let mut row = div().flex().gap(space(2.0));
        for (label, appearance) in [
            ("Dark", Appearance::Dark),
            ("Light", Appearance::Light),
            ("OLED", Appearance::Oled),
        ] {
            let active = p.appearance == appearance;
            let accent = p.accent_kind;
            row =
                row.child(
                    div()
                        .id(SharedString::from(format!("appearance-{label}")))
                        .h(px(30.0))
                        .px(space(3.0))
                        .flex()
                        .items_center()
                        .rounded(px(radius::SM))
                        .border_1()
                        .border_color(if active { p.accent } else { p.border })
                        .bg(if active { p.accent_soft } else { p.surface })
                        .text_size(px(type_scale::BODY))
                        .text_color(if active {
                            p.accent_text
                        } else {
                            p.text_secondary
                        })
                        .cursor_pointer()
                        .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                            set_look(appearance, accent, cx)
                        }))
                        .child(label),
                );
        }
        row
    };

    let accent_picker = {
        let mut row = div().flex().gap(space(2.0));
        for accent in Accent::ALL {
            let swatch = crate::theme::Palette::new(p.appearance, accent).accent;
            let active = p.accent_kind == accent;
            let appearance = p.appearance;
            row =
                row.child(
                    div()
                        .id(SharedString::from(format!("accent-{}", accent.name())))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(space(1.5))
                        .cursor_pointer()
                        .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                            set_look(appearance, accent, cx)
                        }))
                        .child(
                            div()
                                .size(px(28.0))
                                .rounded_full()
                                .p(px(3.0))
                                .border_2()
                                .border_color(if active {
                                    swatch
                                } else {
                                    Hsla::transparent_black()
                                })
                                .child(div().size_full().rounded_full().bg(swatch)),
                        )
                        .child(
                            div()
                                .text_size(px(type_scale::CAPTION))
                                .text_color(if active { p.text } else { p.text_muted })
                                .child(accent.name()),
                        ),
                );
        }
        row
    };

    let swatch = |name: &'static str, color: Hsla| {
        div()
            .flex()
            .flex_col()
            .gap(space(1.5))
            .w(px(96.0))
            .child(
                div()
                    .h(px(44.0))
                    .rounded(px(radius::MD))
                    .bg(color)
                    .border_1()
                    .border_color(p.border),
            )
            .child(
                div()
                    .text_size(px(type_scale::CAPTION))
                    .text_color(p.text_muted)
                    .child(name),
            )
    };

    let surfaces = div()
        .flex()
        .flex_wrap()
        .gap(space(3.0))
        .child(swatch("rail", p.rail))
        .child(swatch("app", p.app))
        .child(swatch("surface", p.surface))
        .child(swatch("raised", p.raised))
        .child(swatch("hover", p.hover))
        .child(swatch("border", p.border_strong))
        .child(swatch("accent", p.accent))
        .child(swatch("running", p.running))
        .child(swatch("warning", p.warning))
        .child(swatch("danger", p.danger))
        .child(swatch("info", p.info));

    let type_specimen = {
        let mut col = div().flex().flex_col().gap(space(2.0));
        for (size, weight, sample) in [
            (type_scale::DISPLAY, FontWeight::SEMIBOLD, "41h 20m"),
            (
                type_scale::HEADLINE,
                FontWeight::SEMIBOLD,
                "Fabulously Optimized",
            ),
            (type_scale::TITLE, FontWeight::SEMIBOLD, "Library"),
            (type_scale::SECTION, FontWeight::SEMIBOLD, "Create Survival"),
            (
                type_scale::BODY_LG,
                FontWeight::MEDIUM,
                "Play with 87 mods on Fabric 26.3",
            ),
            (
                type_scale::BODY,
                FontWeight::NORMAL,
                "Sodium makes chunks render faster and frees up your CPU.",
            ),
            (
                type_scale::SMALL,
                FontWeight::NORMAL,
                "Updated 2 hours ago · 19.3M downloads",
            ),
            (type_scale::CAPTION, FontWeight::MEDIUM, "FABRIC 26.3"),
        ] {
            col = col.child(
                div()
                    .flex()
                    .items_center()
                    .gap(space(4.0))
                    .child(
                        div()
                            .w(px(40.0))
                            .text_size(px(type_scale::CAPTION))
                            .text_color(p.text_disabled)
                            .child(format!("{size}")),
                    )
                    .child(
                        div()
                            .text_size(px(size))
                            .font_weight(weight)
                            .text_color(p.text)
                            .child(sample),
                    ),
            );
        }
        col
    };

    let buttons = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(space(2.0))
        .child(
            Button::new("g-play")
                .icon("play")
                .label("Play")
                .primary()
                .size(ButtonSize::Large),
        )
        .child(
            Button::new("g-install")
                .icon("arrow-down-to-line")
                .label("Install")
                .primary(),
        )
        .child(
            Button::new("g-sec")
                .icon("folder-open")
                .label("Open folder"),
        )
        .child(
            Button::new("g-ghost")
                .icon("settings")
                .label("Settings")
                .ghost(),
        )
        .child(
            Button::new("g-danger")
                .icon("trash")
                .label("Delete instance")
                .danger(),
        )
        .child(Button::new("g-small").label("Small").small())
        .child(Button::new("g-icon").icon("ellipsis"))
        .child(Button::new("g-disabled").label("Disabled").disabled(true));

    let chips = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(space(2.0))
        .child(chip("Fabric 26.3", cx))
        .child(chip("87 mods", cx))
        .child(icon_chip("clock", "41h 20m", cx))
        .child(state_chip("Running · 18m", p.running, p.running_soft))
        .child(state_chip("Update available", p.accent_text, p.accent_soft))
        .child(state_chip("1 issue", p.warning, p.warning_soft))
        .child(state_chip("Crashed", p.danger, p.danger_soft))
        .child(state_chip("Downloading", p.info, p.info_soft));

    let loading = panel(cx)
        .p(space(4.0))
        .flex()
        .gap(space(4.0))
        .child(skeleton(px(56.0), px(56.0), cx).rounded(px(radius::MD)))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .gap(space(2.0))
                .child(skeleton(px(180.0), px(14.0), cx))
                .child(skeleton(gpui_kit::relative(0.8), px(12.0), cx))
                .child(div().pt(space(2.0)).child(progress_bar(0.42, cx))),
        );

    let monograms = div()
        .flex()
        .gap(space(3.0))
        .child(thumb(None, "Vanilla 26.3", 48.0, radius::MD, cx))
        .child(thumb(None, "PvP Practice", 48.0, radius::MD, cx))
        .child(thumb(None, "Snapshot Testing", 48.0, radius::MD, cx))
        .child(thumb(None, "Building World", 48.0, radius::MD, cx));

    let block = |title: &'static str, body: AnyElement| {
        div()
            .flex()
            .flex_col()
            .gap(space(3.0))
            .child(section_heading(title, cx))
            .child(body)
    };

    div()
        .id("gallery-scroll")
        .size_full()
        .overflow_y_scroll()
        .child(
            div()
                .flex()
                .flex_col()
                .gap(space(8.0))
                .px(space(8.0))
                .py(space(7.0))
                .max_w(px(980.0))
                .child(crate::ui::pages::page_header(
                    "Design system",
                    Some("Calcite's palette, type and controls".into()),
                    cx,
                ))
                .child(block("Appearance", appearance_picker.into_any_element()))
                .child(block("Accent", accent_picker.into_any_element()))
                .child(block("Palette", surfaces.into_any_element()))
                .child(block("Type scale (Inter)", type_specimen.into_any_element()))
                .child(block("Buttons", buttons.into_any_element()))
                .child(block("Chips and states", chips.into_any_element()))
                .child(block("Loading", loading.into_any_element()))
                .child(block("Instances without art", monograms.into_any_element()))
                .child(block(
                    "Empty state",
                    panel(cx)
                        .child(empty_state(
                            "layout-grid",
                            "No instances yet",
                            "Install a modpack from Discover, create one from scratch, or bring your instances over from another launcher.",
                            Some(
                                div()
                                    .flex()
                                    .gap(space(2.0))
                                    .child(Button::new("e-new").icon("plus").label("New instance").primary())
                                    .child(Button::new("e-import").icon("arrow-down-to-line").label("Move in from another launcher"))
                                    .into_any_element(),
                            ),
                            cx,
                        ))
                        .into_any_element(),
                ))
                .child(div().flex().items_center().gap(space(2.0)).child(icon("info", 14.0, p.text_muted)).child(
                    div()
                        .text_size(px(type_scale::SMALL))
                        .text_color(p.text_muted)
                        .child("Icons: Lucide. Type: Inter and JetBrains Mono. All open licenses."),
                )),
        )
        .into_any_element()
}
