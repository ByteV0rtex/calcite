//! Appearance and accent pickers, shared by Settings, onboarding and the
//! design-system page. Choices are applied immediately and remembered.

use gpui_kit::prelude::*;
use gpui_kit::{App, Hsla, SharedString, div, px};

use crate::prefs::Prefs;
use crate::theme::{Accent, Appearance, Palette, PaletteExt, radius, space, type_scale};
use crate::ui::components::icon;

pub fn appearance_picker(cx: &App) -> impl IntoElement {
    let p = cx.palette().clone();
    let mut row = div().flex().gap(space(2.0));
    let following = cx.global::<Prefs>().follow_system;
    // None: match the system.
    for (label, choice, glyph) in [
        ("Dark", Some(Appearance::Dark), "moon"),
        ("Light", Some(Appearance::Light), "sun"),
        ("OLED", Some(Appearance::Oled), "monitor"),
        ("System", None, "sun-medium"),
    ] {
        let active = match choice {
            Some(appearance) => !following && p.appearance == appearance,
            None => following,
        };
        let accent = p.accent_kind;
        let appearance = choice.unwrap_or(Appearance::Light);
        // A tiny preview of each appearance's surfaces; System shows both.
        let preview = Palette::new(appearance, accent);
        let night = Palette::new(Appearance::Dark, accent);
        row = row.child(
            div()
                .id(SharedString::from(format!("appearance-{label}")))
                .w(px(132.0))
                .flex()
                .flex_col()
                .gap(space(2.0))
                .p(space(2.0))
                .rounded(px(radius::LG))
                .border_1()
                .border_color(if active { p.accent } else { p.border })
                .bg(if active { p.accent_soft } else { p.surface })
                .cursor_pointer()
                .hover(|s| s.border_color(if active { p.accent } else { p.border_strong }))
                .on_click(move |_, _, cx| match choice {
                    Some(appearance) => Prefs::set_look(appearance, accent, cx),
                    None => Prefs::set_follow_system(accent, cx),
                })
                .child(
                    div()
                        .h(px(56.0))
                        .rounded(px(radius::MD))
                        .bg(preview.app)
                        .border_1()
                        .border_color(if choice.is_none() {
                            night.border
                        } else {
                            preview.border
                        })
                        .flex()
                        .gap(px(4.0))
                        .p(px(6.0))
                        .relative()
                        .overflow_hidden()
                        .child(div().w(px(22.0)).h_full().rounded(px(3.0)).bg(preview.rail))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .gap(px(4.0))
                                .child(div().h(px(14.0)).rounded(px(3.0)).bg(preview.surface))
                                .child(
                                    div()
                                        .h(px(8.0))
                                        .w(px(28.0))
                                        .rounded(px(3.0))
                                        .bg(preview.accent),
                                ),
                        )
                        .when(choice.is_none(), |d| {
                            d.child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .right_0()
                                    .bottom_0()
                                    .w_1_2()
                                    .bg(night.app)
                                    .border_l_1()
                                    .border_color(night.border),
                            )
                        }),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space(1.5))
                        .text_size(px(type_scale::BODY))
                        .text_color(if active { p.text } else { p.text_secondary })
                        .child(icon(
                            glyph,
                            14.0,
                            if active { p.accent } else { p.text_muted },
                        ))
                        .child(label),
                ),
        );
    }
    row
}

pub fn accent_picker(cx: &App) -> impl IntoElement {
    let p = cx.palette().clone();
    let mut row = div().flex().flex_wrap().gap(space(3.0));
    for accent in Accent::ALL {
        let swatch = Palette::new(p.appearance, accent).accent;
        let active = p.accent_kind == accent;
        let appearance = p.appearance;
        row = row.child(
            div()
                .id(SharedString::from(format!("accent-{}", accent.name())))
                .flex()
                .flex_col()
                .items_center()
                .gap(space(1.5))
                .cursor_pointer()
                .on_click(move |_, _, cx| Prefs::set_look(appearance, accent, cx))
                .child(
                    div()
                        .size(px(30.0))
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
}
