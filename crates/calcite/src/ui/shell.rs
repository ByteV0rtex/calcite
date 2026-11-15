//! The window frame: title bar, sidebar, and the page area.

use gpui_kit::component::TitleBar;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Entity, FontWeight, SharedString, Window, div, px,
};

use crate::images::Images;
use crate::theme::{self, Accent, Appearance, PaletteExt, radius, space, type_scale};
use crate::ui::components::{icon, state_chip, thumb};
use crate::ui::pages::{discover::DiscoverPage, gallery, home, instance, library};
use crate::ui::sample;

#[derive(Clone, Debug, PartialEq)]
pub enum Route {
    Home,
    Library,
    Discover,
    Instance(&'static str),
    Gallery,
}

impl Route {
    fn title(&self) -> SharedString {
        match self {
            Route::Home => "Home".into(),
            Route::Library => "Library".into(),
            Route::Discover => "Discover".into(),
            Route::Instance(id) => sample::INSTANCES
                .iter()
                .find(|i| i.id == *id)
                .map(|i| i.name)
                .unwrap_or("Instance")
                .into(),
            Route::Gallery => "Design system".into(),
        }
    }

    fn parent(&self) -> Option<Route> {
        match self {
            Route::Instance(_) => Some(Route::Library),
            _ => None,
        }
    }
}

/// Width of the left sidebar.
pub const SIDEBAR_WIDTH: f32 = 228.0;
/// Height of the window title bar (macOS traffic lights are centered on it).
pub const TITLE_BAR_HEIGHT: f32 = 40.0;

pub struct Shell {
    route: Route,
    back: Vec<Route>,
    forward: Vec<Route>,
    discover: Entity<DiscoverPage>,
    instance_tab: instance::Tab,
}

impl Shell {
    pub fn new(start: Route, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let discover = cx.new(|cx| DiscoverPage::new(window, cx));
        Shell {
            route: start,
            back: Vec::new(),
            forward: Vec::new(),
            discover,
            instance_tab: instance::Tab::Content,
        }
    }

    pub fn navigate(&mut self, route: Route, cx: &mut Context<Self>) {
        if route == self.route {
            return;
        }
        let previous = std::mem::replace(&mut self.route, route);
        self.back.push(previous);
        self.forward.clear();
        if matches!(self.route, Route::Instance(_)) {
            self.instance_tab = instance::Tab::Content;
        }
        cx.notify();
    }

    fn go_back(&mut self, cx: &mut Context<Self>) {
        if let Some(route) = self.back.pop() {
            let current = std::mem::replace(&mut self.route, route);
            self.forward.push(current);
            cx.notify();
        }
    }

    fn go_forward(&mut self, cx: &mut Context<Self>) {
        if let Some(route) = self.forward.pop() {
            let current = std::mem::replace(&mut self.route, route);
            self.back.push(current);
            cx.notify();
        }
    }

    pub fn set_instance_tab(&mut self, tab: instance::Tab, cx: &mut Context<Self>) {
        self.instance_tab = tab;
        cx.notify();
    }

    // ------------------------------------------------------------ title bar

    fn title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();
        let can_back = !self.back.is_empty();
        let can_forward = !self.forward.is_empty();

