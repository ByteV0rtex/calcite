//! A Modrinth user's page: who they are, what they've made, and whether
//! you're friends.

use chrono::{DateTime, Utc};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, ClickEvent, Context, Entity, EventEmitter, FontWeight, SharedString, Window, div,
    px,
};
use serde_json::Value;

use crate::backend::Backend;
use crate::images::Images;
use crate::state::AppState;
use crate::theme::{PaletteExt, radius, space, type_scale};
use crate::ui::components::{
    Button, chip, compact_number, empty_state, icon_chip, panel, relative_time, section_heading,
    skeleton, state_chip, thumb,
};

pub enum UserEvent {
    OpenProject(String),
}

#[derive(Clone)]
struct Profile {
    id: String,
    username: SharedString,
    avatar_url: Option<String>,
    bio: Option<SharedString>,
    joined: Option<DateTime<Utc>>,
}

#[derive(Clone)]
struct Made {
    id: String,
    title: SharedString,
    summary: SharedString,
    icon_url: Option<String>,
    downloads: u64,
    kind: SharedString,
}

pub struct UserPage {
    id: String,
    state: Entity<AppState>,
    profile: Option<Result<Profile, String>>,
    projects: Option<Vec<Made>>,
}

impl EventEmitter<UserEvent> for UserPage {}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

impl UserPage {
    pub fn new(id: String, cx: &mut Context<Self>) -> Self {
        let page = UserPage {
            id: id.clone(),
            state: AppState::global(cx),
            profile: None,
            projects: None,
        };
        let task = Backend::global(cx).call(async move {
            let profile = theseus::users::get_user_profile(&id).await?;
            let projects = theseus::users::get_user_projects(&id)
                .await
                .unwrap_or(Value::Null);
            Ok((profile, projects))
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok((profile, projects)) => {
                        this.profile = Some(Ok(Profile {
                            id: text(&profile, "id").unwrap_or_else(|| this.id.clone()),
                            username: text(&profile, "username")
                                .unwrap_or_else(|| this.id.clone())
                                .into(),
                            avatar_url: text(&profile, "avatar_url"),
                            bio: text(&profile, "bio").map(Into::into),
                            joined: text(&profile, "created").and_then(|s| s.parse().ok()),
                        }));
                        let mut made: Vec<Made> = projects
                            .as_array()
                            .map(|list| {
                                list.iter()
                                    .map(|p| Made {
                                        id: text(p, "slug")
                                            .or_else(|| text(p, "id"))
                                            .unwrap_or_default(),
                                        title: text(p, "name")
                                            .or_else(|| text(p, "title"))
                                            .unwrap_or_default()
                                            .into(),
                                        summary: text(p, "summary")
                                            .or_else(|| text(p, "description"))
                                            .unwrap_or_default()
                                            .into(),
                                        icon_url: text(p, "icon_url"),
                                        downloads: p
                                            .get("downloads")
                                            .and_then(Value::as_u64)
                                            .unwrap_or(0),
                                        kind: p
                                            .get("project_types")
                                            .and_then(|t| t.get(0))
                                            .and_then(Value::as_str)
                                            .or_else(|| {
                                                p.get("project_type").and_then(Value::as_str)
                                            })
                                            .map(kind_label)
                                            .unwrap_or("Project")
                                            .into(),
                                    })
                                    .filter(|m| !m.id.is_empty())
                                    .collect()
                            })
                            .unwrap_or_default();
                        made.sort_by_key(|m| std::cmp::Reverse(m.downloads));
                        this.projects = Some(made);
                    }
                    Err(error) => this.profile = Some(Err(format!("{error:#}"))),
                }
                cx.notify();
            });
        })
        .detach();
        page
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    fn friend_action(&self, profile: &Profile, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = cx.palette().clone();
        let state = self.state.read(cx);
        let me = state.modrinth.as_ref()?;
        if me.id == profile.id {
            return Some(chip("This is you", cx).into_any_element());
        }
        let friend = state
            .friends
            .iter()
            .find(|f| f.user_id == profile.id)
            .cloned();
        let id = profile.id.clone();
        let element = match friend {
            Some(f) if f.accepted => div()
                .flex()
                .items_center()
                .gap(space(2.0))
                .child(match &f.playing {
                    Some(what) if f.online => {
                        state_chip(format!("Playing {what}"), p.running, p.running_soft)
                    }
                    _ if f.online => state_chip("Online", p.running, p.running_soft),
                    _ => state_chip("Friends", p.text_secondary, p.raised),
                })
                .child(
                    Button::new("user-unfriend")
                        .label("Remove friend")
                        .ghost()
                        .small()
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            let id = id.clone();
                            this.state.update(cx, |s, cx| s.remove_friend(id, cx));
                        })),
                )
                .into_any_element(),
            Some(f) if f.incoming => Button::new("user-accept")
                .icon("check")
                .label("Accept friend request")
                .primary()
                .small()
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    let id = id.clone();
                    this.state.update(cx, |s, cx| s.add_friend(id, cx));
                }))
                .into_any_element(),
            Some(_) => Button::new("user-cancel")
                .label("Request sent · Cancel")
                .small()
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    let id = id.clone();
                    this.state.update(cx, |s, cx| s.remove_friend(id, cx));
                }))
                .into_any_element(),
            None => Button::new("user-add")
                .icon("user-plus")
                .label("Add friend")
                .primary()
                .small()
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    let id = id.clone();
                    this.state.update(cx, |s, cx| s.add_friend(id, cx));
                }))
                .into_any_element(),
        };
        Some(element)
    }
}

