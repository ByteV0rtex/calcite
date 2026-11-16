//! "Add to which instance?" for files dropped outside an instance page.

use std::path::PathBuf;

use calcite_core::dropped::Dropped;
use gpui_kit::prelude::*;
use gpui_kit::{ClickEvent, Context, EventEmitter, FontWeight, SharedString, Window, div, px};

use crate::state::AppState;
use crate::theme::{PaletteExt, radius, space, type_scale};
use crate::ui::components::{Button, thumb};
use crate::ui::create_instance::DialogEvent;
use crate::ui::shell::instance_icon;

pub struct DropDialog {
    items: Vec<(PathBuf, Dropped)>,
}

impl EventEmitter<DialogEvent> for DropDialog {}

impl DropDialog {
    pub fn new(items: Vec<(PathBuf, Dropped)>) -> Self {
        DropDialog { items }
    }

    fn summary(&self) -> String {
        let mut counts: Vec<(&'static str, usize)> = Vec::new();
        for (_, kind) in &self.items {
            match counts.iter_mut().find(|(k, _)| *k == kind.label()) {
                Some((_, n)) => *n += 1,
                None => counts.push((kind.label(), 1)),
            }
        }
        counts
            .into_iter()
            .map(|(k, n)| {
                if n == 1 {
                    format!("1 {k}")
                } else {
                    format!("{n} {k}s")
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl Render for DropDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();
        let mut instances = AppState::global(cx).read(cx).instances.clone();
        instances.sort_by_key(|i| std::cmp::Reverse(i.last_played));

        let mut list = div()
            .id("drop-instances")
            .flex()
            .flex_col()
            .max_h(px(360.0))
            .overflow_y_scroll()
            .p(space(1.5));
        for (ix, inst) in instances.iter().enumerate() {
            let id = inst.id.clone();
            let source = instance_icon(inst);
            list = list.child(
                div()
                    .id(SharedString::from(format!("drop-to-{ix}")))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(space(3.0))
                    .h(px(48.0))
                    .px(space(3.0))
                    .rounded(px(radius::SM))
                    .cursor_pointer()
                    .hover(|s| s.bg(p.hover))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        let items = std::mem::take(&mut this.items);
                        let id = id.clone();
                        AppState::global(cx).update(cx, |s, cx| s.add_dropped(id, items, cx));
                        cx.emit(DialogEvent::Close);
                    }))
                    .child(thumb(source, &inst.name, 32.0, radius::SM, cx))
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
                                    .child(inst.name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(type_scale::SMALL))
                                    .text_color(p.text_muted)
                                    .child(inst.loader_line()),
                            ),
                    ),
            );
        }

        div()
            .w(px(480.0))
            .flex()
            .flex_col()
            .rounded(px(radius::XL))
            .bg(p.overlay)
            .border_1()
            .border_color(p.border_strong)
            .shadow_lg()
            .child(
                div()
                    .flex()
                    .items_start()
                    .justify_between()
                    .px(space(5.0))
                    .pt(space(5.0))
                    .pb(space(3.0))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(space(1.0))
                            .child(
                                div()
                                    .text_size(px(type_scale::SECTION))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(p.text)
                                    .child("Add to which instance?"),
                            )
                            .child(
                                div()
                                    .text_size(px(type_scale::SMALL))
                                    .text_color(p.text_muted)
                                    .child(self.summary()),
                            ),
                    )
                    .child(
                        Button::new("drop-close")
                            .icon("x")
                            .ghost()
                            .small()
                            .on_click(
                                cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(DialogEvent::Close)),
                            ),
                    ),
            )
            .child(div().border_t_1().border_color(p.border).child(list))
    }
}
