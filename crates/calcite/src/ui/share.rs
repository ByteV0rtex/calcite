//! Shared instances: share one of yours with friends through Modrinth, or
//! join one from an invite link. Shared instances stay in sync: when the
//! owner publishes changes, everyone can update.

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, ClickEvent, ClipboardItem, Context, Entity, EventEmitter, FontWeight, SharedString,
    Window, div, px,
};

use crate::backend::Backend;
use crate::images::Images;
use crate::state::{AppState, Friend};
use crate::theme::{PaletteExt, radius, space, type_scale};
use crate::ui::components::{Button, chip, icon, thumb};
use crate::ui::create_instance::DialogEvent;

/// Invite links last a week.
const LINK_DAYS: i32 = 7;

fn frame(
    title: SharedString,
    body: AnyElement,
    cx: &mut Context<impl EventEmitter<DialogEvent>>,
) -> gpui_kit::Div {
    let p = cx.palette().clone();
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
                        .child(title),
                )
                .child(
                    Button::new("share-close")
                        .icon("x")
                        .ghost()
                        .small()
                        .on_click(
                            cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(DialogEvent::Close)),
                        ),
                ),
        )
        .child(div().px(space(6.0)).pb(space(6.0)).child(body))
}

#[derive(Clone)]
struct Member {
    id: String,
    username: SharedString,
    avatar_url: Option<String>,
    owner: bool,
}

/// Share an instance (as its owner): publish, invite, manage members.
pub struct ShareDialog {
    instance_id: String,
    name: SharedString,
    shared: bool,
    members: Option<Vec<Member>>,
    /// Local changes not published yet.
    changes: Option<usize>,
    link: Option<String>,
    busy: Option<&'static str>,
    error: Option<SharedString>,
}

impl EventEmitter<DialogEvent> for ShareDialog {}

