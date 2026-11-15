//! The window frame: title bar, sidebar, page area, dialogs and notices.

use gpui_kit::component::TitleBar;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Entity, FontWeight, ImageSource, SharedString,
    Subscription, Window, div, px,
};

use crate::state::{AppState, BackendStatus, InstanceInfo};
use crate::theme::{self, PaletteExt, radius, space, type_scale};
use crate::ui::components::{
    Button, empty_state, icon, progress_bar, short_duration, state_chip, thumb,
};
use crate::ui::create_instance::{CreateInstanceDialog, DialogEvent};
use crate::ui::instance_picker::{InstancePicker, ProjectSummary};
use crate::ui::pages::discover::DiscoverEvent;
use crate::ui::pages::instance::InstancePage;
use crate::ui::pages::instance::InstancePageEvent;
use crate::ui::pages::move_in::MoveInPage;
use crate::ui::pages::project::{ProjectEvent, ProjectPage};
use crate::ui::pages::settings::{self, Section as SettingsSection};
use crate::ui::pages::{discover::DiscoverPage, gallery, home, library};

/// Width of the left sidebar.
pub const SIDEBAR_WIDTH: f32 = 228.0;
/// Height of the window title bar (macOS traffic lights are centered on it).
pub const TITLE_BAR_HEIGHT: f32 = 40.0;

#[derive(Clone, Debug, PartialEq)]
pub enum Route {
    Home,
    Library,
    Discover,
    Instance(String),
    /// A Modrinth project, optionally being added to an instance.
    Project(String, Option<String>),
    Settings(SettingsSection),
    Gallery,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LibrarySort {
    #[default]
    LastPlayed,
    Name,
    Created,
}

impl LibrarySort {
    pub fn label(self) -> &'static str {
        match self {
            LibrarySort::LastPlayed => "Last played",
            LibrarySort::Name => "Name",
            LibrarySort::Created => "Date created",
        }
    }
    fn next(self) -> Self {
        match self {
            LibrarySort::LastPlayed => LibrarySort::Name,
            LibrarySort::Name => LibrarySort::Created,
            LibrarySort::Created => LibrarySort::LastPlayed,
        }
    }
}

pub struct Shell {
    route: Route,
    back: Vec<Route>,
    forward: Vec<Route>,
    state: Entity<AppState>,
    discover: Entity<DiscoverPage>,
    instance_page: Option<Entity<InstancePage>>,
    project_page: Option<Entity<ProjectPage>>,
    move_in: Option<Entity<MoveInPage>>,
    dialog: Option<gpui_kit::AnyView>,
    pub library_filter: Entity<InputState>,
    pub library_sort: LibrarySort,
    account_menu_open: bool,
    onboarding_step: Option<usize>,
    _subscriptions: Vec<Subscription>,
}

impl Shell {
    pub fn new(start: Route, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let discover = cx.new(|cx| DiscoverPage::new(window, cx));
        let library_filter =
            cx.new(|cx| InputState::new(window, cx).placeholder("Filter instances"));
        let subscriptions = vec![
            cx.observe(&state, |_, _, cx| cx.notify()),
            cx.subscribe_in(
                &discover,
                window,
                |this, _, event: &DiscoverEvent, window, cx| match event {
                    DiscoverEvent::OpenProject { id, target } => {
                        this.navigate(Route::Project(id.clone(), target.clone()), window, cx)
                    }
                    DiscoverEvent::AddTo(summary) => this.open_picker(summary.clone(), window, cx),
                },
            ),
            cx.subscribe(&library_filter, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
        ];
        let mut shell = Shell {
            route: Route::Home,
            back: Vec::new(),
            forward: Vec::new(),
            state,
            discover,
            instance_page: None,
            project_page: None,
            move_in: None,
            dialog: None,
            library_filter,
            library_sort: LibrarySort::LastPlayed,
            account_menu_open: false,
            onboarding_step: (!cx.global::<crate::prefs::Prefs>().onboarding_done).then_some(0),
            _subscriptions: subscriptions,
        };
        shell.set_route(start, window, cx);
        shell
    }

    pub fn state(&self) -> &Entity<AppState> {
        &self.state
    }

    fn set_route(&mut self, route: Route, window: &mut Window, cx: &mut Context<Self>) {
        if let Route::Instance(id) = &route {
            let reuse = self
                .instance_page
                .as_ref()
                .is_some_and(|page| page.read(cx).id() == id);
            if !reuse {
                let id = id.clone();
                let page = cx.new(|cx| InstancePage::new(id, window, cx));
                cx.subscribe_in(
                    &page,
                    window,
                    |this, _, event: &InstancePageEvent, window, cx| match event {
                        InstancePageEvent::AddContent(id) => {
                            this.open_discover(Some(id.clone()), window, cx)
                        }
                        InstancePageEvent::OpenProject(project) => {
                            this.navigate(Route::Project(project.clone(), None), window, cx)
                        }
                    },
                )
                .detach();
                self.instance_page = Some(page);
            }
        }
        if let Route::Project(id, target) = &route {
            let reuse = self
                .project_page
                .as_ref()
                .is_some_and(|page| page.read(cx).id() == id);
            if !reuse {
                let (id, target) = (id.clone(), target.clone());
                let page = cx.new(|cx| ProjectPage::new(id, target, cx));
                cx.subscribe_in(
                    &page,
                    window,
                    |this, _, event: &ProjectEvent, window, cx| match event {
                        ProjectEvent::AddTo(summary) => {
                            this.open_picker(summary.clone(), window, cx)
                        }
                    },
                )
                .detach();
                self.project_page = Some(page);
            }
        }
        if route == Route::Settings(SettingsSection::MoveIn) {
            self.ensure_move_in(window, cx);
        }
        self.route = route;
        self.account_menu_open = false;
        cx.notify();
    }

    pub fn navigate(&mut self, route: Route, window: &mut Window, cx: &mut Context<Self>) {
        if route == self.route {
            return;
        }
        let previous = self.route.clone();
        self.set_route(route, window, cx);
        self.back.push(previous);
        self.forward.clear();
    }

    fn go_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(route) = self.back.pop() {
            let current = self.route.clone();
            self.set_route(route, window, cx);
            self.forward.push(current);
        }
    }

