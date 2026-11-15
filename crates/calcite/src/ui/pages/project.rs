//! A Modrinth project: description, versions, gallery, and a way to install it.

use calcite_core::markup::clean_description;
use calcite_core::modrinth::{Project, ProjectType, Version};
use gpui_kit::component::text::TextView;
use gpui_kit::prelude::*;
use gpui_kit::{
    ClickEvent, Context, EventEmitter, FontWeight, ObjectFit, SharedString, Window, div, img, px,
};

use crate::backend::Backend;
use crate::images::Images;
use crate::state::AppState;
use crate::theme::{PaletteExt, radius, space, type_scale};
use crate::ui::components::{
    Button, ButtonSize, chip, compact_number, empty_state, icon_chip, relative_time, skeleton,
    thumb,
};
use crate::ui::instance_picker::{ProjectSummary, content_type};
use crate::ui::pages::discover::Api;

pub enum ProjectEvent {
    /// Ask the shell to open the "Add to instance" picker.
    AddTo(ProjectSummary),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Description,
    Versions,
    Gallery,
}

pub struct ProjectPage {
    id: String,
    /// When opened from an instance, installs go straight into it.
    target: Option<String>,
    project: Option<Result<Project, String>>,
    versions: Option<Result<Vec<Version>, String>>,
    body: SharedString,
    tab: Tab,
    installing_modpack: bool,
}

impl EventEmitter<ProjectEvent> for ProjectPage {}

