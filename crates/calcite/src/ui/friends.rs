//! Friends: the sidebar list (requests, who's playing, who's online) and the
//! "Add friend" dialog.

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, ClickEvent, Context, Entity, EventEmitter, FontWeight, SharedString, Window, div,
    px,
};

use crate::backend::Backend;
use crate::images::Images;
use crate::state::{AppState, Friend};
use crate::theme::{PaletteExt, radius, space, type_scale};
use crate::ui::components::{Button, icon, thumb};
use crate::ui::create_instance::DialogEvent;
use crate::ui::shell::{Route, Shell};

/// Offline friends shown before "N more".
const OFFLINE_SHOWN: usize = 3;

/// The sidebar section. Nothing when signed out of Modrinth.
pub fn sidebar_section(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let p = cx.palette().clone();
    let state = shell.state().read(cx);
    state.modrinth.as_ref()?;
    let friends = state.friends.clone();
    let online = friends.iter().filter(|f| f.accepted && f.online).count();

    let header =
        div()
            .flex()
            .items_center()
            .justify_between()
            .px(space(2.5))
            .pb(space(1.0))
            .child(
                div()
                    .text_size(px(type_scale::CAPTION))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(p.text_muted)
                    .child(if online > 0 {
                        format!("Friends · {online} online")
                    } else {
                        "Friends".to_string()
                    }),
            )
            .child(
                div()
                    .id("friends-add")
                    .size(px(20.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(radius::SM))
                    .cursor_pointer()
                    .hover(|s| s.bg(p.hover))
                    .child(icon("user-plus", 13.0, p.text_muted))
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.open_add_friend(window, cx)
                    })),
            );

    let mut list = div().flex().flex_col().gap(space(0.5));
    let offline: Vec<&Friend> = friends.iter().filter(|f| f.accepted && !f.online).collect();
    let mut shown = 0;
    for (ix, friend) in friends.iter().enumerate() {
        if friend.accepted && !friend.online {
            if shown >= OFFLINE_SHOWN && !shell.all_friends_shown() {
                continue;
            }
            shown += 1;
        }
        list = list.child(row(friend, ix, cx));
    }
    if friends.is_empty() {
        list = list.child(
            div()
                .px(space(2.5))
                .py(space(1.0))
                .text_size(px(type_scale::SMALL))
                .text_color(p.text_muted)
                .child("No friends yet. Add someone by their Modrinth username."),
        );
    }
    let hidden = offline.len().saturating_sub(OFFLINE_SHOWN);
    if hidden > 0 {
        let all = shell.all_friends_shown();
        list = list.child(
            div()
                .id("friends-more")
                .px(space(2.5))
                .py(space(1.0))
                .text_size(px(type_scale::SMALL))
                .text_color(p.text_muted)
                .cursor_pointer()
                .hover(|s| s.text_color(p.text_secondary))
                .child(if all {
                    "Show fewer".to_string()
                } else {
                    format!("{hidden} more offline")
                })
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_all_friends(cx))),
        );
    }

    // Shrinks and scrolls rather than pushing the account row off screen.
    Some(
        div()
            .flex()
            .flex_col()
            .flex_shrink(1.0)
            .min_h_0()
            .gap(space(1.0))
            .px(space(2.0))
            .pt(space(3.0))
            .child(header)
            .child(
                div()
                    .id("friends-scroll")
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(list),
            )
            .into_any_element(),
    )
}