impl ShareDialog {
    pub fn new(instance_id: String, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx).read(cx);
        let inst = state.instance(&instance_id).cloned();
        let mut dialog = ShareDialog {
            name: inst.as_ref().map(|i| i.name.clone()).unwrap_or_default(),
            shared: inst.as_ref().is_some_and(|i| i.shared.is_some()),
            instance_id,
            members: None,
            changes: None,
            link: None,
            busy: None,
            error: None,
        };
        dialog.load(cx);
        dialog
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        if !self.shared {
            return;
        }
        let id = self.instance_id.clone();
        let task = Backend::global(cx).call(async move {
            let users = theseus::instance::get_shared_instance_users(&id).await?;
            let ids: Vec<&str> = users.users.iter().map(|u| u.id.as_str()).collect();
            let profiles = theseus::cache::get_user_many(&ids, None)
                .await
                .unwrap_or_default();
            let members: Vec<Member> = users
                .users
                .iter()
                .map(|u| {
                    let profile = profiles.iter().find(|p| p.id == u.id);
                    Member {
                        id: u.id.clone(),
                        username: profile
                            .map(|p| p.username.clone())
                            .unwrap_or_else(|| u.id.clone())
                            .into(),
                        avatar_url: profile.and_then(|p| p.avatar_url.clone()),
                        owner: matches!(
                            u.join_type,
                            theseus::instance::SharedInstanceJoinType::Owner
                        ),
                    }
                })
                .collect();
            let changes = theseus::instance::get_shared_instance_publish_preview(&id)
                .await
                .ok()
                .flatten()
                .map(|p| p.diffs.len());
            Ok((members, changes))
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok((members, changes)) => {
                        this.members = Some(members);
                        this.changes = changes;
                    }
                    Err(error) => this.error = Some(format!("{error:#}").into()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Run a backend call, then reload. `after` runs on success.
    fn act<T: Send + 'static>(
        &mut self,
        label: &'static str,
        task: impl Future<Output = anyhow::Result<T>> + 'static,
        after: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        self.busy = Some(label);
        self.error = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = None;
                match result {
                    Ok(value) => after(this, value, cx),
                    Err(error) => this.error = Some(format!("{error:#}").into()),
                }
                this.load(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// First share, or publishing changes later. Only Modrinth content is
    /// shared; local files stay on this computer.
    fn publish(&mut self, cx: &mut Context<Self>) {
        let id = self.instance_id.clone();
        let first = !self.shared;
        let task = Backend::global(cx).call(async move {
            theseus::instance::publish_shared_instance(&id, Vec::new(), &|_, _| {})
                .await
                .map(|_| ())
        });
        self.act(
            if first { "Sharing…" } else { "Publishing…" },
            task,
            |this, (), cx| {
                this.shared = true;
                AppState::global(cx).update(cx, |s, cx| s.refresh_all(cx));
            },
            cx,
        );
    }

    fn make_link(&mut self, cx: &mut Context<Self>) {
        let id = self.instance_id.clone();
        let task = Backend::global(cx).call(async move {
            theseus::instance::create_shared_instance_invite_link(
                &id,
                Some(LINK_DAYS * 24 * 3600),
                None,
                None,
            )
            .await
        });
        self.act(
            "Creating link…",
            task,
            |this, link, cx| {
                let url = format!("https://modrinth.com/share/{}", link.invite_id);
                cx.write_to_clipboard(ClipboardItem::new_string(url.clone()));
                this.link = Some(url);
            },
            cx,
        );
    }

    fn invite(&mut self, user_id: String, cx: &mut Context<Self>) {
        let id = self.instance_id.clone();
        let task = Backend::global(cx).call(async move {
            theseus::instance::invite_shared_instance_users(&id, vec![user_id])
                .await
                .map(|_| ())
        });
        self.act("Inviting…", task, |_, (), _| {}, cx);
    }

    fn remove(&mut self, user_id: String, cx: &mut Context<Self>) {
        let id = self.instance_id.clone();
        let task = Backend::global(cx).call(async move {
            theseus::instance::remove_shared_instance_users(&id, vec![user_id], false)
                .await
                .map(|_| ())
        });
        self.act("Removing…", task, |_, (), _| {}, cx);
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        let id = self.instance_id.clone();
        let task = Backend::global(cx)
            .call(async move { theseus::instance::unpublish_shared_instance(&id).await });
        self.act(
            "Stopping…",
            task,
            |_, (), cx| {
                AppState::global(cx).update(cx, |s, cx| s.refresh_all(cx));
                cx.emit(DialogEvent::Close);
            },
            cx,
        );
    }

    fn person(
        &self,
        ix: usize,
        name: SharedString,
        avatar: Option<String>,
        note: &'static str,
        action: Option<AnyElement>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = cx.palette().clone();
        let image = avatar.as_deref().and_then(|u| Images::get(u, 48, cx));
        div()
            .flex()
            .items_center()
            .gap(space(2.5))
            .h(px(40.0))
            .when(ix > 0, |d| d.border_t_1().border_color(p.border))
            .child(thumb(image, &name, 24.0, radius::SM, cx))
            .child(
                div()
                    .flex_1()
                    .truncate()
                    .text_size(px(type_scale::BODY))
                    .text_color(p.text)
                    .child(name),
            )
            .when(!note.is_empty(), |d| {
                d.child(
                    div()
                        .text_size(px(type_scale::SMALL))
                        .text_color(p.text_muted)
                        .child(note),
                )
            })
            .children(action)
            .into_any_element()
    }
}

impl Render for ShareDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();
        let busy = self.busy;
        let note = |text: &'static str| {
            div()
                .text_size(px(type_scale::SMALL))
                .text_color(p.text_secondary)
                .child(text)
        };
        let mut body = div().flex().flex_col().gap(space(4.0));

        if !self.shared {
            body = body
                .child(note("Friends you invite get this instance in their launcher, with the same mods. When you change it, you publish the changes and they can update with one click."))
                .child(note("Only content from Modrinth is shared. Your worlds, settings and other files stay on this computer."))
                .child(
                    div().flex().justify_end().child(
                        Button::new("share-publish")
                            .icon("share-2")
                            .label(busy.unwrap_or("Share instance"))
                            .primary()
                            .disabled(busy.is_some())
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.publish(cx))),
                    ),
                );
        } else {
            // Invite link.
            let link_row = div()
                .flex()
                .items_center()
                .gap(space(2.0))
                .child(match &self.link {
                    Some(url) => div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .px(space(2.5))
                        .py(space(1.5))
                        .rounded(px(radius::SM))
                        .bg(p.raised)
                        .text_size(px(type_scale::SMALL))
                        .text_color(p.text)
                        .child(url.clone())
                        .into_any_element(),
                    None => note("Anyone with the link can join for the next 7 days.")
                        .flex_1()
                        .into_any_element(),
                })
                .child(
                    Button::new("share-link")
                        .icon(if self.link.is_some() { "check" } else { "link" })
                        .label(if self.link.is_some() {
                            "Copied"
                        } else {
                            "Copy invite link"
                        })
                        .small()
                        .disabled(busy.is_some())
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.make_link(cx))),
                );
            body = body.child(link_row);

            // Changes to publish.
            if let Some(n) = self.changes.filter(|n| *n > 0) {
                body = body.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space(3.0))
                        .p(space(3.0))
                        .rounded(px(radius::MD))
                        .bg(p.accent_soft)
                        .child(icon("refresh-cw", 16.0, p.accent))
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(type_scale::SMALL))
                                .text_color(p.text)
                                .child(format!(
                                    "{n} change{} since you last published.",
                                    if n == 1 { "" } else { "s" }
                                )),
                        )
                        .child(
                            Button::new("share-publish-changes")
                                .label(busy.unwrap_or("Publish"))
                                .small()
                                .primary()
                                .disabled(busy.is_some())
                                .on_click(
                                    cx.listener(|this, _: &ClickEvent, _, cx| this.publish(cx)),
                                ),
                        ),
                );
            }

            // Members.
            let members = self.members.clone().unwrap_or_default();
            let mut people = div().flex().flex_col();
            for (ix, m) in members.iter().enumerate() {
                let remove_id = m.id.clone();
                let action = (!m.owner).then(|| {
                    Button::new(SharedString::from(format!("share-remove-{ix}")))
                        .label("Remove")
                        .ghost()
                        .small()
                        .disabled(busy.is_some())
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.remove(remove_id.clone(), cx)
                        }))
                        .into_any_element()
                });
                people = people.child(self.person(
                    ix,
                    m.username.clone(),
                    m.avatar_url.clone(),
                    if m.owner { "Owner" } else { "" },
                    action,
                    cx,
                ));
            }
            // Friends who aren't in yet.
            let friends: Vec<Friend> = AppState::global(cx)
                .read(cx)
                .friends
                .iter()
                .filter(|f| f.accepted && !members.iter().any(|m| m.id == f.user_id))
                .cloned()
                .collect();
            let offset = members.len();
            for (ix, f) in friends.iter().enumerate() {
                let id = f.user_id.clone();
                let action = Button::new(SharedString::from(format!("share-invite-{ix}")))
                    .icon("user-plus")
                    .label("Invite")
                    .small()
                    .disabled(busy.is_some())
                    .on_click(
                        cx.listener(move |this, _: &ClickEvent, _, cx| this.invite(id.clone(), cx)),
                    )
                    .into_any_element();
                people = people.child(self.person(
                    offset + ix,
                    f.username.clone(),
                    f.avatar_url.clone(),
                    "",
                    Some(action),
                    cx,
                ));
            }
            body = body
                .child(
                    div()
                        .text_size(px(type_scale::CAPTION))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(p.text_muted)
                        .child("People"),
                )
                .child(people)
                .child(
                    div().flex().justify_start().child(
                        Button::new("share-stop")
                            .label("Stop sharing")
                            .ghost()
                            .small()
                            .disabled(busy.is_some())
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.stop(cx))),
                    ),
                );
        }
        if let Some(error) = self.error.clone() {
            body = body.child(
                div()
                    .text_size(px(type_scale::SMALL))
                    .text_color(p.danger)
                    .child(error),
            );
        }
        frame(
            format!("Share {}", self.name).into(),
            body.into_any_element(),
            cx,
        )
    }
}

