//! Discover: search Modrinth for modpacks, mods, packs and shaders.

use std::time::Duration;

use calcite_core::modrinth::{ProjectType, SearchHit, SearchIndex, SearchQuery, SearchResults};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{
    ClickEvent, Context, Entity, FontWeight, SharedString, Subscription, Task, Window, div, px,
};

use crate::backend::Backend;
use crate::images::Images;
use crate::state::AppState;
use crate::theme::{PaletteExt, radius, space, type_scale};
use crate::ui::components::{Button, chip, compact_number, empty_state, icon, skeleton, thumb};
use crate::ui::pages::page_header;

/// Shared Modrinth API client, installed at startup.
pub struct Api(pub calcite_core::modrinth::Client);
impl gpui_kit::Global for Api {}

pub struct DiscoverPage {
    query: SearchQuery,
    input: Entity<InputState>,
    results: Option<Result<SearchResults, String>>,
    loading: bool,
    /// Modpacks the user started installing from this page.
    started: std::collections::HashSet<String>,
    pending: Option<Task<()>>,
    _subscription: Subscription,
}

impl DiscoverPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search modpacks"));
        let subscription = cx.subscribe_in(&input, window, |this, input, event, _window, cx| {
            if let InputEvent::Change = event {
                this.query.text = input.read(cx).value().to_string();
                this.query.offset = 0;
                this.search(Duration::from_millis(250), cx);
            }
        });
        let mut page = DiscoverPage {
            query: SearchQuery::new(ProjectType::Modpack),
            input,
            results: None,
            loading: false,
            started: Default::default(),
            pending: None,
            _subscription: subscription,
        };
        page.search(Duration::ZERO, cx);
        page
    }

    fn set_type(&mut self, project_type: ProjectType, window: &mut Window, cx: &mut Context<Self>) {
        if self.query.project_type == project_type {
            return;
        }
        self.query.project_type = project_type;
        self.query.offset = 0;
        let placeholder = format!("Search {}", project_type.plural_label().to_lowercase());
        self.input.update(cx, |input, cx| {
            input.set_placeholder(placeholder, window, cx);
        });
        self.search(Duration::ZERO, cx);
    }

    fn set_index(&mut self, index: SearchIndex, cx: &mut Context<Self>) {
        self.query.index = index;
        self.query.offset = 0;
        self.search(Duration::ZERO, cx);
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
                    .when(active, |d| d.bg(p.accent_soft).text_color(p.accent_text))
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
        // Modpacks install as a new instance. Adding mods to an existing
        // instance arrives with the instance picker in the next milestone.
        let installable =
            self.query.project_type == ProjectType::Modpack && hit.latest_version.is_some();
        let started = self.started.contains(&hit.project_id);
        let install = (
            hit.project_id.clone(),
            hit.latest_version.clone(),
            hit.title.clone(),
            hit.icon_url.clone(),
        );

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
                    .when(installable, |d| {
                        d.child(
                            Button::new(SharedString::from(format!("install-{}", hit.project_id)))
                                .icon(if started {
                                    "check"
                                } else {
                                    "arrow-down-to-line"
                                })
                                .label(if started { "Installing" } else { "Install" })
                                .disabled(started)
                                .small()
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    let (project_id, version, title, icon_url) = install.clone();
                                    let Some(version_id) = version else { return };
                                    this.started.insert(project_id.clone());
                                    AppState::global(cx).update(cx, |state, cx| {
                                        state.install_modpack(
                                            project_id, version_id, title, icon_url, cx,
                                        )
                                    });
                                    cx.notify();
                                })),
                        )
                    }),
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _ = window;
        let p = cx.palette().clone();

        let mut list = div().flex().flex_col().gap(space(2.5));
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
                    "Nothing matches that search",
                    "Try fewer words or a different spelling.",
                    None,
                    cx,
                ));
            }
            Some(Ok(results)) => {
                let hits = results.hits.clone();
                for (ix, hit) in hits.iter().enumerate() {
                    list = list.child(self.result_row(ix, hit, cx));
                }
            }
        }

        let total = match &self.results {
            Some(Ok(r)) => Some(format!("{} results", compact_number(r.total_hits as u64))),
            _ => None,
        };

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
                    .child(self.type_tabs(cx))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(space(3.0))
                            .child(
                                div().w(px(420.0)).child(
                                    Input::new(&self.input)
                                        .prefix(icon("search", 14.0, p.text_muted))
                                        .cleanable(true)
                                        .large(),
                                ),
                            )
                            .child(div().flex_1())
                            .when(self.loading && self.results.is_some(), |d| {
                                d.child(
                                    div()
                                        .text_size(px(type_scale::SMALL))
                                        .text_color(p.text_muted)
                                        .child("Searching…"),
                                )
                            })
                            .child(self.sort_row(cx)),
                    ),
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