impl ProjectPage {
    pub fn new(id: String, target: Option<String>, cx: &mut Context<Self>) -> Self {
        let client = cx.global::<Api>().0.clone();
        let key = id.clone();
        let fetch = Backend::global(cx).run(async move {
            let (project, versions) = futures::join!(client.project(&key), client.versions(&key));
            (
                project.map_err(|e| format!("{e:#}")),
                versions.map_err(|e| format!("{e:#}")),
            )
        });
        cx.observe(&AppState::global(cx), |_, _, cx| cx.notify())
            .detach();
        cx.spawn(async move |this, cx| {
            if let Ok((project, versions)) = fetch.await {
                let _ = this.update(cx, |this, cx| {
                    if let Ok(p) = &project {
                        this.body = clean_description(&p.body).into();
                    }
                    this.project = Some(project);
                    this.versions = Some(versions);
                    cx.notify();
                });
            }
        })
        .detach();
        ProjectPage {
            id,
            target,
            project: None,
            versions: None,
            body: SharedString::default(),
            tab: Tab::Description,
            installing_modpack: false,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn title(&self) -> SharedString {
        match &self.project {
            Some(Ok(p)) => p.title.clone().into(),
            _ => "Project".into(),
        }
    }

    fn summary(project: &Project) -> ProjectSummary {
        ProjectSummary {
            id: project.id.clone(),
            title: project.title.clone().into(),
            icon_url: project.icon_url.clone(),
            kind: project.project_type(),
        }
    }

    /// Install a specific version (or the newest fitting one).
    fn install(&mut self, project: &Project, version: Option<&Version>, cx: &mut Context<Self>) {
        let state = AppState::global(cx);
        if project.project_type() == ProjectType::Modpack {
            let version_id = version.map(|v| v.id.clone()).or_else(|| {
                self.versions
                    .as_ref()
                    .and_then(|v| v.as_ref().ok())
                    .and_then(|v| v.first())
                    .map(|v| v.id.clone())
            });
            let Some(version_id) = version_id else { return };
            self.installing_modpack = true;
            state.update(cx, |s, cx| {
                s.install_modpack(
                    project.id.clone(),
                    version_id,
                    project.title.clone(),
                    project.icon_url.clone(),
                    cx,
                )
            });
        } else if let Some(target) = self.target.clone() {
            state.update(cx, |s, cx| {
                s.install_content(
                    target,
                    project.id.clone(),
                    version.map(|v| v.id.clone()),
                    content_type(project.project_type()),
                    cx,
                )
            });
        } else {
            cx.emit(ProjectEvent::AddTo(Self::summary(project)));
        }
        cx.notify();
    }

    fn header(&self, project: &Project, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();
        let source = project
            .icon_url
            .as_deref()
            .and_then(|u| Images::get(u, 192, cx));
        let state = AppState::global(cx).read(cx);
        let target_name = self
            .target
            .as_ref()
            .and_then(|t| state.instance(t))
            .map(|i| i.name.clone());
        let busy = self
            .target
            .as_ref()
            .is_some_and(|t| state.is_installing(t, &project.id));
        let is_modpack = project.project_type() == ProjectType::Modpack;
        let label: SharedString = if is_modpack {
            if self.installing_modpack {
                "Installing".into()
            } else {
                "Install".into()
            }
        } else if let Some(name) = &target_name {
            if busy {
                "Adding…".into()
            } else {
                format!("Add to {name}").into()
            }
        } else {
            "Add to instance".into()
        };
        let p_clone = project.clone();
        let mut loaders = div().flex().flex_wrap().gap(space(1.5));
        for loader in project.loaders.iter().take(4) {
            loaders = loaders.child(chip(title_case(loader), cx));
        }
        for cat in project.categories.iter().take(3) {
            loaders = loaders.child(chip(title_case(cat), cx));
        }
        let links: Vec<(&str, &str, Option<String>)> = vec![
            ("Source", "code-xml", project.source_url.clone()),
            ("Issues", "bug", project.issues_url.clone()),
            ("Wiki", "book-open", project.wiki_url.clone()),
            ("Discord", "users", project.discord_url.clone()),
        ];
        let mut link_row = div().flex().gap(space(1.0));
        for (label, glyph, url) in links {
            if let Some(url) = url {
                link_row = link_row.child(
                    Button::new(SharedString::from(format!("link-{label}")))
                        .icon(glyph)
                        .label(label)
                        .ghost()
                        .small()
                        .on_click(move |_, _, cx| cx.open_url(&url)),
                );
            }
        }
        let updated = chrono::DateTime::parse_from_rfc3339(&project.updated)
            .map(|d| relative_time(d.with_timezone(&chrono::Utc)))
            .unwrap_or_default();

        div()
            .flex()
            .flex_col()
            .gap(space(4.0))
            .px(space(8.0))
            .pt(space(7.0))
            .pb(space(4.0))
            .child(
                div()
                    .flex()
                    .gap(space(5.0))
                    .child(thumb(source, &project.title, 96.0, radius::XL, cx))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .gap(space(2.0))
                            .child(
                                div()
                                    .text_size(px(type_scale::HEADLINE))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(p.text)
                                    .child(project.title.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(type_scale::BODY_LG))
                                    .text_color(p.text_secondary)
                                    .child(project.description.clone()),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .gap(space(1.5))
                                    .child(icon_chip(
                                        "arrow-down-to-line",
                                        compact_number(project.downloads),
                                        cx,
                                    ))
                                    .child(icon_chip(
                                        "heart",
                                        compact_number(project.followers),
                                        cx,
                                    ))
                                    .child(icon_chip("clock", format!("Updated {updated}"), cx))
                                    .children(
                                        project
                                            .license
                                            .as_ref()
                                            .map(|l| chip(l.id.replace("LicenseRef-", ""), cx)),
                                    ),
                            )
                            .child(loaders),
                    )
                    .child(
                        div().flex_none().child(
                            Button::new("project-install")
                                .icon(if is_modpack {
                                    "arrow-down-to-line"
                                } else {
                                    "plus"
                                })
                                .label(label)
                                .primary()
                                .size(ButtonSize::Large)
                                .disabled(busy || self.installing_modpack)
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.install(&p_clone, None, cx)
                                })),
                        ),
                    ),
            )
            .child(link_row)
    }

    fn tabs(&self, project: &Project, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();
        let versions = self
            .versions
            .as_ref()
            .and_then(|v| v.as_ref().ok())
            .map(Vec::len);
        let mut row = div()
            .flex()
            .items_end()
            .gap(space(5.0))
            .h(px(40.0))
            .px(space(8.0))
            .border_b_1()
            .border_color(p.border);
        for (tab, label, badge) in [
            (Tab::Description, "Description", None),
            (Tab::Versions, "Versions", versions),
            (
                Tab::Gallery,
                "Gallery",
                Some(project.gallery.len()).filter(|n| *n > 0),
            ),
        ] {
            if tab == Tab::Gallery && project.gallery.is_empty() {
                continue;
            }
            let active = tab == self.tab;
            row = row.child(
                div()
                    .id(SharedString::from(format!("ptab-{label}")))
                    .flex()
                    .items_center()
                    .gap(space(1.5))
                    .h_full()
                    .border_b_2()
                    .border_color(if active {
                        p.accent
                    } else {
                        gpui_kit::Hsla::transparent_black()
                    })
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.tab = tab;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .text_size(px(type_scale::BODY))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if active { p.text } else { p.text_secondary })
                            .child(label),
                    )
                    .when_some(badge, |d, n| {
                        d.child(
                            div()
                                .px(space(1.5))
                                .rounded(px(radius::SM))
                                .bg(p.raised)
                                .text_size(px(type_scale::CAPTION))
                                .text_color(p.text_muted)
                                .child(n.to_string()),
                        )
                    }),
            );
        }
        row
    }

    fn versions_tab(&self, project: &Project, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let p = cx.palette().clone();
        let versions = match &self.versions {
            None => return skeleton(gpui_kit::relative(1.0), px(200.0), cx).into_any_element(),
            Some(Err(e)) => {
                return empty_state(
                    "circle-alert",
                    "Couldn't load versions",
                    e.clone(),
                    None,
                    cx,
                )
                .into_any_element();
            }
            Some(Ok(v)) => v.clone(),
        };
        let mut list = div().flex().flex_col();
        for (ix, version) in versions.iter().take(80).enumerate() {
            let (fg, bg) = match version.version_type.as_str() {
                "beta" => (p.warning, p.warning_soft),
                "alpha" => (p.danger, p.danger_soft),
                _ => (p.running, p.running_soft),
            };
            let published = chrono::DateTime::parse_from_rfc3339(&version.date_published)
                .map(|d| relative_time(d.with_timezone(&chrono::Utc)))
                .unwrap_or_default();
            let (project_c, version_c) = (project.clone(), version.clone());
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(space(4.0))
                    .h(px(56.0))
                    .border_b_1()
                    .border_color(p.border)
                    .child(
                        div().w(px(64.0)).flex_none().child(
                            div()
                                .px(space(1.5))
                                .rounded(px(radius::SM))
                                .bg(bg)
                                .text_size(px(type_scale::CAPTION))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(fg)
                                .child(title_case(&version.version_type)),
                        ),
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
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.text)
                                    .child(version.name.clone()),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(type_scale::SMALL))
                                    .text_color(p.text_muted)
                                    .child(format!(
                                        "{} · {} · {}",
                                        version
                                            .loaders
                                            .iter()
                                            .map(|l| title_case(l))
                                            .collect::<Vec<_>>()
                                            .join(", "),
                                        version.game_version_summary(),
                                        published
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .w(px(80.0))
                            .text_size(px(type_scale::SMALL))
                            .text_color(p.text_secondary)
                            .child(compact_number(version.downloads)),
                    )
                    .child(
                        Button::new(SharedString::from(format!("install-version-{ix}")))
                            .icon("arrow-down-to-line")
                            .label("Install")
                            .small()
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.install(&project_c, Some(&version_c), cx)
                            })),
                    ),
            );
        }
        list.into_any_element()
    }

    fn gallery_tab(&self, project: &Project, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let p = cx.palette().clone();
        let mut grid = div().flex().flex_wrap().gap(space(3.0));
        for image in project.gallery.iter().take(24) {
            let src = Images::get(&image.url, 700, cx);
            grid = grid.child(
                div()
                    .w(px(300.0))
                    .flex()
                    .flex_col()
                    .gap(space(1.5))
                    .child(
                        div()
                            .w_full()
                            .h(px(170.0))
                            .rounded(px(radius::LG))
                            .overflow_hidden()
                            .bg(p.raised)
                            .children(src.map(|s| img(s).size_full().object_fit(ObjectFit::Cover))),
                    )
                    .when_some(image.title.clone(), |d, t| {
                        d.child(
                            div()
                                .truncate()
                                .text_size(px(type_scale::SMALL))
                                .text_color(p.text_secondary)
                                .child(t),
                        )
                    }),
            );
        }
        grid.into_any_element()
    }
}

