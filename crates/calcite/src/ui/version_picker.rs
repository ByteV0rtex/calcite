//! Pick another version of something: the instance's modpack, or one
//! installed mod or pack. A list on the left, the changelog on the right.

use calcite_core::markup::clean_description;
use calcite_core::modrinth::Version;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::text::TextView;
use gpui_kit::prelude::*;
use gpui_kit::{
    ClickEvent, Context, Entity, EventEmitter, FontWeight, SharedString, Window, div, px,
};
use theseus::data::ProjectType;

use crate::backend::Backend;
use crate::images::Images;
use crate::state::AppState;
use crate::theme::{PaletteExt, radius, space, type_scale};
use crate::ui::components::{Button, empty_state, relative_time, skeleton, thumb};
use crate::ui::create_instance::DialogEvent;
use crate::ui::pages::discover::Api;

/// One installed file whose version can change.
#[derive(Clone, Debug)]
pub struct PickFile {
    pub path: String,
    pub project_id: String,
    pub title: String,
    pub icon_url: Option<String>,
    pub current: Option<String>,
    pub kind: ProjectType,
}

enum Target {
    Modpack,
    File { path: String, kind: ProjectType },
}

pub struct VersionPicker {
    instance_id: String,
    target: Target,
    project_id: Option<String>,
    title: SharedString,
    icon_url: Option<String>,
    current: Option<String>,
    game_version: String,
    loader: String,
    versions: Option<Result<Vec<Version>, String>>,
    selected: Option<String>,
    search: Entity<InputState>,
    /// Only versions made for this instance's Minecraft and loader.
    only_fitting: bool,
    working: bool,
    error: Option<SharedString>,
}

impl EventEmitter<DialogEvent> for VersionPicker {}

