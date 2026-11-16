//! Discover: search Modrinth for modpacks, mods, packs and shaders.
//!
//! Opened from an instance, it shows only what fits that instance and
//! installs straight into it.

use std::time::Duration;

use calcite_core::modrinth::{ProjectType, SearchHit, SearchIndex, SearchQuery, SearchResults};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::select::{SearchableVec, Select, SelectEvent, SelectState};
use gpui_kit::prelude::*;
use gpui_kit::{
    ClickEvent, Context, Entity, EventEmitter, FontWeight, SharedString, Subscription, Task,
    Window, div, px,
};

use crate::backend::Backend;
use crate::images::Images;
use crate::state::AppState;
use crate::theme::{PaletteExt, radius, space, type_scale};
use crate::ui::components::{Button, chip, compact_number, empty_state, icon, skeleton, thumb};
use crate::ui::instance_picker::{ProjectSummary, content_type, modrinth_loader};
use crate::ui::pages::page_header;

/// Shared Modrinth API client, installed at startup.
pub struct Api(pub calcite_core::modrinth::Client);
impl gpui_kit::Global for Api {}

pub enum DiscoverEvent {
    OpenProject { id: String, target: Option<String> },
    AddTo(ProjectSummary),
}

const LOADER_FILTERS: [(&str, &str); 5] = [
    ("Any loader", ""),
    ("Fabric", "fabric"),
    ("Quilt", "quilt"),
    ("NeoForge", "neoforge"),
    ("Forge", "forge"),
];

type VersionSelect = SelectState<SearchableVec<SharedString>>;

