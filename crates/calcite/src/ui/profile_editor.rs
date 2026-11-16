//! Edit your Modrinth profile: name, bio and picture.

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::prelude::*;
use gpui_kit::{
    ClickEvent, Context, Entity, EventEmitter, FontWeight, SharedString, Window, div, px,
};

use crate::backend::Backend;
use crate::images::Images;
use crate::state::AppState;
use crate::theme::{PaletteExt, radius, space, type_scale};
use crate::ui::components::{Button, thumb};
use crate::ui::create_instance::DialogEvent;

pub struct ProfileEditor {
    user_id: String,
    name: Entity<InputState>,
    bio: Entity<TextareaState>,
    avatar: Option<String>,
    loaded: bool,
    saving: bool,
    error: Option<SharedString>,
}

impl EventEmitter<DialogEvent> for ProfileEditor {}

impl ProfileEditor {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let user = AppState::global(cx).read(cx).modrinth.clone();
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("Username"));
        let bio = cx.new(|cx| TextareaState::new(window, cx));
        let user_id = user.as_ref().map(|u| u.id.clone()).unwrap_or_default();
        let mut this = ProfileEditor {
            user_id: user_id.clone(),
            name,
            bio,
            avatar: user.as_ref().and_then(|u| u.avatar_url.clone()),
            loaded: false,
            saving: false,
            error: None,
        };
        if user_id.is_empty() {
            this.error = Some("Sign in to Modrinth first.".into());
            return this;
        }
        let fetch = Backend::global(cx)
            .call(async move { theseus::users::get_user_profile(&user_id).await });
        cx.spawn_in(window, async move |this, cx| {
            let result = fetch.await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(profile) => {
                        let text = |key: &str| {
                            profile
                                .get(key)
                                .and_then(|v| v.as_str())
                                .unwrap_or_default()
                                .to_string()
                        };
                        let (name, bio) = (text("username"), text("bio"));
                        this.name.update(cx, |s, cx| s.set_value(name, window, cx));
                        this.bio.update(cx, |s, cx| s.set_value(bio, window, cx));
                        if let Some(url) = profile.get("avatar_url").and_then(|v| v.as_str()) {
                            this.avatar = Some(url.to_string());
                        }
                        this.loaded = true;
                    }
                    Err(error) => {
                        this.error = Some(format!("Couldn't load your profile. {error:#}").into())
                    }
                }
                cx.notify();
            });
        })
        .detach();
        this
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let name = self.name.read(cx).value().trim().to_string();
        let bio = self.bio.read(cx).value().trim().to_string();
        if name.is_empty() || self.saving {
            return;
        }
        self.saving = true;
        self.error = None;
        cx.notify();
        let id = self.user_id.clone();
        let task = Backend::global(cx).call(async move {
            theseus::users::patch_user(&id, serde_json::json!({ "username": name, "bio": bio }))
                .await
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.saving = false;
                match result {
                    Ok(()) => {
                        AppState::global(cx).update(cx, |s, cx| s.refresh_modrinth(cx));
                        cx.emit(DialogEvent::Close);
                    }
                    Err(error) => {
                        this.error = Some(format!("Modrinth didn't accept that. {error:#}").into())
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn change_avatar(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Use as profile picture".into()),
        });
        let id = self.user_id.clone();
        let backend = Backend::global(cx).clone();
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("png")
                .to_lowercase();
            let result = backend
                .call(async move {
                    let data = tokio::fs::read(&path).await?;
                    theseus::users::change_user_avatar(&id, bytes::Bytes::from(data), &ext).await
                })
                .await;
            let _ = this.update(cx, |this, cx| this.avatar_changed(result, cx));
        })
        .detach();
    }

    fn remove_avatar(&mut self, cx: &mut Context<Self>) {
        let id = self.user_id.clone();
        let task =
            Backend::global(cx).call(async move { theseus::users::delete_user_avatar(&id).await });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if result.is_ok() {
                    this.avatar = None;
                }
                this.avatar_changed(result, cx)
            });
        })
        .detach();
    }

    fn avatar_changed(&mut self, result: anyhow::Result<()>, cx: &mut Context<Self>) {
        match result {
            Ok(()) => {
                AppState::global(cx).update(cx, |s, cx| s.refresh_modrinth(cx));
                // Pick up the new picture's address.
                let id = self.user_id.clone();
                let fetch = Backend::global(cx)
                    .call(async move { theseus::users::get_user_profile(&id).await });
                cx.spawn(async move |this, cx| {
                    if let Ok(profile) = fetch.await {
                        let _ = this.update(cx, |this, cx| {
                            this.avatar = profile
                                .get("avatar_url")
                                .and_then(|v| v.as_str())
                                .map(str::to_string);
                            cx.notify();
                        });
                    }
                })
                .detach();
            }
            Err(error) => {
                self.error = Some(format!("Couldn't change the picture. {error:#}").into())
            }
        }
        cx.notify();
    }
}

impl Render for ProfileEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();
        let name = self.name.read(cx).value().to_string();
        let source = self.avatar.as_deref().and_then(|u| Images::get(u, 128, cx));
        let label = |text: &'static str| {
            div()
                .text_size(px(type_scale::SMALL))
                .font_weight(FontWeight::MEDIUM)
                .text_color(p.text_secondary)
                .child(text)
        };
        div()
            .w(px(520.0))
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
                    .items_center()
                    .justify_between()
                    .px(space(6.0))
                    .pt(space(5.0))
                    .pb(space(3.0))
                    .child(
                        div()
                            .text_size(px(type_scale::SECTION))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.text)
                            .child("Your Modrinth profile"),
                    )
                    .child(Button::new("pe-close").icon("x").ghost().small().on_click(
                        cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(DialogEvent::Close)),
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(space(4.0))
                    .px(space(6.0))
                    .py(space(3.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(space(3.0))
                            .child(thumb(source, &name, 64.0, radius::LG, cx))
                            .child(
                                Button::new("pe-avatar")
                                    .icon("image")
                                    .label("Change picture…")
                                    .small()
                                    .disabled(!self.loaded)
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.change_avatar(cx)
                                    })),
                            )
                            .when(self.avatar.is_some(), |d| {
                                d.child(
                                    Button::new("pe-avatar-remove")
                                        .label("Remove")
                                        .small()
                                        .ghost()
                                        .disabled(!self.loaded)
                                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                            this.remove_avatar(cx)
                                        })),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(space(1.5))
                            .child(label("Username"))
                            .child(Input::new(&self.name)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(space(1.5))
                            .child(label("Bio"))
                            .child(div().h(px(110.0)).child(Textarea::new(&self.bio).h_full())),
                    )
                    .when_some(self.error.clone(), |d, e| {
                        d.child(
                            div()
                                .text_size(px(type_scale::SMALL))
                                .text_color(p.danger)
                                .child(e),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(space(2.0))
                    .px(space(6.0))
                    .py(space(4.0))
                    .border_t_1()
                    .border_color(p.border)
                    .child(Button::new("pe-cancel").label("Cancel").on_click(
                        cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(DialogEvent::Close)),
                    ))
                    .child(
                        Button::new("pe-save")
                            .icon("check")
                            .label(if self.saving { "Saving…" } else { "Save" })
                            .primary()
                            .disabled(!self.loaded || self.saving)
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.save(cx))),
                    ),
            )
    }
}