impl VersionPicker {
    /// Switch the instance's modpack to another version.
    pub fn modpack(instance_id: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self::empty(instance_id.clone(), Target::Modpack, window, cx);
        let id = instance_id;
        let info = Backend::global(cx)
            .call(async move { theseus::instance::get_linked_modpack_info(&id, None).await });
        cx.spawn(async move |this, cx| {
            let result = info.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(Some(info)) => {
                    this.title = info.project.title.clone().into();
                    this.icon_url = info.project.icon_url.clone();
                    this.current = info.version.as_ref().map(|v| v.id.clone());
                    this.project_id = Some(info.project.id.clone());
                    this.load_versions(info.project.id, cx);
                }
                Ok(None) => {
                    this.versions = Some(Err(
                        "This instance isn't linked to a Modrinth modpack anymore.".into(),
                    ));
                    cx.notify();
                }
                Err(error) => {
                    this.versions = Some(Err(format!("{error:#}")));
                    cx.notify();
                }
            });
        })
        .detach();
        this.only_fitting = false;
        this
    }

    /// Switch one installed file to another version of its project.
    pub fn file(
        instance_id: String,
        file: PickFile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self::empty(
            instance_id,
            Target::File {
                path: file.path,
                kind: file.kind,
            },
            window,
            cx,
        );
        this.title = file.title.into();
        this.icon_url = file.icon_url;
        this.current = file.current;
        this.project_id = Some(file.project_id.clone());
        this.load_versions(file.project_id, cx);
        this
    }

    fn empty(
        instance_id: String,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search versions"));
        cx.subscribe(&search, |this: &mut Self, _, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                this.ensure_selection(cx);
                cx.notify();
            }
        })
        .detach();
        let (game_version, loader) = AppState::global(cx)
            .read(cx)
            .instance(&instance_id)
            .map(|i| (i.game_version.to_string(), i.loader.as_str().to_string()))
            .unwrap_or_default();
        VersionPicker {
            instance_id,
            target,
            project_id: None,
            title: "".into(),
            icon_url: None,
            current: None,
            game_version,
            loader,
            versions: None,
            selected: None,
            search,
            only_fitting: true,
            working: false,
            error: None,
        }
    }

    fn load_versions(&mut self, project_id: String, cx: &mut Context<Self>) {
        let client = cx.global::<Api>().0.clone();
        let fetch = Backend::global(cx).run(async move { client.versions(&project_id).await });
        cx.spawn(async move |this, cx| {
            let result = fetch.await;
            let _ = this.update(cx, |this, cx| {
                this.versions = Some(match result {
                    Ok(Ok(versions)) => Ok(versions),
                    Ok(Err(error)) => Err(format!("{error:#}")),
                    Err(error) => Err(format!("{error:#}")),
                });
                this.ensure_selection(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn fits(&self, v: &Version) -> bool {
        let Target::File { kind, .. } = &self.target else {
            return true;
        };
        if !self.only_fitting {
            return true;
        }
        let game = v.game_versions.contains(&self.game_version);
        let loader = *kind != ProjectType::Mod
            || v.loaders.contains(&self.loader)
            || (self.loader == "quilt" && v.loaders.iter().any(|l| l == "fabric"));
        game && loader
    }

    fn visible(&self, cx: &App) -> Vec<Version> {
        let query = self.search.read(cx).value().trim().to_lowercase();
        let Some(Ok(versions)) = &self.versions else {
            return Vec::new();
        };
        versions
            .iter()
            .filter(|v| self.fits(v))
            .filter(|v| {
                query.is_empty()
                    || v.version_number.to_lowercase().contains(&query)
                    || v.name.to_lowercase().contains(&query)
            })
            .cloned()
            .collect()
    }

    /// Keep a sensible version selected: the newest one that isn't the
    /// current one, if the selection is gone.
    fn ensure_selection(&mut self, cx: &App) {
        let visible = self.visible(cx);
        if self
            .selected
            .as_ref()
            .is_some_and(|s| visible.iter().any(|v| v.id == *s))
        {
            return;
        }
        self.selected = visible
            .iter()
            .find(|v| Some(&v.id) != self.current.as_ref())
            .or(visible.first())
            .map(|v| v.id.clone());
    }

    fn apply(&mut self, cx: &mut Context<Self>) {
        let Some(version_id) = self.selected.clone() else {
            return;
        };
        if self.working || Some(&version_id) == self.current.as_ref() {
            return;
        }
        self.working = true;
        self.error = None;
        cx.notify();
        let id = self.instance_id.clone();
        let state = AppState::global(cx);
        match &self.target {
            Target::Modpack => {
                let snapshots = crate::ui::pages::instance_tabs::snapshots_dir(&id);
                let version_name = self
                    .versions
                    .as_ref()
                    .and_then(|r| r.as_ref().ok())
                    .and_then(|vs| vs.iter().find(|v| v.id == version_id))
                    .map(|v| v.version_number.clone())
                    .unwrap_or_default();
                let call = Backend::global(cx).call(async move {
                    // Keep a way back: the pack can change a lot.
                    if let Some(snaps) = snapshots
                        && let Ok(dir) = theseus::instance::get_full_path(&id).await
                    {
                        let label = format!("Before switching the modpack to {version_name}");
                        let _ = tokio::task::spawn_blocking(move || {
                            let result = calcite_core::snapshots::take(&dir, &snaps, &label, true);
                            calcite_core::snapshots::prune_automatic(&snaps, 10);
                            result
                        })
                        .await;
                    }
                    theseus::instance::update_managed_modrinth_version(&id, &version_id).await
                });
                cx.spawn(async move |this, cx| {
                    let result = call.await;
                    let ok = result.is_ok();
                    let message = result.as_ref().err().map(|e| format!("{e:#}"));
                    state.update(cx, |state, cx| {
                        if let Ok(job) = result {
                            state.track_job(job, cx);
                        }
                        cx.notify();
                    });
                    let _ = this.update(cx, |this, cx| {
                        this.working = false;
                        if ok {
                            cx.emit(DialogEvent::ContentChanged);
                        } else {
                            this.error =
                                message.map(|m| format!("Couldn't switch the modpack. {m}").into());
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
            Target::File { path, .. } => {
                let path = path.clone();
                let call = Backend::global(cx).call(async move {
                    theseus::instance::switch_project_version_with_dependencies(
                        &id,
                        &path,
                        &version_id,
                    )
                    .await
                });
                cx.spawn(async move |this, cx| {
                    let result = call.await;
                    let _ = this.update(cx, |this, cx| {
                        this.working = false;
                        match result {
                            Ok(_) => cx.emit(DialogEvent::ContentChanged),
                            Err(error) => {
                                this.error =
                                    Some(format!("Couldn't switch versions. {error:#}").into())
                            }
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
        }
    }
}

/// Short channel badge: R, B or A, colored like the rest of the app.
fn channel_badge(channel: &str, long: bool, cx: &App) -> impl IntoElement {
    let p = cx.palette();
    let (fg, bg, short, name) = match channel {
        "beta" => (p.warning, p.warning_soft, "B", "Beta"),
        "alpha" => (p.danger, p.danger_soft, "A", "Alpha"),
        _ => (p.running, p.running_soft, "R", "Release"),
    };
    div()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .when(!long, |d| d.size(px(18.0)))
        .when(long, |d| d.h(px(18.0)).px(space(1.5)))
        .rounded(px(radius::SM))
        .bg(bg)
        .text_size(px(type_scale::CAPTION))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(fg)
        .child(if long { name } else { short })
}

use gpui_kit::App;

impl Render for VersionPicker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();
        let visible = self.visible(cx);
        let selected = self
            .selected
            .as_ref()
            .and_then(|s| visible.iter().find(|v| v.id == *s))
            .cloned();
        let is_pack = matches!(self.target, Target::Modpack);

        let source = self
            .icon_url
            .as_deref()
            .and_then(|url| Images::get(url, 64, cx));
        let header =
            div()
                .flex()
                .items_center()
                .gap(space(3.0))
                .px(space(5.0))
                .py(space(4.0))
                .border_b_1()
                .border_color(p.border)
                .child(thumb(source, &self.title, 28.0, radius::SM, cx))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .text_size(px(type_scale::SECTION))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(p.text)
                                .child(if is_pack {
                                    "Switch modpack version"
                                } else {
                                    "Switch version"
                                }),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(px(type_scale::SMALL))
                                .text_color(p.text_muted)
                                .child(self.title.clone()),
                        ),
                )
                .child(
                    Button::new("vp-close").icon("x").ghost().small().on_click(
                        cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(DialogEvent::Close)),
                    ),
                );

        // Left: search and the list.
        let mut list = div()
            .id("vp-list")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .gap(space(0.5))
            .p(space(1.5));
        match &self.versions {
            None => {
                for _ in 0..8 {
                    list = list.child(div().p(space(1.5)).child(skeleton(px(160.0), px(14.0), cx)));
                }
            }
            Some(Err(_)) => {}
            Some(Ok(_)) if visible.is_empty() => {
                list = list.child(
                    div()
                        .p(space(3.0))
                        .text_size(px(type_scale::SMALL))
                        .text_color(p.text_muted)
                        .child("No versions match."),
                );
            }
            Some(Ok(_)) => {
                for (ix, v) in visible.iter().enumerate() {
                    let active = Some(&v.id) == self.selected.as_ref();
                    let current = Some(&v.id) == self.current.as_ref();
                    let pick = v.id.clone();
                    list = list.child(
                        div()
                            .id(SharedString::from(format!("vp-{ix}")))
                            .flex()
                            .items_center()
                            .gap(space(2.0))
                            .h(px(32.0))
                            .px(space(2.0))
                            .rounded(px(radius::SM))
                            .cursor_pointer()
                            .when(active, |d| d.bg(p.accent_soft))
                            .when(!active, |d| d.hover(|s| s.bg(p.hover)))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.selected = Some(pick.clone());
                                cx.notify();
                            }))
                            .child(channel_badge(&v.version_type, false, cx))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(type_scale::BODY))
                                    .text_color(if active { p.accent_text } else { p.text })
                                    .child(v.version_number.clone()),
                            )
                            .when(current, |d| {
                                d.child(
                                    div()
                                        .flex_none()
                                        .px(space(1.5))
                                        .rounded(px(radius::SM))
                                        .bg(p.raised)
                                        .text_size(px(type_scale::CAPTION))
                                        .text_color(p.text_secondary)
                                        .child("Installed"),
                                )
                            }),
                    );
                }
            }
        }
        let fitting_toggle = (!is_pack).then(|| {
            let on = self.only_fitting;
            let label = format!(
                "Only for {} {}",
                crate::state::loader_name(
                    AppState::global(cx)
                        .read(cx)
                        .instance(&self.instance_id)
                        .map(|i| i.loader)
                        .unwrap_or(theseus::data::ModLoader::Vanilla)
                ),
                self.game_version
            );
            div()
                .id("vp-fitting")
                .flex()
                .items_center()
                .gap(space(2.0))
                .px(space(3.0))
                .py(space(2.0))
                .border_t_1()
                .border_color(p.border)
                .cursor_pointer()
                .text_size(px(type_scale::SMALL))
                .text_color(p.text_secondary)
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.only_fitting = !this.only_fitting;
                    this.ensure_selection(cx);
                    cx.notify();
                }))
                .child(crate::ui::pages::library::checkbox(on, cx))
                .child(label)
        });
        let left = div()
            .w(px(240.0))
            .flex_none()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(p.border)
            .child(div().p(space(2.0)).child(Input::new(&self.search).small()))
            .child(list)
            .children(fitting_toggle);

        // Right: what the selected version is.
        let right = match (&self.versions, &selected) {
            (Some(Err(error)), _) => div().flex_1().child(empty_state(
                "circle-alert",
                "Couldn't load versions",
                error.clone(),
                None,
                cx,
            )),
            (_, Some(v)) => {
                let published = chrono::DateTime::parse_from_rfc3339(&v.date_published)
                    .map(|d| {
                        let date = d.format("%b %-d, %Y").to_string();
                        let ago = relative_time(d.with_timezone(&chrono::Utc)).to_string();
                        if ago == date {
                            date
                        } else {
                            format!("{date} ({ago})")
                        }
                    })
                    .unwrap_or_default();
                let changelog = v
                    .changelog
                    .as_deref()
                    .map(clean_description)
                    .filter(|c| !c.trim().is_empty())
                    .unwrap_or_else(|| "_No changelog for this version._".into());
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(space(1.0))
                            .px(space(5.0))
                            .py(space(3.0))
                            .border_b_1()
                            .border_color(p.border)
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(space(2.0))
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(px(type_scale::BODY_LG))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(p.text)
                                            .child(v.version_number.clone()),
                                    )
                                    .child(channel_badge(&v.version_type, true, cx)),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(type_scale::SMALL))
                                    .text_color(p.text_muted)
                                    .child(format!(
                                        "{} · {} · {}",
                                        v.loaders
                                            .iter()
                                            .map(|l| crate::ui::pages::project::title_case(l))
                                            .collect::<Vec<_>>()
                                            .join(", "),
                                        v.game_version_summary(),
                                        published
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .id("vp-changelog")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .px(space(5.0))
                            .py(space(3.0))
                            .text_size(px(type_scale::BODY))
                            .text_color(p.text_secondary)
                            .child(
                                TextView::markdown(
                                    SharedString::from(format!("vp-log-{}", v.id)),
                                    changelog,
                                )
                                .selectable(true),
                            ),
                    )
            }
            _ => div()
                .flex_1()
                .p(space(5.0))
                .child(skeleton(px(260.0), px(16.0), cx)),
        };

        let current_selected = selected
            .as_ref()
            .is_some_and(|v| Some(&v.id) == self.current.as_ref());
        let action = match (&selected, current_selected) {
            (Some(_), true) => "Already installed".to_string(),
            (Some(v), false) => {
                if self.working {
                    "Switching…".to_string()
                } else if is_pack {
                    format!("Switch to {}", v.version_number)
                } else {
                    format!("Install {}", v.version_number)
                }
            }
            (None, _) => "Pick a version".to_string(),
        };
        let footer = div()
            .flex()
            .items_center()
            .gap(space(2.0))
            .px(space(5.0))
            .py(space(3.0))
            .border_t_1()
            .border_color(p.border)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(type_scale::SMALL))
                    .when_some(self.error.clone(), |d, e| d.text_color(p.danger).child(e))
                    .when(self.error.is_none() && is_pack, |d| {
                        d.text_color(p.text_muted).child(
                            "Calcite saves a snapshot first, so you can go back from Snapshots.",
                        )
                    }),
            )
            .child(
                Button::new("vp-cancel")
                    .label("Cancel")
                    .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(DialogEvent::Close))),
            )
            .child(
                Button::new("vp-apply")
                    .icon("arrow-down-to-line")
                    .label(action)
                    .primary()
                    .disabled(selected.is_none() || current_selected || self.working)
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.apply(cx))),
            );

        div()
            .w(px(760.0))
            .h(px(520.0))
            .flex()
            .flex_col()
            .rounded(px(radius::XL))
            .bg(p.overlay)
            .border_1()
            .border_color(p.border_strong)
            .shadow_lg()
            .overflow_hidden()
            .child(header)
            .child(div().flex().flex_1().min_h_0().child(left).child(right))
            .child(footer)
    }
}