pub struct DiscoverPage {
    query: SearchQuery,
    input: Entity<InputState>,
    version_select: Entity<VersionSelect>,
    results: Option<Result<SearchResults, String>>,
    loading: bool,
    /// The instance results are being added to, if opened from one.
    target: Option<String>,
    /// Modpacks the user started installing from this page.
    started: std::collections::HashSet<String>,
    /// Project ids already in the target instance.
    installed: std::collections::HashSet<String>,
    pending: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DiscoverEvent> for DiscoverPage {}

impl DiscoverPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search modpacks"));
        let version_select = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(vec![SharedString::from("Any version")]),
                Some(gpui_kit::component::IndexPath::new(0)),
                window,
                cx,
            )
            .searchable(true)
        });
        let subscriptions = vec![
            cx.subscribe_in(&input, window, |this, input, event, _window, cx| {
                if let InputEvent::Change = event {
                    this.query.text = input.read(cx).value().to_string();
                    this.query.offset = 0;
                    this.search(Duration::from_millis(250), cx);
                }
            }),
            cx.subscribe_in(&version_select, window, |this, _, event, _, cx| {
                let SelectEvent::Confirm(value) = event;
                this.query.game_versions = value
                    .as_ref()
                    .filter(|v| v.as_ref() != "Any version")
                    .map(|v| vec![v.to_string()])
                    .unwrap_or_default();
                this.query.offset = 0;
                this.search(Duration::ZERO, cx);
            }),
            cx.observe(&AppState::global(cx), |_, _, cx| cx.notify()),
        ];

        // Fill the version filter once.
        let client = cx.global::<Api>().0.clone();
        let fetch = Backend::global(cx).run(async move { client.release_game_versions().await });
        let select = version_select.clone();
        cx.spawn_in(window, async move |_, cx| {
            if let Ok(Ok(versions)) = fetch.await {
                let mut items = vec![SharedString::from("Any version")];
                items.extend(versions.into_iter().map(SharedString::from));
                let _ = select.update_in(cx, |select, window, cx| {
                    select.set_items(SearchableVec::new(items), window, cx);
                    select.set_selected_index(
                        Some(gpui_kit::component::IndexPath::new(0)),
                        window,
                        cx,
                    );
                });
            }
        })
        .detach();

        let mut page = DiscoverPage {
            query: SearchQuery::new(ProjectType::Modpack),
            input,
            version_select,
            results: None,
            loading: false,
            target: None,
            started: Default::default(),
            installed: Default::default(),
            pending: None,
            _subscriptions: subscriptions,
        };
        page.search(Duration::ZERO, cx);
        page
    }

    /// Search for `text` (from the title bar), keeping this page's own
    /// search box in step.
    pub fn search_for(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.query.text == text {
            return;
        }
        self.input
            .update(cx, |input, cx| input.set_value(text.clone(), window, cx));
        self.query.text = text;
        self.query.offset = 0;
        self.search(Duration::from_millis(250), cx);
    }

    /// Show only what fits `instance_id` and install straight into it.
    /// `None` returns to browsing everything.
    pub fn set_target(
        &mut self,
        instance_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.target = instance_id.clone();
        self.installed.clear();
        if let Some(id) = &instance_id {
            if let Some(inst) = AppState::global(cx).read(cx).instance(id).cloned() {
                if self.query.project_type == ProjectType::Modpack {
                    self.set_type_inner(ProjectType::Mod, window, cx);
                }
                self.query.game_versions = vec![inst.game_version.to_string()];
                self.query.loaders = if inst.loader == theseus::data::ModLoader::Vanilla {
                    Vec::new()
                } else {
                    vec![modrinth_loader(&inst)]
                };
            }
            let id = id.clone();
            let fetch = Backend::global(cx)
                .call(async move { theseus::instance::get_installed_project_ids(&id).await });
            cx.spawn(async move |this, cx| {
                if let Ok(ids) = fetch.await {
                    let _ = this.update(cx, |this, cx| {
                        this.installed = ids.into_iter().collect();
                        cx.notify();
                    });
                }
            })
            .detach();
        } else {
            self.query.game_versions.clear();
            self.query.loaders.clear();
        }
        self.query.offset = 0;
        self.search(Duration::ZERO, cx);
    }

    fn set_type_inner(
        &mut self,
        project_type: ProjectType,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.query.project_type = project_type;
        self.query.offset = 0;
        // Loader filters only mean something for mods and modpacks.
        if !matches!(project_type, ProjectType::Mod | ProjectType::Modpack) {
            self.query.loaders.clear();
        } else if let Some(inst) = self
            .target
            .as_ref()
            .and_then(|t| AppState::global(cx).read(cx).instance(t).cloned())
            && inst.loader != theseus::data::ModLoader::Vanilla
        {
            self.query.loaders = vec![modrinth_loader(&inst)];
        }
        let placeholder = format!("Search {}", project_type.plural_label().to_lowercase());
        self.input.update(cx, |input, cx| {
            input.set_placeholder(placeholder, window, cx);
        });
    }

    fn set_type(&mut self, project_type: ProjectType, window: &mut Window, cx: &mut Context<Self>) {
        if self.query.project_type == project_type {
            return;
        }
        self.set_type_inner(project_type, window, cx);
        self.search(Duration::ZERO, cx);
    }

    fn set_loader(&mut self, loader: &str, cx: &mut Context<Self>) {
        self.query.loaders = if loader.is_empty() {
            Vec::new()
        } else {
            vec![loader.to_string()]
        };
        self.query.offset = 0;
        self.search(Duration::ZERO, cx);
    }

    fn set_index(&mut self, index: SearchIndex, cx: &mut Context<Self>) {
        self.query.index = index;
        self.query.offset = 0;
        self.search(Duration::ZERO, cx);
    }

    fn load_more(&mut self, cx: &mut Context<Self>) {
        let Some(Ok(results)) = &self.results else {
            return;
        };
        if self.loading || results.hits.len() as u32 >= results.total_hits {
            return;
        }
        let mut query = self.query.clone();
        query.offset = results.hits.len() as u32;
        let client = cx.global::<Api>().0.clone();
        let fetch = Backend::global(cx).run(async move { client.search(&query).await });
        self.loading = true;
        cx.notify();
        self.pending = Some(cx.spawn(async move |this, cx| {
            let result = fetch.await.and_then(|r| r);
            let _ = this.update(cx, |this, cx| {
                if let (Ok(more), Some(Ok(existing))) = (result, this.results.as_mut()) {
                    existing.hits.extend(more.hits);
                }
                this.loading = false;
                cx.notify();
            });
        }));
    }

    /// Run the current query after `delay`. A newer call replaces an older
    /// pending one, which is all the debouncing typing needs.
    fn search(&mut self, delay: Duration, cx: &mut Context<Self>) {
        let query = self.query.clone();
        let client = cx.global::<Api>().0.clone();
        let backend = Backend::global(cx).clone();
        self.loading = true;
        cx.notify();
        self.pending = Some(cx.spawn(async move |this, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }
            let result = backend
                .run(async move { client.search(&query).await })
                .await
                .and_then(|r| r)
                .map_err(|e| format!("{e:#}"));
            let _ = this.update(cx, |this, cx| {
                this.results = Some(result);
                this.loading = false;
                cx.notify();
            });
        }));
    }

    fn type_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();
        let mut row = div().flex().items_center().gap(space(1.0));
        for t in ProjectType::BROWSABLE {
            // Modpacks become their own instances, so they don't go into one.
            if self.target.is_some() && t == ProjectType::Modpack {
                continue;
            }
            let active = t == self.query.project_type;
            row = row.child(
                div()
                    .id(SharedString::from(format!("ptype-{}", t.as_str())))
                    .h(px(30.0))
                    .px(space(3.0))
                    .flex()
                    .items_center()
                    .rounded(px(radius::SM))
                    .text_size(px(type_scale::BODY))
                    .font_weight(FontWeight::MEDIUM)
                    .cursor_pointer()
                    .when(active, |d| d.bg(p.raised).text_color(p.text))
                    .when(!active, |d| {
                        d.text_color(p.text_secondary)
                            .hover(|s| s.text_color(p.text).bg(p.hover))
                    })
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.set_type(t, window, cx)
                    }))
                    .child(t.plural_label()),
            );
        }
        row
    }

    fn filters(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();
        let mut row = div().flex().items_center().gap(space(1.0));
        let show_loaders = self.target.is_none()
            && matches!(
                self.query.project_type,
                ProjectType::Mod | ProjectType::Modpack
            );
        if show_loaders {
            for (label, key) in LOADER_FILTERS {
                let active = self.query.loaders.first().map(String::as_str).unwrap_or("") == key;
                row =
                    row.child(
                        div()
                            .id(SharedString::from(format!("loader-filter-{label}")))
                            .h(px(26.0))
                            .px(space(2.5))
                            .flex()
                            .items_center()
                            .rounded(px(radius::SM))
                            .text_size(px(type_scale::SMALL))
                            .cursor_pointer()
                            .when(active, |d| d.bg(p.accent_soft).text_color(p.accent_text))
                            .when(!active, |d| {
                                d.text_color(p.text_muted).hover(|s| s.text_color(p.text))
                            })
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.set_loader(key, cx)
                            }))
                            .child(label),
                    );
            }
        }
        row
    }

    fn sort_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();
        let mut row = div().flex().items_center().gap(space(1.0));
        for index in [
            SearchIndex::Relevance,
            SearchIndex::Downloads,
            SearchIndex::Updated,
            SearchIndex::Newest,
        ] {
            let active = index == self.query.index;
            row = row.child(
                div()
                    .id(SharedString::from(format!("sort-{}", index.as_str())))
                    .h(px(26.0))
                    .px(space(2.5))
                    .flex()
                    .items_center()
                    .rounded(px(radius::SM))
                    .text_size(px(type_scale::SMALL))
                    .cursor_pointer()
                    .when(active, |d| d.bg(p.raised).text_color(p.text))
                    .when(!active, |d| {
                        d.text_color(p.text_muted).hover(|s| s.text_color(p.text))
                    })
                    .on_click(
                        cx.listener(move |this, _: &ClickEvent, _, cx| this.set_index(index, cx)),
                    )
                    .child(index.label()),
            );
        }
        row
    }

    fn result_row(&self, ix: usize, hit: &SearchHit, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();
        let source = hit
            .icon_url
            .as_deref()
            .and_then(|url| Images::get(url, 128, cx));
        let mut tags = div().flex().gap(space(1.5)).flex_wrap();
        if let Some(v) = hit.newest_game_version() {
            tags = tags.child(chip(v.to_string(), cx));
        }
        for loader in hit.loaders().into_iter().take(3) {
            tags = tags.child(chip(title_case(loader), cx));
        }
        for cat in hit.display_categories.iter().take(3) {
            if !hit.loaders().contains(&cat.as_str()) {
                tags = tags.child(chip(title_case(cat), cx));
            }
        }

        let is_modpack = self.query.project_type == ProjectType::Modpack;
        let state = AppState::global(cx).read(cx);
        let installed = self.installed.contains(&hit.project_id);
        let busy = self
            .target
            .as_ref()
            .is_some_and(|t| state.is_installing(t, &hit.project_id));
        let started = self.started.contains(&hit.project_id);

        let summary = ProjectSummary {
            id: hit.project_id.clone(),
            title: hit.title.clone().into(),
            icon_url: hit.icon_url.clone(),
            kind: self.query.project_type,
        };
        let latest = hit.latest_version.clone();
        let target = self.target.clone();
        let action: gpui_kit::AnyElement = if installed {
            div()
                .flex()
                .items_center()
                .gap(space(1.5))
                .text_size(px(type_scale::SMALL))
                .text_color(p.running)
                .child(icon("check", 14.0, p.running))
                .child("Installed")
                .into_any_element()
        } else {
            let label: SharedString = if is_modpack {
                if started {
                    "Installing".into()
                } else {
                    "Install".into()
                }
            } else if target.is_some() {
                if busy {
                    "Adding…".into()
                } else {
                    "Add".into()
                }
            } else {
                "Add to…".into()
            };
            Button::new(SharedString::from(format!("install-{}", hit.project_id)))
                .icon(if is_modpack {
                    "arrow-down-to-line"
                } else {
                    "plus"
                })
                .label(label)
                .small()
                .when(target.is_some() || is_modpack, |b| b.primary())
                .disabled(busy || started || (is_modpack && latest.is_none()))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    cx.stop_propagation();
                    let summary = summary.clone();
                    if is_modpack {
                        let Some(version_id) = latest.clone() else {
                            return;
                        };
                        this.started.insert(summary.id.clone());
                        AppState::global(cx).update(cx, |state, cx| {
                            state.install_modpack(
                                summary.id,
                                version_id,
                                summary.title.to_string(),
                                summary.icon_url,
                                cx,
                            )
                        });
                    } else if let Some(target) = this.target.clone() {
                        this.installed.insert(summary.id.clone());
                        AppState::global(cx).update(cx, |state, cx| {
                            state.install_content(
                                target,
                                summary.id,
                                None,
                                content_type(summary.kind),
                                cx,
                            )
                        });
                    } else {
                        cx.emit(DiscoverEvent::AddTo(summary));
                    }
                    cx.notify();
                }))
                .into_any_element()
        };

        let open_id = hit.project_id.clone();
        div()
            .id(ix)
            .flex()
            .gap(space(4.0))
            .p(space(4.0))
            .rounded(px(radius::LG))
            .bg(p.surface)
            .border_1()
            .border_color(p.border)
            .hover(|s| s.border_color(p.border_strong))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                cx.emit(DiscoverEvent::OpenProject {
                    id: open_id.clone(),
                    target: this.target.clone(),
                })
            }))
            .child(thumb(source, &hit.title, 72.0, radius::LG, cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(space(1.5))
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(space(2.0))
                            .child(
                                div()
                                    .text_size(px(type_scale::SECTION))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(p.text)
                                    .truncate()
                                    .child(hit.title.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(type_scale::SMALL))
                                    .text_color(p.text_muted)
                                    .child(format!("by {}", hit.author)),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(type_scale::BODY))
                            .text_color(p.text_secondary)
                            .line_clamp(2)
                            .child(hit.description.clone()),
                    )
                    .child(tags),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_end()
                    .justify_between()
                    .flex_none()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_end()
                            .gap(space(1.0))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(space(1.5))
                                    .text_size(px(type_scale::SMALL))
                                    .text_color(p.text_secondary)
                                    .child(icon("arrow-down-to-line", 12.0, p.text_muted))
                                    .child(compact_number(hit.downloads)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(space(1.5))
                                    .text_size(px(type_scale::SMALL))
                                    .text_color(p.text_secondary)
                                    .child(icon("heart", 12.0, p.text_muted))
                                    .child(compact_number(hit.follows)),
                            ),
                    )
                    .child(action),
            )
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