    fn go_forward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(route) = self.forward.pop() {
            let current = self.route.clone();
            self.set_route(route, window, cx);
            self.back.push(current);
        }
    }

    fn ensure_move_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.move_in.is_none() {
            self.move_in = Some(cx.new(|cx| MoveInPage::new(window, cx)));
        }
    }

    pub fn open_settings(
        &mut self,
        section: SettingsSection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.navigate(Route::Settings(section), window, cx);
    }

    pub fn set_onboarding_step(
        &mut self,
        step: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if step == 1 {
            self.ensure_move_in(window, cx);
        }
        self.onboarding_step = Some(step.min(crate::ui::onboarding::STEPS.len() - 1));
        cx.notify();
    }

    pub fn finish_onboarding(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        crate::prefs::Prefs::finish_onboarding(cx);
        self.onboarding_step = None;
        let has_instances = !self.state.read(cx).instances.is_empty();
        self.set_route(
            if has_instances {
                Route::Library
            } else {
                Route::Home
            },
            window,
            cx,
        );
    }

    pub fn open_create_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dialog = cx.new(|cx| CreateInstanceDialog::new(window, cx));
        self.show_dialog(dialog, cx);
    }

    /// "Add to instance" for a project.
    pub fn open_picker(
        &mut self,
        project: ProjectSummary,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let picker = cx.new(|cx| InstancePicker::new(project, window, cx));
        self.show_dialog(picker, cx);
    }

    fn show_dialog<V: Render + gpui_kit::EventEmitter<DialogEvent>>(
        &mut self,
        dialog: Entity<V>,
        cx: &mut Context<Self>,
    ) {
        cx.subscribe(&dialog, |this, _, event: &DialogEvent, cx| match event {
            DialogEvent::Close => {
                this.dialog = None;
                cx.notify();
            }
        })
        .detach();
        self.dialog = Some(dialog.into());
        cx.notify();
    }

    /// Discover, either for everything or scoped to one instance.
    pub fn open_discover(
        &mut self,
        target: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.discover
            .update(cx, |page, cx| page.set_target(target, window, cx));
        self.navigate(Route::Discover, window, cx);
    }

    /// Run `f` against the open instance page, if one is showing.
    #[allow(dead_code)]
    pub fn with_instance_page(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut InstancePage, &mut Context<InstancePage>),
    ) {
        if let Some(page) = &self.instance_page {
            page.update(cx, f);
        }
    }

    pub fn cycle_sort(&mut self, cx: &mut Context<Self>) {
        self.library_sort = self.library_sort.next();
        cx.notify();
    }

    fn instance_title(&self, cx: &App) -> SharedString {
        match &self.route {
            Route::Project(..) => self
                .project_page
                .as_ref()
                .map(|p| p.read(cx).title())
                .unwrap_or_default(),
            Route::Instance(id) => self
                .state
                .read(cx)
                .instance(id)
                .map(|i| i.name.clone())
                .unwrap_or_else(|| "Instance".into()),
            _ => SharedString::default(),
        }
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
        let parent = match &self.route {
            Route::Instance(_) => Some(("Library", Route::Library)),
            Route::Project(..) => Some(("Discover", Route::Discover)),
            _ => None,
        };
        if let Some((parent_label, parent_route)) = parent {
            crumbs = crumbs
                .child(
                    div()
                        .id("crumb-parent")
                        .text_size(px(type_scale::BODY))
                        .text_color(p.text_muted)
                        .cursor_pointer()
                        .hover(|s| s.text_color(p.text_secondary))
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.navigate(parent_route.clone(), window, cx)
                        }))
                        .child(parent_label),
                )
                .child(icon("chevron-right", 14.0, p.text_disabled))
                .child(
                    div()
                        .text_size(px(type_scale::BODY))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(p.text)
                        .truncate()
                        .child(self.instance_title(cx)),
                );
        }

        let state = self.state.read(cx);
        let running: Option<(String, SharedString)> = state
            .running
            .keys()
            .next()
            .and_then(|id| state.instance(id).map(|i| (i.id.clone(), i.name.clone())));
        let running_count = state.running.len();

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
                    .child(nav_button("nav-back", "arrow-left", can_back).on_click(
                        cx.listener(|this, _: &ClickEvent, window, cx| this.go_back(window, cx)),
                    ))
                    .child(
                        nav_button("nav-forward", "arrow-right", can_forward).on_click(
                            cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.go_forward(window, cx)
                            }),
                        ),
                    )
                    .child(div().w(space(2.0)))
                    .child(crumbs),
            )
            .child(
                // The command palette trigger. Milestone 9 adds ⌘K; for now it opens Discover.
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
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.open_discover(None, window, cx)
                    }))
                    .child(icon("search", 14.0, p.text_muted))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(type_scale::SMALL))
                            .text_color(p.text_muted)
                            .child("Search Modrinth"),
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
                    .when_some(running, |d, (id, name)| {
                        let label: SharedString = if running_count > 1 {
                            format!("{name} +{}", running_count - 1).into()
                        } else {
                            name
                        };
                        d.child(
                            div()
                                .id("running-pill")
                                .flex()
                                .items_center()
                                .gap(space(1.5))
                                .h(px(28.0))
                                .px(space(2.5))
                                .rounded(px(radius::SM))
                                .bg(p.running_soft)
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.navigate(Route::Instance(id.clone()), window, cx)
                                }))
                                .child(div().size(px(6.0)).rounded_full().bg(p.running))
                                .child(
                                    div()
                                        .text_size(px(type_scale::SMALL))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(p.running)
                                        .child(label),
                                ),
                        )
                    }),
            )
    }

    // ------------------------------------------------------------ sidebar

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();
        let route = self.route.clone();
        let in_library = matches!(route, Route::Library | Route::Instance(_));

        let nav_item = |id: &'static str,
                        name: &'static str,
                        text: &'static str,
                        target: Route,
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
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    if target == Route::Discover {
                        this.open_discover(None, window, cx)
                    } else {
                        this.navigate(target.clone(), window, cx)
                    }
                }))
                .child(icon(
                    name,
                    16.0,
                    if active { p.accent } else { p.text_muted },
                ))
                .child(text)
        };

        let instances: Vec<InstanceInfo> = self
            .state
            .read(cx)
            .instances
            .iter()
            .take(6)
            .cloned()
            .collect();
        let running_ids: Vec<String> = self.state.read(cx).running.keys().cloned().collect();
        let mut recent = div().flex().flex_col().gap(space(0.5));
        for inst in &instances {
            let active = route == Route::Instance(inst.id.clone());
            let source = instance_icon(inst);
            let id = inst.id.clone();
            let running = running_ids.contains(&inst.id);
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
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.navigate(Route::Instance(id.clone()), window, cx)
                    }))
                    .child(thumb(source, &inst.name, 18.0, 4.0, cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(type_scale::BODY))
                            .text_color(if active { p.text } else { p.text_secondary })
                            .child(inst.name.clone()),
                    )
                    .when(running, |d| {
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
            .when(!instances.is_empty(), |d| {
                d.child(
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
            })
            .child(div().flex_1())
            .children(self.download_card(cx))
            .when(self.account_menu_open, |d| d.child(self.account_menu(cx)))
            .child(self.account_row(cx))
    }

    fn download_card(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let p = cx.palette().clone();
        let state = self.state.read(cx);
        let jobs: Vec<_> = state
            .jobs
            .iter()
            .filter(|j| j.is_active())
            .cloned()
            .collect();
        let job = jobs.first()?.clone();
        let extra = jobs.len().saturating_sub(1);
        let title: SharedString = if extra > 0 {
            format!("{} +{extra}", job.title).into()
        } else {
            job.title.clone()
        };
        let percent = if job.total > 0 {
            format!("{}%", (job.fraction() * 100.0).round() as u32)
        } else {
            String::new()
        };
        let target = job.instance_id.clone();
        Some(
            div()
                .id("download-card")
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
                .when_some(target, |d, id| {
                    d.cursor_pointer()
                        .hover(|s| s.border_color(p.border_strong))
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.navigate(Route::Instance(id.clone()), window, cx)
                        }))
                })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(space(2.0))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(space(2.0))
                                .min_w_0()
                                .child(icon("arrow-down-to-line", 14.0, p.accent))
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(type_scale::SMALL))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(p.text)
                                        .child(title),
                                ),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(type_scale::CAPTION))
                                .text_color(p.text_muted)
                                .child(percent),
                        ),
                )
                .child(progress_bar(job.fraction(), cx))
                .child(
                    div()
                        .text_size(px(type_scale::CAPTION))
                        .text_color(p.text_muted)
                        .child(job.phase),
                ),
        )
    }

    fn account_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();
        let state = self.state.read(cx);
        let active = state.active_account().map(|a| a.id);
        let accounts = state.accounts.clone();

        let mut menu = div()
            .mx(space(2.0))
            .mb(space(2.0))
            .p(space(1.0))
            .rounded(px(radius::MD))
            .bg(p.overlay)
            .border_1()
            .border_color(p.border_strong)
            .flex()
            .flex_col();
        for account in accounts {
            let id = account.id;
            let is_active = Some(id) == active;
            menu = menu.child(
                div()
                    .id(SharedString::from(format!("account-{id}")))
                    .flex()
                    .items_center()
                    .gap(space(2.0))
                    .h(px(30.0))
                    .px(space(2.0))
                    .rounded(px(radius::SM))
                    .cursor_pointer()
                    .hover(|s| s.bg(p.hover))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.state
                            .update(cx, |state, cx| state.set_default_account(id, cx));
                        this.account_menu_open = false;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_size(px(type_scale::BODY))
                            .text_color(p.text)
                            .child(account.name.clone()),
                    )
                    .when(is_active, |d| d.child(icon("check", 14.0, p.accent)))
                    .child(
                        div()
                            .id(SharedString::from(format!("remove-account-{id}")))
                            .size(px(22.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(4.0))
                            .hover(|s| s.bg(p.pressed))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                cx.stop_propagation();
                                this.state
                                    .update(cx, |state, cx| state.remove_account(id, cx));
                            }))
                            .child(icon("log-out", 13.0, p.text_muted)),
                    ),
            );
        }
        menu.child(
            div()
                .id("add-account")
                .flex()
                .items_center()
                .gap(space(2.0))
                .h(px(30.0))
                .px(space(2.0))
                .rounded(px(radius::SM))
                .cursor_pointer()
                .hover(|s| s.bg(p.hover))
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.account_menu_open = false;
                    crate::login::sign_in(cx);
                    cx.notify();
                }))
                .child(icon("user-plus", 14.0, p.text_secondary))
                .child(
                    div()
                        .text_size(px(type_scale::BODY))
                        .text_color(p.text_secondary)
                        .child("Add Microsoft account"),
                ),
        )
    }

    fn account_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();
        let account = self.state.read(cx).active_account().cloned();
        let (title, subtitle): (SharedString, SharedString) = match &account {
            Some(a) => (a.name.clone(), "Microsoft account".into()),
            None => ("Sign in".into(), "Needed to play".into()),
        };
        div()
            .flex()
            .items_center()
            .gap(space(2.0))
            .h(px(52.0))
            .px(space(2.0))
            .border_t_1()
            .border_color(p.border)
            .child(
                div()
                    .id("account-button")
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap(space(2.5))
                    .h(px(40.0))
                    .px(space(2.0))
                    .rounded(px(radius::SM))
                    .cursor_pointer()
                    .hover(|s| s.bg(p.hover))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        if this.state.read(cx).accounts.is_empty() {
                            crate::login::sign_in(cx);
                        } else {
                            this.account_menu_open = !this.account_menu_open;
                        }
                        cx.notify();
                    }))
                    .child(
                        div()
                            .size(px(26.0))
                            .flex_none()
                            .rounded(px(radius::SM))
                            .bg(if account.is_some() {
                                p.raised
                            } else {
                                p.accent_soft
                            })
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon(
                                if account.is_some() { "user" } else { "log-in" },
                                14.0,
                                if account.is_some() {
                                    p.text_secondary
                                } else {
                                    p.accent
                                },
                            )),
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
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_size(px(type_scale::CAPTION))
                                    .text_color(p.text_muted)
                                    .child(subtitle),
                            ),
                    )
                    .when(account.is_some(), |d| {
                        d.child(icon("chevrons-up-down", 14.0, p.text_muted))
                    }),
            )
            .child(
                div()
                    .id("open-design")
                    .size(px(28.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(radius::SM))
                    .cursor_pointer()
                    .hover(|s| s.bg(p.hover))
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.open_settings(SettingsSection::Appearance, window, cx)
                    }))
                    .child(icon(
                        "settings",
                        16.0,
                        if matches!(self.route, Route::Settings(_)) {
                            p.accent
                        } else {
                            p.text_muted
                        },
                    )),
            )
    }

    fn page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if let Some(step) = self.onboarding_step {
            let move_in = self.move_in.clone();
            return crate::ui::onboarding::render(self, step, move_in, cx);
        }
        match self.state.read(cx).status.clone() {
            BackendStatus::Starting
                if !matches!(
                    self.route,
                    Route::Discover | Route::Gallery | Route::Settings(_)
                ) =>
            {
                return div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(type_scale::BODY))
                    .text_color(cx.palette().text_muted)
                    .child("Getting your library ready…")
                    .into_any_element();
            }
            BackendStatus::Failed(error) => {
                return empty_state(
                    "circle-alert",
                    "Calcite couldn't open its data folder",
                    error,
                    None,
                    cx,
                )
                .size_full()
                .into_any_element();
            }
            _ => {}
        }
        match self.route.clone() {
            Route::Home => home::render(self, window, cx),
            Route::Library => library::render(self, window, cx),
            Route::Discover => self.discover.clone().into_any_element(),
            Route::Project(..) => match &self.project_page {
                Some(page) => page.clone().into_any_element(),
                None => div().into_any_element(),
            },
            Route::Instance(_) => match &self.instance_page {
                Some(page) => page.clone().into_any_element(),
                None => div().into_any_element(),
            },
            Route::Settings(section) => {
                let move_in = self.move_in.clone();
                settings::render(self, section, move_in, window, cx)
            }
            Route::Gallery => gallery::render(self, window, cx),
        }
    }

    fn notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = cx.palette().clone();
        let notice = self.state.read(cx).notice.clone()?;
        let accent = if notice.is_error { p.danger } else { p.accent };
        Some(
            div()
                .absolute()
                .bottom(space(5.0))
                .right(space(5.0))
                .w(px(380.0))
                .p(space(4.0))
                .rounded(px(radius::LG))
                .bg(p.overlay)
                .border_1()
                .border_color(p.border_strong)
                .shadow_lg()
                .flex()
                .gap(space(3.0))
                .child(icon(
                    if notice.is_error {
                        "circle-alert"
                    } else {
                        "info"
                    },
                    18.0,
                    accent,
                ))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .gap(space(1.0))
                        .child(
                            div()
                                .text_size(px(type_scale::BODY))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(p.text)
                                .child(notice.title.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(type_scale::SMALL))
                                .text_color(p.text_secondary)
                                .child(notice.body.clone()),
                        ),
                )
                .child(
                    Button::new("dismiss-notice")
                        .icon("x")
                        .ghost()
                        .small()
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.state.update(cx, |state, cx| state.dismiss_notice(cx))
                        })),
                )
                .into_any_element(),
        )
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();
        let page = self.page(window, cx);
        let notice = self.notice(cx);
        let dialog = self.dialog.clone();
        div()
            .size_full()
            .relative()
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
                    .when(self.onboarding_step.is_none(), |d| {
                        d.child(self.sidebar(cx))
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .relative()
                            .child(page)
                            .children(notice),
                    ),
            )
            .when_some(dialog, |d, dialog| {
                d.child(
                    div()
                        .id("dialog-scrim")
                        .absolute()
                        .inset_0()
                        .bg(p.scrim)
                        .occlude()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(dialog),
                )
            })
    }
}

/// An instance's own icon file, if it has one on disk.
pub fn instance_icon(inst: &InstanceInfo) -> Option<ImageSource> {
    inst.icon_path
        .as_ref()
        .filter(|p| p.exists())
        .map(|p| ImageSource::from(p.clone()))
}

pub fn running_chip(secs: u64, cx: &App) -> gpui_kit::Div {
    let p = cx.palette();
    state_chip(
        format!("Running · {}", short_duration(secs)),
        p.running,
        p.running_soft,
    )
}
