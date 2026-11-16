//! Add or edit a server in an instance's server list.

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{
    ClickEvent, Context, Entity, EventEmitter, FontWeight, SharedString, Window, div, px,
};
use theseus::worlds::ServerPackStatus;

use crate::backend::Backend;
use crate::theme::{PaletteExt, radius, space, type_scale};
use crate::ui::components::Button;
use crate::ui::create_instance::DialogEvent;

/// An existing entry being edited.
#[derive(Clone)]
pub struct Existing {
    pub index: usize,
    pub name: String,
    pub address: String,
    pub pack: Option<serde_json::Value>,
}

pub struct ServerDialog {
    instance_id: String,
    existing: Option<Existing>,
    name: Entity<InputState>,
    address: Entity<InputState>,
    saving: bool,
    error: Option<SharedString>,
}

impl EventEmitter<DialogEvent> for ServerDialog {}

impl ServerDialog {
    pub fn new(
        instance_id: String,
        existing: Option<Existing>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("My server"));
        let address = cx.new(|cx| InputState::new(window, cx).placeholder("play.example.com"));
        if let Some(e) = &existing {
            name.update(cx, |s, cx| s.set_value(e.name.clone(), window, cx));
            address.update(cx, |s, cx| s.set_value(e.address.clone(), window, cx));
        }
        for input in [&name, &address] {
            cx.subscribe_in(input, window, |this, _, event: &InputEvent, _, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    this.save(cx);
                }
            })
            .detach();
        }
        name.update(cx, |s, cx| s.focus(window, cx));
        ServerDialog {
            instance_id,
            existing,
            name,
            address,
            saving: false,
            error: None,
        }
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let address = self.address.read(cx).value().trim().to_string();
        let mut name = self.name.read(cx).value().trim().to_string();
        if address.is_empty() || address.contains(char::is_whitespace) {
            self.error = Some(
                "Enter the server's address, like play.example.com or 192.168.1.20:25565.".into(),
            );
            cx.notify();
            return;
        }
        if name.is_empty() {
            name = address.clone();
        }
        self.saving = true;
        cx.notify();
        let id = self.instance_id.clone();
        let existing = self.existing.clone();
        let task = Backend::global(cx).call(async move {
            match existing {
                Some(e) => {
                    let pack = e
                        .pack
                        .and_then(|v| serde_json::from_value::<ServerPackStatus>(v).ok())
                        .unwrap_or_default();
                    theseus::worlds::edit_server_in_instance(&id, e.index, name, address, pack)
                        .await
                }
                None => theseus::worlds::add_server_to_instance(
                    &id,
                    name,
                    address,
                    ServerPackStatus::Prompt,
                    None,
                    None,
                )
                .await
                .map(|_| ()),
            }
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.saving = false;
                match result {
                    Ok(()) => cx.emit(DialogEvent::WorldsChanged),
                    Err(error) => this.error = Some(format!("{error:#}").into()),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for ServerDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();
        let label = |text: &'static str| {
            div()
                .text_size(px(type_scale::SMALL))
                .font_weight(FontWeight::MEDIUM)
                .text_color(p.text_secondary)
                .child(text)
        };
        div()
            .w(px(420.0))
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
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(type_scale::SECTION))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.text)
                            .child(if self.existing.is_some() {
                                "Edit server"
                            } else {
                                "Add a server"
                            }),
                    )
                    .child(
                        Button::new("server-close")
                            .icon("x")
                            .ghost()
                            .small()
                            .on_click(
                                cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(DialogEvent::Close)),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(space(1.5))
                    .child(label("Name"))
                    .child(Input::new(&self.name)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(space(1.5))
                    .child(label("Address"))
                    .child(Input::new(&self.address)),
            )
            .when_some(self.error.clone(), |d, e| {
                d.child(
                    div()
                        .text_size(px(type_scale::SMALL))
                        .text_color(p.danger)
                        .child(e),
                )
            })
            .child(
                div().flex().justify_end().child(
                    Button::new("server-save")
                        .label(if self.saving {
                            "Saving…"
                        } else if self.existing.is_some() {
                            "Save"
                        } else {
                            "Add server"
                        })
                        .primary()
                        .disabled(self.saving)
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.save(cx))),
                ),
            )
    }
}