/// The invite id from a link like `https://modrinth.com/share/<id>`, a
/// `modrinth://` link, or the bare id.
pub fn invite_id(text: &str) -> Option<String> {
    let text = text.trim().trim_end_matches('/');
    let id = text.rsplit('/').next()?.split(['?', '#']).next()?;
    (!id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
    .then(|| id.to_string())
}

/// Join a shared instance from an invite link.
pub struct JoinDialog {
    link: Entity<InputState>,
    preview: Option<(
        String,
        theseus::instance::SharedInstanceInviteInstallPreview,
    )>,
    busy: Option<&'static str>,
    error: Option<SharedString>,
}

impl EventEmitter<DialogEvent> for JoinDialog {}

impl JoinDialog {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let link =
            cx.new(|cx| InputState::new(window, cx).placeholder("https://modrinth.com/share/…"));
        cx.subscribe_in(
            &link,
            window,
            |this, _, event: &InputEvent, _, cx| match event {
                InputEvent::PressEnter { .. } => this.look_up(cx),
                InputEvent::Change => {
                    this.preview = None;
                    this.error = None;
                    cx.notify();
                }
                _ => {}
            },
        )
        .detach();
        link.update(cx, |input, cx| input.focus(window, cx));
        JoinDialog {
            link,
            preview: None,
            busy: None,
            error: None,
        }
    }

    fn look_up(&mut self, cx: &mut Context<Self>) {
        let Some(invite) = invite_id(&self.link.read(cx).value()) else {
            self.error = Some("That doesn't look like an invite link.".into());
            cx.notify();
            return;
        };
        self.busy = Some("Looking…");
        cx.notify();
        let lookup = invite.clone();
        let task = Backend::global(cx).call(async move {
            theseus::instance::get_shared_instance_invite_install_preview(&lookup).await
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = None;
                match result {
                    Ok(preview) => this.preview = Some((invite, preview)),
                    Err(_) => {
                        this.error = Some(
                            "This invite has expired or doesn't exist. Ask for a new link.".into(),
                        )
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn install(&mut self, cx: &mut Context<Self>) {
        let Some((invite, preview)) = self.preview.clone() else {
            return;
        };
        self.busy = Some("Joining…");
        cx.notify();
        let task = Backend::global(cx).call(async move {
            let shared = preview.shared_instance_id.clone();
            theseus::instance::accept_shared_instance_invite_for_install(&shared, &invite).await?;
            theseus::instance::install_shared_instance(
                &shared,
                preview.preview.name.clone(),
                preview.manager_id.clone(),
                preview.server_manager_name.clone(),
                preview.server_manager_icon_url.clone(),
                preview.instance_icon_url.clone(),
                preview.instance_icon_config.clone(),
            )
            .await
            .map(|_| ())
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = None;
                match result {
                    Ok(()) => {
                        AppState::global(cx).update(cx, |s, cx| s.refresh_all(cx));
                        cx.emit(DialogEvent::Close);
                    }
                    Err(error) => this.error = Some(format!("{error:#}").into()),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for JoinDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();
        let busy = self.busy;
        let mut body = div()
            .flex()
            .flex_col()
            .gap(space(4.0))
            .child(
                div()
                    .text_size(px(type_scale::SMALL))
                    .text_color(p.text_secondary)
                    .child("Paste the invite link a friend sent you. Calcite installs their instance and keeps it in sync when they change it."),
            )
            .child(Input::new(&self.link));
        if let Some((_, preview)) = &self.preview {
            let icon_src = preview
                .instance_icon_url
                .as_deref()
                .and_then(|u| Images::get(u, 96, cx));
            let from = preview.inviter.as_ref().map(|i| i.name.clone());
            let what = &preview.preview;
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .gap(space(3.0))
                    .p(space(3.0))
                    .rounded(px(radius::MD))
                    .bg(p.raised)
                    .child(thumb(icon_src, &what.name, 40.0, radius::MD, cx))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .gap(space(1.0))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(type_scale::BODY))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.text)
                                    .child(what.name.clone()),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap(space(1.5))
                                    .child(chip(
                                        format!(
                                            "{} {}",
                                            crate::state::loader_name(what.loader),
                                            what.game_version
                                        ),
                                        cx,
                                    ))
                                    .child(chip(format!("{} mods", what.mod_count), cx)),
                            )
                            .when_some(from, |d, from| {
                                d.child(
                                    div()
                                        .text_size(px(type_scale::SMALL))
                                        .text_color(p.text_muted)
                                        .child(format!("Shared by {from}")),
                                )
                            }),
                    ),
            );
        }
        if let Some(error) = self.error.clone() {
            body = body.child(
                div()
                    .text_size(px(type_scale::SMALL))
                    .text_color(p.danger)
                    .child(error),
            );
        }
        let ready = self.preview.is_some();
        body = body.child(
            div().flex().justify_end().child(
                Button::new("join-go")
                    .label(busy.unwrap_or(if ready { "Join and install" } else { "Look up" }))
                    .primary()
                    .disabled(busy.is_some())
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        if ready {
                            this.install(cx)
                        } else {
                            this.look_up(cx)
                        }
                    })),
            ),
        );
        frame("Join a shared instance".into(), body.into_any_element(), cx)
    }
}

#[cfg(test)]
mod tests {
    use super::invite_id;

    #[test]
    fn reads_invite_ids_from_links() {
        assert_eq!(
            invite_id("https://modrinth.com/share/AbC123").as_deref(),
            Some("AbC123")
        );
        assert_eq!(
            invite_id(" https://modrinth.com/share/AbC123/ ").as_deref(),
            Some("AbC123")
        );
        assert_eq!(
            invite_id("https://modrinth.com/share/AbC123?ref=x").as_deref(),
            Some("AbC123")
        );
        assert_eq!(invite_id("AbC123").as_deref(), Some("AbC123"));
        assert_eq!(invite_id("not a link!"), None);
        assert_eq!(invite_id(""), None);
    }
}