fn title_case(s: &str) -> String {
    let s = s.replace('-', " ");
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

impl Render for ProjectPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let project = match &self.project {
            None => {
                return div()
                    .p(space(8.0))
                    .flex()
                    .gap(space(5.0))
                    .child(skeleton(px(96.0), px(96.0), cx))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(space(3.0))
                            .flex_1()
                            .child(skeleton(px(280.0), px(24.0), cx))
                            .child(skeleton(gpui_kit::relative(0.8), px(14.0), cx)),
                    )
                    .into_any_element();
            }
            Some(Err(error)) => {
                return empty_state(
                    "wifi-off",
                    "Couldn't load this project",
                    error.clone(),
                    None,
                    cx,
                )
                .into_any_element();
            }
            Some(Ok(project)) => project.clone(),
        };
        let p = cx.palette().clone();
        let body = match self.tab {
            Tab::Description => div()
                .max_w(px(820.0))
                .text_size(px(type_scale::BODY_LG))
                .text_color(p.text_secondary)
                .child(TextView::markdown("project-body", self.body.clone()).selectable(true))
                .into_any_element(),
            Tab::Versions => self.versions_tab(&project, cx),
            Tab::Gallery => self.gallery_tab(&project, cx),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.header(&project, cx))
            .child(self.tabs(&project, cx))
            .child(
                div()
                    .id("project-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(space(8.0))
                    .py(space(5.0))
                    .child(body),
            )
            .into_any_element()
    }
}