fn kind_label(kind: &str) -> &'static str {
    match kind {
        "mod" => "Mod",
        "modpack" => "Modpack",
        "resourcepack" => "Resource pack",
        "shader" => "Shader",
        "datapack" => "Data pack",
        "plugin" => "Plugin",
        "minecraft_java_server" | "server" => "Server",
        _ => "Project",
    }
}

impl Render for UserPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();
        let profile = match &self.profile {
            None => {
                return div()
                    .p(space(8.0))
                    .flex()
                    .gap(space(4.0))
                    .child(skeleton(px(72.0), px(72.0), cx))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(space(2.0))
                            .child(skeleton(px(180.0), px(24.0), cx))
                            .child(skeleton(px(320.0), px(14.0), cx)),
                    )
                    .into_any_element();
            }
            Some(Err(error)) => {
                return div()
                    .size_full()
                    .child(empty_state(
                        "triangle-alert",
                        "Couldn't load this profile",
                        format!("Check your connection and try again. ({error})"),
                        None,
                        cx,
                    ))
                    .into_any_element();
            }
            Some(Ok(profile)) => profile.clone(),
        };
        let avatar = profile
            .avatar_url
            .as_deref()
            .and_then(|u| Images::get(u, 144, cx));
        let action = self.friend_action(&profile, cx);
        let total: u64 = self
            .projects
            .as_ref()
            .map(|m| m.iter().map(|x| x.downloads).sum())
            .unwrap_or(0);

        let mut list = panel(cx).overflow_hidden();
        let projects = self.projects.clone().unwrap_or_default();
        for (ix, made) in projects.iter().enumerate() {
            let icon_src = made
                .icon_url
                .as_deref()
                .and_then(|u| Images::get(u, 96, cx));
            let open = made.id.clone();
            list = list.child(
                div()
                    .id(SharedString::from(format!("made-{ix}")))
                    .flex()
                    .items_center()
                    .gap(space(3.0))
                    .px(space(4.0))
                    .py(space(3.0))
                    .when(ix > 0, |d| d.border_t_1().border_color(p.border))
                    .cursor_pointer()
                    .hover(|s| s.bg(p.hover))
                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                        cx.emit(UserEvent::OpenProject(open.clone()))
                    }))
                    .child(thumb(icon_src, &made.title, 40.0, radius::MD, cx))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .gap(space(0.5))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(space(2.0))
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(px(type_scale::BODY))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(p.text)
                                            .child(made.title.clone()),
                                    )
                                    .child(chip(made.kind.clone(), cx)),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(type_scale::SMALL))
                                    .text_color(p.text_secondary)
                                    .child(made.summary.clone()),
                            ),
                    )
                    .child(icon_chip("download", compact_number(made.downloads), cx)),
            );
        }

        div()
            .id("user-page")
            .size_full()
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(space(8.0))
                    .px(space(8.0))
                    .py(space(7.0))
                    .max_w(px(900.0))
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .gap(space(4.0))
                            .child(thumb(avatar, &profile.username, 72.0, radius::LG, cx))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_w_0()
                                    .gap(space(1.5))
                                    .child(
                                        div()
                                            .text_size(px(type_scale::HEADLINE))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(p.text)
                                            .child(profile.username.clone()),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(space(1.5))
                                            .when_some(profile.joined, |d, t| {
                                                d.child(icon_chip(
                                                    "calendar-clock",
                                                    format!("Joined {}", relative_time(t)),
                                                    cx,
                                                ))
                                            })
                                            .when(!projects.is_empty(), |d| {
                                                d.child(icon_chip(
                                                    "package",
                                                    format!(
                                                        "{} project{}",
                                                        projects.len(),
                                                        if projects.len() == 1 { "" } else { "s" }
                                                    ),
                                                    cx,
                                                ))
                                                .child(icon_chip(
                                                    "download",
                                                    format!("{} downloads", compact_number(total)),
                                                    cx,
                                                ))
                                            }),
                                    )
                                    .when_some(profile.bio.clone(), |d, bio| {
                                        d.child(
                                            div()
                                                .pt(space(1.0))
                                                .text_size(px(type_scale::BODY))
                                                .text_color(p.text_secondary)
                                                .child(bio),
                                        )
                                    }),
                            )
                            .children(action),
                    )
                    .when(!projects.is_empty(), |d| {
                        d.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(space(3.0))
                                .child(section_heading("Projects", cx))
                                .child(list),
                        )
                    }),
            )
            .into_any_element()
    }
}
