pub mod discover;
pub mod gallery;
pub mod game_settings;
pub mod health_tab;
pub mod home;
pub mod instance;
pub mod instance_tabs;
pub mod library;
pub mod move_in;
pub mod project;
pub mod settings;

use gpui_kit::prelude::*;
use gpui_kit::{App, FontWeight, SharedString, div, px};

use crate::theme::{PaletteExt, space, type_scale};

/// Page title with an optional quiet subtitle on the same baseline.
pub fn page_header(
    title: impl Into<SharedString>,
    subtitle: Option<String>,
    cx: &App,
) -> gpui_kit::Div {
    let p = cx.palette();
    div()
        .flex()
        .items_baseline()
        .gap(space(3.0))
        .child(
            div()
                .text_size(px(type_scale::TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(p.text)
                .child(title.into()),
        )
        .when_some(subtitle, |d, s| {
            d.child(
                div()
                    .text_size(px(type_scale::BODY))
                    .text_color(p.text_muted)
                    .child(s),
            )
        })
}