fn row(friend: &Friend, ix: usize, cx: &mut Context<Shell>) -> AnyElement {
    let p = cx.palette().clone();
    let avatar = friend
        .avatar_url
        .as_deref()
        .and_then(|u| Images::get(u, 48, cx));
    let status: SharedString = match (
        friend.accepted,
        friend.incoming,
        friend.online,
        &friend.playing,
    ) {
        (false, true, ..) => "Friend request".into(),
        (false, false, ..) => "Request sent".into(),
        (true, true, ..) | (true, false, true, _) if friend.playing.is_some() => {
            format!("Playing {}", friend.playing.clone().unwrap_or_default()).into()
        }
        (true, _, true, _) => "Online".into(),
        _ => "Offline".into(),
    };
    let open_id = friend.user_id.clone();
    let accept_id = friend.user_id.clone();
    let decline_id = friend.user_id.clone();
    let dim = friend.accepted && !friend.online;
    div()
        .id(SharedString::from(format!("friend-{ix}")))
        .flex()
        .items_center()
        .gap(space(2.5))
        .h(px(40.0))
        .px(space(2.5))
        .rounded(px(radius::SM))
        .cursor_pointer()
        .hover(|s| s.bg(p.hover))
        .when(dim, |d| d.opacity(0.6))
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            this.navigate(Route::User(open_id.clone()), window, cx)
        }))
        .child(
            div()
                .relative()
                .child(thumb(avatar, &friend.username, 24.0, radius::SM, cx))
                .when(friend.online, |d| {
                    d.child(
                        div()
                            .absolute()
                            .bottom(px(-2.0))
                            .right(px(-2.0))
                            .size(px(9.0))
                            .rounded_full()
                            .border_2()
                            .border_color(p.rail)
                            .bg(p.running),
                    )
                }),
        )
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
                        .text_color(p.text)
                        .child(friend.username.clone()),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(px(type_scale::CAPTION))
                        .text_color(if friend.playing.is_some() {
                            p.running
                        } else {
                            p.text_muted
                        })
                        .child(status),
                ),
        )
        .when(!friend.accepted && friend.incoming, |d| {
            d.child(
                Button::new(SharedString::from(format!("friend-accept-{ix}")))
                    .icon("check")
                    .ghost()
                    .small()
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        let id = accept_id.clone();
                        this.state().update(cx, |s, cx| s.add_friend(id, cx));
                    })),
            )
        })
        .when(!friend.accepted, |d| {
            d.child(
                Button::new(SharedString::from(format!("friend-decline-{ix}")))
                    .icon("x")
                    .ghost()
                    .small()
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        let id = decline_id.clone();
                        this.state().update(cx, |s, cx| s.remove_friend(id, cx));
                    })),
            )
        })
        .into_any_element()
}

/// "Add friend": type a Modrinth username, Calcite sends the request.
pub struct AddFriendDialog {
    name: Entity<InputState>,
    error: Option<SharedString>,
    sending: bool,
}

impl EventEmitter<DialogEvent> for AddFriendDialog {}

impl AddFriendDialog {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("Modrinth username"));
        cx.subscribe_in(
            &name,
            window,
            |this, _, event: &InputEvent, _, cx| match event {
                InputEvent::PressEnter { .. } => this.send(cx),
                InputEvent::Change => {
                    this.error = None;
                    cx.notify();
                }
                _ => {}
            },
        )
        .detach();
        name.update(cx, |input, cx| input.focus(window, cx));
        AddFriendDialog {
            name,
            error: None,
            sending: false,
        }
    }

    fn send(&mut self, cx: &mut Context<Self>) {
        let username = self.name.read(cx).value().trim().to_string();
        if username.is_empty() || self.sending {
            return;
        }
        self.sending = true;
        cx.notify();
        let task = Backend::global(cx).call(async move {
            // The user endpoint takes a username as well as an id.
            let profile = theseus::users::get_user_profile(&username).await?;
            let id = profile
                .get("id")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .ok_or_else(|| {
                    theseus::ErrorKind::FSError("No Modrinth user has that name.".into())
                })?;
            theseus::friends::add_friend(&id).await
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.sending = false;
                match result {
                    Ok(()) => {
                        AppState::global(cx).update(cx, |s, cx| s.refresh_friends(cx));
                        cx.emit(DialogEvent::Close);
                    }
                    Err(_) => this.error = Some(
                        "Couldn't find that user. Check the spelling of their Modrinth username."
                            .into(),
                    ),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for AddFriendDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();
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
                            .child("Add a friend"),
                    )
                    .child(
                        Button::new("add-friend-close")
                            .icon("x")
                            .ghost()
                            .small()
                            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(DialogEvent::Close))),
                    ),
            )
            .child(
                div()
                    .text_size(px(type_scale::SMALL))
                    .text_color(p.text_secondary)
                    .child("They'll see your request in their launcher or on modrinth.com. Once they accept, you'll see when they're online and what they're playing."),
            )
            .child(Input::new(&self.name))
            .when_some(self.error.clone(), |d, e| {
                d.child(div().text_size(px(type_scale::SMALL)).text_color(p.danger).child(e))
            })
            .child(
                div().flex().justify_end().child(
                    Button::new("add-friend-send")
                        .label(if self.sending { "Sending…" } else { "Send request" })
                        .primary()
                        .disabled(self.sending)
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.send(cx))),
                ),
            )
    }
}
