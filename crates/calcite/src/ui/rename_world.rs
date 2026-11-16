//! Rename a singleplayer world (its in-game name, not the folder).

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{
    ClickEvent, Context, Entity, EventEmitter, FontWeight, SharedString, Window, div, px,
};

use crate::backend::Backend;
use crate::theme::{PaletteExt, radius, space, type_scale};
use crate::ui::components::Button;
use crate::ui::create_instance::DialogEvent;

pub struct RenameWorldDialog {
    instance_dir: std::path::PathBuf,
    folder: String,
    name: Entity<InputState>,
    saving: bool,
    error: Option<SharedString>,
}

impl EventEmitter<DialogEvent> for RenameWorldDialog {}

impl RenameWorldDialog {
    pub fn new(
        instance_dir: std::path::PathBuf,
        folder: String,
        current: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("World name"));
        name.update(cx, |s, cx| {
            s.set_value(current, window, cx);
            s.focus(window, cx);
        });
        cx.subscribe_in(&name, window, |this, _, event: &InputEvent, _, cx| {
            if let InputEvent::PressEnter { .. } = event {
                this.save(cx);
            }
        })
        .detach();
        RenameWorldDialog {
            instance_dir,
            folder,
            name,
            saving: false,
            error: None,
        }
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let name = self.name.read(cx).value().trim().to_string();
        if name.is_empty() || self.saving {
            return;
        }
        self.saving = true;
        cx.notify();
        let (dir, folder) = (self.instance_dir.clone(), self.folder.clone());
        let task = Backend::global(cx)
            .call(async move { theseus::worlds::rename_world(&dir, &folder, &name).await });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.saving = false;
                match result {
                    Ok(()) => cx.emit(DialogEvent::WorldsChanged),
                    Err(error) => this.error = Some(
                        format!(
                            "Couldn't rename it. Close the game if this world is open. ({error:#})"
                        )
                        .into(),
                    ),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for RenameWorldDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();
        div()
            .w(px(400.0))
            .flex()
            .flex_col()
            .gap(space(4.0))
            .p(space(6.0))
            .rounded(px(radius::XL))
            .bg(p.overlay)
            .border_1()
            .border_color(p.border_strong)
            .shadow_lg()
            .child(
                div()
                    .text_size(px(type_scale::SECTION))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(p.text)
                    .child("Rename world"),
            )
            .child(Input::new(&self.name))
            .when_some(self.error.clone(), |d, e| {
                d.child(
                    div()
                        .text_size(px(type_scale::SMALL))
                        .text_color(p.danger)
                        .child(e),
                )
            })
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(space(2.0))
                    .child(Button::new("rename-world-cancel").label("Cancel").on_click(
                        cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(DialogEvent::Close)),
                    ))
                    .child(
                        Button::new("rename-world-save")
                            .label(if self.saving { "Saving…" } else { "Rename" })
                            .primary()
                            .disabled(self.saving)
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.save(cx))),
                    ),
            )
    }
}