        let nav_button = |id: &'static str, name: &'static str, enabled: bool| {
            div()
                .id(id)
                .size(px(28.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(radius::SM))
                .child(icon(
                    name,
                    16.0,
                    if enabled {
                        p.text_secondary
                    } else {
                        p.text_disabled
                    },
                ))
                .when(enabled, |d| d.cursor_pointer().hover(|s| s.bg(p.hover)))
        };

        let mut crumbs = div().flex().items_center().gap(space(1.5)).min_w_0();
        if let Some(parent) = self.route.parent() {
            let parent_title = parent.title();
            crumbs = crumbs
                .child(
                    div()
                        .id("crumb-parent")
                        .text_size(px(type_scale::BODY))
                        .text_color(p.text_muted)
                        .cursor_pointer()
                        .hover(|s| s.text_color(p.text_secondary))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.navigate(parent.clone(), cx)
                        }))
                        .child(parent_title),
                )
                .child(icon("chevron-right", 14.0, p.text_disabled));
        }
        if self.route.parent().is_some() {
            crumbs = crumbs.child(
                div()
                    .text_size(px(type_scale::BODY))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(p.text)
                    .truncate()
                    .child(self.route.title()),
            );
        }

        let running = sample::INSTANCES.iter().find(|i| i.running_secs.is_some());

        TitleBar::new()
            .h(px(TITLE_BAR_HEIGHT))
            .bg(p.app)
            .border_color(p.border)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap(space(1.0))
                    .pl(if cfg!(target_os = "macos") {
                        space(2.0)
                    } else {
                        space(3.0)
                    })
                    .child(
                        nav_button("nav-back", "arrow-left", can_back)
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.go_back(cx))),
                    )
                    .child(
                        nav_button("nav-forward", "arrow-right", can_forward).on_click(
                            cx.listener(|this, _: &ClickEvent, _, cx| this.go_forward(cx)),
                        ),
                    )
                    .child(div().w(space(2.0)))
                    .child(crumbs),
            )
            .child(
                // The command palette trigger. Milestone 9 wires it to ⌘K.
                div()
                    .id("palette-trigger")
                    .flex()
                    .items_center()
                    .gap(space(2.0))
                    .h(px(28.0))
                    .w(px(440.0))
                    // Sit 6px below the top edge, a touch lower than centre.
                    .relative()
                    .top(px(1.0))
                    .px(space(3.0))
                    .rounded(px(radius::SM))
                    .bg(p.surface)
                    .border_1()
                    .border_color(p.border)
                    .cursor_pointer()
                    .hover(|s| s.border_color(p.border_strong))
                    .on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.navigate(Route::Discover, cx)
                        }),
                    )
                    .child(icon("search", 14.0, p.text_muted))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(type_scale::SMALL))
                            .text_color(p.text_muted)
                            .child("Search or jump to…"),
                    )
                    .child(
                        div()
                            .text_size(px(type_scale::CAPTION))
                            .text_color(p.text_disabled)
                            .child(if cfg!(target_os = "macos") {
                                "⌘K"
                            } else {
                                "Ctrl K"
                            }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .justify_end()
                    .items_center()
                    .gap(space(2.0))
                    .pr(space(2.0))
                    .when_some(running, |d, inst| {
                        let id = inst.id;
                        d.child(
                            div()
                                .id("running-pill")
                                .flex()
                                .items_center()
                                .gap(space(1.5))
                                .h(px(28.0))
                                .pl(space(1.0))
                                .pr(space(2.5))
                                .rounded(px(radius::SM))
                                .bg(p.running_soft)
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.navigate(Route::Instance(id), cx)
                                }))
                                .child(
                                    div()
                                        .size(px(6.0))
                                        .ml(space(1.0))
                                        .rounded_full()
                                        .bg(p.running),
                                )
                                .child(
                                    div()
                                        .text_size(px(type_scale::SMALL))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(p.running)
                                        .child(inst.name),
                                ),
                        )
                    }),
            )
    }

    // ------------------------------------------------------------ sidebar

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();

        let nav_item =
            |id: &'static str,
             name: &'static str,
             text: &'static str,
             route: Route,
             active: bool,
             cx: &mut Context<Self>| {
                div()
                    .id(id)
                    .flex()
                    .items_center()
                    .gap(space(2.5))
                    .h(px(30.0))
                    .px(space(2.5))
                    .rounded(px(radius::SM))
                    .text_size(px(type_scale::BODY))
                    .font_weight(FontWeight::MEDIUM)
                    .cursor_pointer()
                    .when(active, |d| d.bg(p.hover).text_color(p.text))
                    .when(!active, |d| {
                        d.text_color(p.text_secondary)
                            .hover(|s| s.bg(p.hover).text_color(p.text))
                    })
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.navigate(route.clone(), cx)
                    }))
                    .child(icon(
                        name,
                        16.0,
                        if active { p.accent } else { p.text_muted },
                    ))
                    .child(text)
            };

        let route = self.route.clone();
        let in_library = matches!(route, Route::Library | Route::Instance(_));

        let mut recent = div().flex().flex_col().gap(space(0.5));
        for inst in sample::INSTANCES.iter().take(5) {
            let active = route == Route::Instance(inst.id);
            let source = inst.icon.and_then(|url| Images::get(url, 64, cx));
            let id = inst.id;
            recent = recent.child(
                div()
                    .id(SharedString::from(format!("recent-{}", inst.id)))
                    .flex()
                    .items_center()
                    .gap(space(2.5))
                    .h(px(30.0))
                    .px(space(2.5))
                    .rounded(px(radius::SM))
                    .cursor_pointer()
                    .when(active, |d| d.bg(p.hover))
                    .hover(|s| s.bg(p.hover))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.navigate(Route::Instance(id), cx)
                    }))
                    .child(thumb(source, inst.name, 18.0, 4.0, cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(type_scale::BODY))
                            .text_color(if active { p.text } else { p.text_secondary })
                            .child(inst.name),
                    )
                    .when(inst.running_secs.is_some(), |d| {
                        d.child(div().size(px(6.0)).rounded_full().bg(p.running))
                    }),
            );
        }

        div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .bg(p.rail)
            .border_r_1()
            .border_color(p.border)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(space(0.5))
                    .p(space(2.0))
                    .child(nav_item(
                        "nav-home",
                        "house",
                        "Home",
                        Route::Home,
                        route == Route::Home,
                        cx,
                    ))
                    .child(nav_item(
                        "nav-library",
                        "layout-grid",
                        "Library",
                        Route::Library,
                        in_library,
                        cx,
                    ))
                    .child(nav_item(
                        "nav-discover",
                        "compass",
                        "Discover",
                        Route::Discover,
                        route == Route::Discover,
                        cx,
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(space(1.0))
                    .px(space(2.0))
                    .pt(space(3.0))
                    .child(
                        div()
                            .px(space(2.5))
                            .pb(space(1.0))
                            .text_size(px(type_scale::CAPTION))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(p.text_muted)
                            .child("Recent"),
                    )
                    .child(recent),
            )
            .child(div().flex_1())
            .child(self.download_card(cx))
            .child(self.account_row(cx))
    }

    fn download_card(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();
        div()
            .mx(space(2.0))
            .mb(space(2.0))
            .p(space(2.5))
            .rounded(px(radius::MD))
            .bg(p.surface)
            .border_1()
            .border_color(p.border)
            .flex()
            .flex_col()
            .gap(space(2.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(space(2.0))
                            .child(icon("arrow-down-to-line", 14.0, p.accent))
                            .child(
                                div()
                                    .text_size(px(type_scale::SMALL))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.text)
                                    .child("Updating 2 mods"),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(type_scale::CAPTION))
                            .text_color(p.text_muted)
                            .child("64%"),
                    ),
            )
            .child(crate::ui::components::progress_bar(0.64, cx))
    }

    fn account_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();
        div()
            .flex()
            .items_center()
            .gap(space(2.5))
            .h(px(52.0))
            .px(space(4.0))
            .border_t_1()
            .border_color(p.border)
            .child(
                div()
                    .size(px(26.0))
                    .rounded(px(radius::SM))
                    .bg(p.raised)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("user", 14.0, p.text_secondary)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(type_scale::BODY))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(p.text)
                            .child("Steve"),
                    )
                    .child(
                        div()
                            .text_size(px(type_scale::CAPTION))
                            .text_color(p.text_muted)
                            .child("Microsoft account"),
                    ),
            )
            .child(
                div()
                    .id("open-settings")
                    .size(px(28.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(radius::SM))
                    .cursor_pointer()
                    .hover(|s| s.bg(p.hover))
                    .on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.navigate(Route::Gallery, cx)
                        }),
                    )
                    .child(icon("settings", 16.0, p.text_muted)),
            )
    }

    fn page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        match self.route.clone() {
            Route::Home => home::render(self, window, cx),
            Route::Library => library::render(self, window, cx),
            Route::Discover => self.discover.clone().into_any_element(),
            Route::Instance(id) => instance::render(self, id, self.instance_tab, window, cx),
            Route::Gallery => gallery::render(self, window, cx),
        }
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();
        let page = self.page(window, cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(p.app)
            .text_color(p.text)
            .font_family(theme::fonts::UI)
            .text_size(px(type_scale::BODY))
            .child(self.title_bar(cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.sidebar(cx))
                    .child(div().flex_1().min_w_0().h_full().child(page)),
            )
    }
}

/// Toggle helpers used by the design-system page.
pub fn set_look(appearance: Appearance, accent: Accent, cx: &mut App) {
    theme::apply(appearance, accent, cx);
}

pub fn running_chip(secs: u64, cx: &App) -> gpui_kit::Div {
    let p = cx.palette();
    state_chip(
        format!("Running · {}", crate::ui::components::short_duration(secs)),
        p.running,
        p.running_soft,
    )
}