impl Render for DiscoverPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();

        let mut list = div().flex().flex_col().gap(space(2.5));
        let mut more = false;
        match &self.results {
            None => {
                for _ in 0..6 {
                    list = list.child(
                        div()
                            .flex()
                            .gap(space(4.0))
                            .p(space(4.0))
                            .rounded(px(radius::LG))
                            .border_1()
                            .border_color(p.border)
                            .child(skeleton(px(72.0), px(72.0), cx).rounded(px(radius::LG)))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(space(2.0))
                                    .flex_1()
                                    .child(skeleton(px(220.0), px(16.0), cx))
                                    .child(skeleton(gpui_kit::relative(0.9), px(12.0), cx))
                                    .child(skeleton(gpui_kit::relative(0.6), px(12.0), cx)),
                            ),
                    );
                }
            }
            Some(Err(error)) => {
                list = list.child(empty_state(
                    "wifi-off",
                    "Couldn't reach Modrinth",
                    format!("Check your connection and try again. ({error})"),
                    Some(
                        Button::new("retry")
                            .icon("refresh-cw")
                            .label("Try again")
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.search(Duration::ZERO, cx)
                            }))
                            .into_any_element(),
                    ),
                    cx,
                ));
            }
            Some(Ok(results)) if results.hits.is_empty() => {
                list = list.child(empty_state(
                    "search",
                    "Nothing matches",
                    if self.target.is_some() {
                        "Nothing like that for this instance's loader and version. Try other words."
                    } else {
                        "Try fewer words, another spelling, or clear the filters."
                    },
                    None,
                    cx,
                ));
            }
            Some(Ok(results)) => {
                let hits = results.hits.clone();
                more = (hits.len() as u32) < results.total_hits;
                for (ix, hit) in hits.iter().enumerate() {
                    list = list.child(self.result_row(ix, hit, cx));
                }
            }
        }
        if more {
            list = list.child(
                div().flex().justify_center().py(space(4.0)).child(
                    Button::new("load-more")
                        .label(if self.loading {
                            "Loading…"
                        } else {
                            "Show more"
                        })
                        .disabled(self.loading)
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.load_more(cx))),
                ),
            );
        }

        let total = match &self.results {
            Some(Ok(r)) => Some(format!("{} results", compact_number(r.total_hits as u64))),
            _ => None,
        };
        let target = self
            .target
            .as_ref()
            .and_then(|t| AppState::global(cx).read(cx).instance(t).cloned());

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(space(4.0))
                    .px(space(8.0))
                    .pt(space(7.0))
                    .pb(space(4.0))
                    .child(page_header("Discover", total, cx))
                    .when_some(target, |d, inst| {
                        d.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(space(3.0))
                                .px(space(3.5))
                                .py(space(2.5))
                                .rounded(px(radius::MD))
                                .bg(p.accent_soft)
                                .child(icon("package", 16.0, p.accent_text))
                                .child(
                                    div()
                                        .flex_1()
                                        .text_size(px(type_scale::BODY))
                                        .text_color(p.text)
                                        .child(format!(
                                            "Adding to {} · showing what fits {}",
                                            inst.name,
                                            inst.loader_line()
                                        )),
                                )
                                .child(
                                    Button::new("clear-target")
                                        .label("Browse everything")
                                        .ghost()
                                        .small()
                                        .on_click(cx.listener(
                                            |this, _: &ClickEvent, window, cx| {
                                                this.set_target(None, window, cx)
                                            },
                                        )),
                                ),
                        )
                    })
                    .child(self.type_tabs(cx))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(space(3.0))
                            .child(
                                div().w(px(380.0)).child(
                                    Input::new(&self.input)
                                        .prefix(icon("search", 14.0, p.text_muted))
                                        .cleanable(true)
                                        .large(),
                                ),
                            )
                            .when(self.target.is_none(), |d| {
                                d.child(div().w(px(150.0)).child(
                                    Select::new(&self.version_select).placeholder("Any version"),
                                ))
                            })
                            .child(div().flex_1())
                            .child(self.sort_row(cx)),
                    )
                    .child(self.filters(cx)),
            )
            .child(
                div()
                    .id("discover-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(space(8.0))
                    .pb(space(10.0))
                    .child(list),
            )
    }
}
