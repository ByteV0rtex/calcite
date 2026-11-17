//! The window frame: title bar, sidebar, page area, dialogs and notices.

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Sizable as _, TitleBar};
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
use crate::ui::instance_settings::InstanceSettings;
use crate::ui::pages::discover::DiscoverEvent;
use crate::ui::pages::instance::InstancePage;
use crate::ui::pages::instance::InstancePageEvent;
use crate::ui::pages::move_in::MoveInPage;
use crate::ui::pages::project::{ProjectEvent, ProjectPage};
use crate::ui::pages::settings::{self, Section as SettingsSection};
use crate::ui::pages::{discover::DiscoverPage, gallery, home, library};
use theseus::install::InstallJobStatus;

use crate::ui::palette::{CommandPalette, PaletteCommand, PaletteEvent};

gpui_kit::actions!(calcite, [TogglePalette]);

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
    Skins,
    Screenshots,
    /// Servers running on this computer.
    Hosting,
    /// A Modrinth user's page.
    User(String),
    Gallery,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LibrarySort {
    #[default]
    LastPlayed,
    Name,
    Played,
    Created,
    GameVersion,
    Loader,
}

impl LibrarySort {
    pub const ALL: [LibrarySort; 6] = [
        LibrarySort::LastPlayed,
        LibrarySort::Name,
        LibrarySort::Played,
        LibrarySort::Created,
        LibrarySort::GameVersion,
        LibrarySort::Loader,
    ];
    pub fn label(self) -> &'static str {
        match self {
            LibrarySort::LastPlayed => "Last played",
            LibrarySort::Name => "Name",
            LibrarySort::Played => "Hours played",
            LibrarySort::Created => "Date created",
            LibrarySort::GameVersion => "Game version",
            LibrarySort::Loader => "Loader",
        }
    }
}

/// How the Library splits instances into sections.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LibraryGroup {
    #[default]
    Groups,
    Loader,
    GameVersion,
    Kind,
    None,
}

impl LibraryGroup {
    pub const ALL: [LibraryGroup; 5] = [
        LibraryGroup::Groups,
        LibraryGroup::Loader,
        LibraryGroup::GameVersion,
        LibraryGroup::Kind,
        LibraryGroup::None,
    ];
    pub fn label(self) -> &'static str {
        match self {
            LibraryGroup::Groups => "Your groups",
            LibraryGroup::Loader => "Loader",
            LibraryGroup::GameVersion => "Game version",
            LibraryGroup::Kind => "Type",
            LibraryGroup::None => "No grouping",
        }
    }
}

/// Which instances the Library shows by kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum KindFilter {
    #[default]
    All,
    Modpacks,
    Custom,
    Shared,
}

/// Library view state (kept while the app runs).
#[derive(Default)]
pub struct LibraryView {
    pub sort: LibrarySort,
    pub group: LibraryGroup,
    pub kind: KindFilter,
    pub loader: Option<theseus::data::ModLoader>,
    pub show_hidden: bool,
    pub selecting: bool,
    pub selected: std::collections::HashSet<String>,
    pub confirm_delete: bool,
    /// Which toolbar menu is open ("sort", "group", "add-to-group").
    pub menu: Option<&'static str>,
    /// Name field for "New group" from the selection bar.
    pub new_group: Option<Entity<InputState>>,
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
    game_settings: Option<Entity<crate::ui::pages::game_settings::GameSettings>>,
    storage: Option<Entity<crate::ui::pages::storage::StoragePage>>,
    advanced: Option<Entity<crate::ui::pages::advanced::AdvancedSettings>>,
    /// Re-read each time it's opened.
    screenshots: Option<Entity<crate::ui::pages::screenshots::ScreenshotsPage>>,
    /// Kept while Calcite runs, so the open console stays put.
    hosting: Option<Entity<crate::ui::pages::hosting::HostingPage>>,
    /// Only alive while Skins is open (it holds rendered thumbnails).
    skins: Option<Entity<crate::ui::pages::skins::SkinsPage>>,
    dialog: Option<gpui_kit::AnyView>,
    /// The ⌘K command palette, while it's open.
    palette: Option<Entity<CommandPalette>>,
    /// Counts dialogs and palettes opened, so each one animates in.
    overlay_seq: usize,
    pub library_filter: Entity<InputState>,
    /// The title bar search field.
    global_search: Entity<InputState>,
    /// "Change version" asked for from instance settings; opened on the
    /// next render (it needs the window).
    pending_change_version: Option<String>,
    /// Same for "Switch modpack version".
    pending_switch_modpack: Option<String>,
    pub library: LibraryView,
    account_menu_open: bool,
    downloads_open: bool,
    all_friends: bool,
    user_page: Option<Entity<crate::ui::pages::user::UserPage>>,
    onboarding_step: Option<usize>,
    _subscriptions: Vec<Subscription>,
}

impl Shell {
    pub fn new(start: Route, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let discover = cx.new(|cx| DiscoverPage::new(window, cx));
        let library_filter =
            cx.new(|cx| InputState::new(window, cx).placeholder("Filter instances"));
        let global_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search mods, modpacks, shaders…"));
        let subscriptions = vec![
            // Typing in the title bar searches Modrinth live, on Discover.
            cx.subscribe_in(
                &global_search,
                window,
                |this, input, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change | InputEvent::PressEnter { .. }) {
                        let text = input.read(cx).value().to_string();
                        if text.trim().is_empty() && this.route != Route::Discover {
                            return;
                        }
                        if this.route != Route::Discover {
                            this.open_discover(None, window, cx);
                        }
                        this.discover
                            .update(cx, |d, cx| d.search_for(text, window, cx));
                    }
                },
            ),
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
            game_settings: None,
            storage: None,
            advanced: None,
            skins: None,
            screenshots: None,
            hosting: None,
            dialog: None,
            palette: None,
            overlay_seq: 0,
            library_filter,
            global_search,
            pending_change_version: None,
            pending_switch_modpack: None,
            library: LibraryView::default(),
            account_menu_open: false,
            downloads_open: false,
            all_friends: false,
            user_page: None,
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
                        InstancePageEvent::OpenSettings(id) => {
                            this.open_instance_settings(id.clone(), window, cx)
                        }
                        InstancePageEvent::Server(id, existing) => {
                            let dialog = cx.new(|cx| {
                                crate::ui::server_dialog::ServerDialog::new(
                                    id.clone(),
                                    existing.clone(),
                                    window,
                                    cx,
                                )
                            });
                            this.show_dialog(dialog, cx);
                        }
                        InstancePageEvent::RenameWorld(dir, folder, name) => {
                            let dialog = cx.new(|cx| {
                                crate::ui::rename_world::RenameWorldDialog::new(
                                    dir.clone(),
                                    folder.clone(),
                                    name.clone(),
                                    window,
                                    cx,
                                )
                            });
                            this.show_dialog(dialog, cx);
                        }
                        InstancePageEvent::Share(id) => {
                            let dialog =
                                cx.new(|cx| crate::ui::share::ShareDialog::new(id.clone(), cx));
                            this.show_dialog(dialog, cx);
                        }
                        InstancePageEvent::OpenProject(project) => {
                            this.navigate(Route::Project(project.clone(), None), window, cx)
                        }
                        InstancePageEvent::PickVersion(id, file) => {
                            use crate::ui::version_picker::VersionPicker;
                            let dialog = cx.new(|cx| match file {
                                Some(file) => {
                                    VersionPicker::file(id.clone(), file.clone(), window, cx)
                                }
                                None => VersionPicker::modpack(id.clone(), window, cx),
                            });
                            this.show_dialog(dialog, cx);
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
        if route == Route::Settings(SettingsSection::Game) && self.game_settings.is_none() {
            self.game_settings =
                Some(cx.new(|cx| crate::ui::pages::game_settings::GameSettings::new(window, cx)));
        }
        if route == Route::Settings(SettingsSection::MoveIn) {
            self.ensure_move_in(window, cx);
        }
        if let Route::User(id) = &route {
            let reuse = self
                .user_page
                .as_ref()
                .is_some_and(|p| p.read(cx).id() == id);
            if !reuse {
                use crate::ui::pages::user::{UserEvent, UserPage};
                let page = cx.new(|cx| UserPage::new(id.clone(), cx));
                cx.subscribe_in(
                    &page,
                    window,
                    |this, _, event: &UserEvent, window, cx| match event {
                        UserEvent::OpenProject(id) => {
                            this.navigate(Route::Project(id.clone(), None), window, cx)
                        }
                    },
                )
                .detach();
                self.user_page = Some(page);
            }
        }
        if route == Route::Hosting && self.hosting.is_none() {
            self.hosting = Some(cx.new(crate::ui::pages::hosting::HostingPage::new));
        }
        if route == Route::Screenshots && self.route != route {
            self.screenshots = Some(cx.new(crate::ui::pages::screenshots::ScreenshotsPage::new));
        }
        if route == Route::Skins {
            if self.skins.is_none() {
                self.skins = Some(cx.new(crate::ui::pages::skins::SkinsPage::new));
            }
        } else {
            self.skins = None;
        }
        if route == Route::Settings(SettingsSection::Advanced) && self.advanced.is_none() {
            self.advanced =
                Some(cx.new(|cx| crate::ui::pages::advanced::AdvancedSettings::new(window, cx)));
        }
        // Storage is measured fresh each time it's opened.
        if route == Route::Settings(SettingsSection::Storage) && self.route != route {
            use crate::ui::pages::storage::{StorageEvent, StoragePage};
            let page = cx.new(StoragePage::new);
            cx.subscribe_in(
                &page,
                window,
                |this, _, event: &StorageEvent, window, cx| match event {
                    StorageEvent::OpenInstance(id) => {
                        this.navigate(Route::Instance(id.clone()), window, cx)
                    }
                },
            )
            .detach();
            self.storage = Some(page);
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
        if step == 2 {
            self.ensure_move_in(window, cx);
            if let Some(page) = &self.move_in {
                page.update(cx, |p, cx| {
                    p.embedded = true;
                    cx.notify();
                });
            }
        }
        self.onboarding_step = Some(step.min(crate::ui::onboarding::STEPS.len() - 1));
        cx.notify();
    }

    pub fn finish_onboarding(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        crate::prefs::Prefs::finish_onboarding(cx);
        self.onboarding_step = None;
        if let Some(page) = &self.move_in {
            page.update(cx, |p, cx| {
                p.embedded = false;
                cx.notify();
            });
        }
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

    pub fn open_instance_settings(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dialog = cx.new(|cx| InstanceSettings::new(id, window, cx));
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
            DialogEvent::WorldsChanged => {
                this.dialog = None;
                this.with_instance_page(cx, |p, cx| p.load_worlds(cx));
                cx.notify();
            }
            DialogEvent::ChangeVersion(id) => {
                this.dialog = None;
                this.pending_change_version = Some(id.clone());
                cx.notify();
            }
            DialogEvent::SwitchModpack(id) => {
                this.dialog = None;
                this.pending_switch_modpack = Some(id.clone());
                cx.notify();
            }
            DialogEvent::ContentChanged => {
                this.dialog = None;
                this.with_instance_page(cx, |p, cx| p.content_changed(cx));
                cx.notify();
            }
        })
        .detach();
        self.dialog = Some(dialog.into());
        self.overlay_seq += 1;
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

    pub fn open_profile_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dialog = cx.new(|cx| crate::ui::profile_editor::ProfileEditor::new(window, cx));
        self.show_dialog(dialog, cx);
    }

    /// Discover's Servers tab.
    pub fn open_servers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.discover.update(cx, |d, cx| {
            d.set_target(None, window, cx);
            d.show_servers(window, cx);
        });
        self.navigate(Route::Discover, window, cx);
    }

    pub fn all_friends_shown(&self) -> bool {
        self.all_friends
    }

    pub fn toggle_all_friends(&mut self, cx: &mut Context<Self>) {
        self.all_friends = !self.all_friends;
        cx.notify();
    }

    /// Sort out dropped files: modpacks become instances, everything else
    /// goes into the open instance or one the player picks.
    pub fn handle_drop(
        &mut self,
        paths: Vec<std::path::PathBuf>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use calcite_core::dropped::{Dropped, classify};
        let sorted = cx.background_spawn(async move {
            paths
                .into_iter()
                .map(|p| {
                    let kind = classify(&p);
                    (p, kind)
                })
                .collect::<Vec<_>>()
        });
        let target = match &self.route {
            Route::Instance(id) => Some(id.clone()),
            _ => None,
        };
        cx.spawn(async move |this, cx| {
            let sorted = sorted.await;
            let _ = this.update(cx, |this, cx| {
                let name = |p: &std::path::PathBuf| {
                    p.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default()
                };
                let mut into_instance = Vec::new();
                let mut skipped = Vec::new();
                for (path, kind) in sorted {
                    match kind {
                        Dropped::Modpack => this
                            .state
                            .update(cx, |s, cx| s.install_modpack_file(path, cx)),
                        Dropped::CurseForgePack
                            if calcite_core::curseforge::api_key().is_some() =>
                        {
                            this.state
                                .update(cx, |s, cx| s.install_curseforge_pack(Err(path), cx))
                        }
                        Dropped::CurseForgePack => skipped.push(format!(
                            "{} is a CurseForge modpack: import it from Settings → Move in.",
                            name(&path)
                        )),
                        Dropped::Unknown => skipped.push(format!(
                            "{} isn't a modpack, mod, pack or world.",
                            name(&path)
                        )),
                        kind => into_instance.push((path, kind)),
                    }
                }
                if !skipped.is_empty() {
                    this.state.update(cx, |s, cx| {
                        s.notify_error("Some files were left out", skipped.join("\n"));
                        cx.notify();
                    });
                }
                if into_instance.is_empty() {
                    return;
                }
                match target {
                    Some(id) => this
                        .state
                        .update(cx, |s, cx| s.add_dropped(id, into_instance, cx)),
                    None => {
                        let dialog =
                            cx.new(|_| crate::ui::drop_dialog::DropDialog::new(into_instance));
                        this.show_dialog(dialog, cx);
                    }
                }
            });
        })
        .detach();
    }

    /// Open the ⌘K palette, or close it if it's open.
    pub fn toggle_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.take().is_some() {
            cx.notify();
            return;
        }
        let palette = cx.new(|cx| CommandPalette::new(window, cx));
        cx.subscribe_in(
            &palette,
            window,
            |this, _, event: &PaletteEvent, window, cx| {
                match event {
                    PaletteEvent::Close => this.palette = None,
                    PaletteEvent::Run(command) => this.run_palette(command.clone(), window, cx),
                }
                cx.notify();
            },
        )
        .detach();
        self.palette = Some(palette);
        self.overlay_seq += 1;
        cx.notify();
    }

    /// Dev hook: type into the open palette.
    #[cfg_attr(not(feature = "snapshot"), allow(dead_code))]
    pub fn palette_query(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(palette) = &self.palette {
            palette.update(cx, |p, cx| p.set_query(text, window, cx));
        }
    }

    fn run_palette(
        &mut self,
        command: PaletteCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match command {
            PaletteCommand::Go(route) => self.navigate(route, window, cx),
            PaletteCommand::Play(id) => self.state.update(cx, |s, cx| s.play(&id, cx)),
            PaletteCommand::OpenFolder(id) => {
                let fetch = crate::backend::Backend::global(cx)
                    .call(async move { theseus::instance::get_full_path(&id).await });
                cx.spawn(async move |_, cx| {
                    if let Ok(path) = fetch.await {
                        cx.update(|cx| cx.open_with_system(&path));
                    }
                })
                .detach();
            }
            PaletteCommand::NewInstance => self.open_create_dialog(window, cx),
            PaletteCommand::FindServer => self.open_servers(window, cx),
            PaletteCommand::SearchModrinth(text) => self.type_search(&text, window, cx),
            PaletteCommand::OpenDataFolder => {
                if let Some(dir) = crate::backend::data_dir() {
                    cx.open_with_system(&dir);
                }
            }
        }
    }

    /// Search Modrinth for `text` from the title bar.
    pub fn type_search(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.global_search
            .update(cx, |s, cx| s.set_value(text.to_string(), window, cx));
        self.open_discover(None, window, cx);
        let text = text.to_string();
        self.discover
            .update(cx, |d, cx| d.search_for(text, window, cx));
    }

    /// Dev hook: expand the downloads panel.
    #[cfg_attr(not(feature = "snapshot"), allow(dead_code))]
    pub fn open_downloads(&mut self, cx: &mut Context<Self>) {
        self.downloads_open = true;
        cx.notify();
    }

    /// Dev hook: set Library view options by name.
    #[cfg_attr(not(feature = "snapshot"), allow(dead_code))]
    pub fn dev_library(&mut self, word: &str, cx: &mut Context<Self>) {
        match word {
            "lib-select" => {
                self.library.selecting = true;
                let ids: Vec<String> = self
                    .state
                    .read(cx)
                    .instances
                    .iter()
                    .take(3)
                    .map(|i| i.id.clone())
                    .collect();
                self.library.selected = ids.into_iter().collect();
            }
            "lib-sort-menu" => self.library.menu = Some("sort"),
            "lib-group-loader" => self.library.group = LibraryGroup::Loader,
            "lib-compact" => crate::prefs::Prefs::set_compact(true, cx),
            "lib-cards" => crate::prefs::Prefs::set_compact(false, cx),
            _ => {}
        }
        cx.notify();
    }

    /// Dev hook: open "Change version" for an instance.
    #[cfg_attr(not(feature = "snapshot"), allow(dead_code))]
    pub fn request_change_version(&mut self, id: String, cx: &mut Context<Self>) {
        self.pending_change_version = Some(id);
        cx.notify();
    }

    pub fn open_join(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dialog = cx.new(|cx| crate::ui::share::JoinDialog::new(window, cx));
        self.show_dialog(dialog, cx);
    }

    pub fn open_add_friend(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dialog = cx.new(|cx| crate::ui::friends::AddFriendDialog::new(window, cx));
        self.show_dialog(dialog, cx);
    }

    #[cfg_attr(not(feature = "snapshot"), allow(dead_code))]
    pub fn skins_page(&self) -> Option<Entity<crate::ui::pages::skins::SkinsPage>> {
        self.skins.clone()
    }

    pub fn advanced_page(&self) -> Option<Entity<crate::ui::pages::advanced::AdvancedSettings>> {
        self.advanced.clone()
    }

    #[cfg_attr(not(feature = "snapshot"), allow(dead_code))]
    pub fn storage_page(&self) -> Option<Entity<crate::ui::pages::storage::StoragePage>> {
        self.storage.clone()
    }

    /// Run `f` against the open instance page, if one is showing.
    pub fn with_instance_page(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut InstancePage, &mut Context<InstancePage>),
    ) {
        if let Some(page) = &self.instance_page {
            page.update(cx, f);
        }
    }

    /// Dev hook: the open instance page.
    #[cfg_attr(not(feature = "snapshot"), allow(dead_code))]
    pub fn instance_page_entity(&self) -> Option<Entity<InstancePage>> {
        self.instance_page.clone()
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
                // Search Modrinth from anywhere; results show on Discover.
                div()
                    .id("global-search")
                    .flex()
                    .items_center()
                    .gap(space(1.0))
                    .h(px(28.0))
                    .w(px(440.0))
                    // Sit 6px below the top edge, a touch lower than centre.
                    .relative()
                    .top(px(1.0))
                    .pl(space(3.0))
                    .rounded(px(radius::SM))
                    .bg(p.surface)
                    .border_1()
                    .border_color(p.border)
                    .hover(|s| s.border_color(p.border_strong))
                    .child(icon("search", 14.0, p.text_muted))
                    .child(
                        div().flex_1().min_w_0().child(
                            Input::new(&self.global_search)
                                .appearance(false)
                                .cleanable(true)
                                .small(),
                        ),
                    )
                    .child(
                        div()
                            .id("palette-hint")
                            .flex_none()
                            .px(space(1.5))
                            .rounded(px(4.0))
                            .border_1()
                            .border_color(p.border)
                            .text_size(px(type_scale::CAPTION))
                            .text_color(p.text_muted)
                            .cursor_pointer()
                            .hover(|s| s.text_color(p.text).bg(p.hover))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.toggle_palette(window, cx)
                            }))
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
                    ))
                    .child(nav_item(
                        "nav-skins",
                        "shirt",
                        "Skins",
                        Route::Skins,
                        route == Route::Skins,
                        cx,
                    ))
                    .child(nav_item(
                        "nav-screenshots",
                        "image",
                        "Screenshots",
                        Route::Screenshots,
                        route == Route::Screenshots,
                        cx,
                    ))
                    .child(nav_item(
                        "nav-hosting",
                        "server",
                        "Hosting",
                        Route::Hosting,
                        route == Route::Hosting,
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
            .children(crate::ui::friends::sidebar_section(self, cx))
            .child(div().flex_1())
            .children(self.update_card(cx))
            .children(self.download_card(cx))
            .when(self.account_menu_open, |d| d.child(self.account_menu(cx)))
            .child(self.account_row(cx))
    }

    fn update_card(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let update = self.state.read(cx).update.clone()?;
        let p = cx.palette().clone();
        let url = update.url.clone();
        Some(
            div()
                .id("update-card")
                .mx(space(2.0))
                .mb(space(2.0))
                .p(space(2.5))
                .rounded(px(radius::MD))
                .bg(p.accent_soft)
                .flex()
                .items_center()
                .gap(space(2.0))
                .cursor_pointer()
                .on_click(move |_, _, cx| cx.open_url(&url))
                .child(icon("arrow-up", 14.0, p.accent_text))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(type_scale::SMALL))
                        .text_color(p.text)
                        .child(format!("Calcite {} is out", update.version)),
                )
                .child(
                    div()
                        .text_size(px(type_scale::CAPTION))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(p.accent_text)
                        .child("Get it"),
                ),
        )
    }

    fn download_card(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        use crate::state::JobAction;
        let p = cx.palette().clone();
        let state = self.state.read(cx);
        let jobs: Vec<_> = state
            .jobs
            .iter()
            .filter(|j| j.is_active() || j.needs_attention())
            .cloned()
            .collect();
        let job = jobs.first()?.clone();
        let waiting = jobs.iter().filter(|j| j.needs_attention()).count();
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
        let open = self.downloads_open;

        let job_row = |ix: usize, job: &crate::state::Job, cx: &mut Context<Self>| {
            let status: SharedString = if job.paused {
                "Paused".into()
            } else {
                match job.status {
                    InstallJobStatus::Failed => {
                        job.error.clone().unwrap_or_else(|| "Failed".into()).into()
                    }
                    InstallJobStatus::Interrupted => "Interrupted; resume to finish".into(),
                    _ => job.phase.into(),
                }
            };
            let mut actions: Vec<(JobAction, &'static str)> = Vec::new();
            if job.paused || job.status == InstallJobStatus::Interrupted {
                actions.push((JobAction::Resume, "play"));
            } else if job.is_active() && job.can_pause {
                actions.push((JobAction::Pause, "pause"));
            }
            if job.status == InstallJobStatus::Failed {
                actions.push((JobAction::Retry, "rotate-ccw"));
            }
            if job.can_cancel && (job.is_active() || job.paused) {
                actions.push((JobAction::Cancel, "x"));
            } else if !job.is_active() {
                actions.push((JobAction::Dismiss, "x"));
            }
            let target = job.instance_id.clone();
            div()
                .flex()
                .flex_col()
                .gap(space(1.5))
                .py(space(2.0))
                .when(ix > 0, |d| d.border_t_1().border_color(p.border))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space(1.0))
                        .child(
                            div()
                                .id(SharedString::from(format!("job-title-{ix}")))
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(type_scale::SMALL))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.text)
                                .when_some(target, |d, id| {
                                    d.cursor_pointer()
                                        .hover(|s| s.text_color(p.accent))
                                        .on_click(cx.listener(
                                            move |this, _: &ClickEvent, window, cx| {
                                                this.navigate(
                                                    Route::Instance(id.clone()),
                                                    window,
                                                    cx,
                                                )
                                            },
                                        ))
                                })
                                .child(job.title.clone()),
                        )
                        .children(actions.into_iter().map(|(action, glyph)| {
                            let id = job.id.clone();
                            Button::new(SharedString::from(format!("job-{ix}-{action:?}")))
                                .icon(glyph)
                                .ghost()
                                .small()
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    let id = id.clone();
                                    this.state.update(cx, |s, cx| s.job_action(id, action, cx));
                                }))
                        })),
                )
                .when(job.is_active() && !job.paused, |d| {
                    d.child(progress_bar(job.fraction(), cx))
                })
                .child(
                    div()
                        .text_size(px(type_scale::CAPTION))
                        .text_color(if job.status == InstallJobStatus::Failed {
                            p.danger
                        } else {
                            p.text_muted
                        })
                        .child(status),
                )
        };

        let mut card = div()
            .id("download-card")
            .mx(space(2.0))
            .mb(space(2.0))
            .p(space(2.5))
            .rounded(px(radius::MD))
            .bg(p.surface)
            .border_1()
            .border_color(if waiting > 0 {
                p.warning_soft
            } else {
                p.border
            })
            .flex()
            .flex_col()
            .gap(space(2.0));
        if open {
            card = card.child(
                div()
                    .id("downloads-close")
                    .flex()
                    .items_center()
                    .justify_between()
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.downloads_open = false;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .text_size(px(type_scale::CAPTION))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(p.text_muted)
                            .child(format!("Downloads · {}", jobs.len())),
                    )
                    .child(icon("chevron-down", 14.0, p.text_muted)),
            );
            let mut list = div()
                .id("downloads-list")
                .flex()
                .flex_col()
                .max_h(px(320.0))
                .overflow_y_scroll();
            for (ix, job) in jobs.iter().enumerate() {
                list = list.child(job_row(ix, job, cx));
            }
            card = card.child(list);
        } else {
            card = card
                .cursor_pointer()
                .hover(|s| s.border_color(p.border_strong))
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.downloads_open = true;
                    cx.notify();
                }))
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
                                .child(icon(
                                    if waiting > 0 {
                                        "triangle-alert"
                                    } else {
                                        "arrow-down-to-line"
                                    },
                                    14.0,
                                    if waiting > 0 { p.warning } else { p.accent },
                                ))
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
                .when(job.is_active() && !job.paused, |d| {
                    d.child(progress_bar(job.fraction(), cx))
                })
                .child(
                    div()
                        .text_size(px(type_scale::CAPTION))
                        .text_color(p.text_muted)
                        .child(if waiting > 0 {
                            format!("{waiting} waiting for you · click to see")
                        } else {
                            job.phase.to_string()
                        }),
                );
        }
        Some(card)
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
        let face = account
            .as_ref()
            .and_then(|a| self.state.read(cx).face_for(a.id));
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
                            .overflow_hidden()
                            .child(match face {
                                Some(face) => gpui_kit::img(face).size_full().into_any_element(),
                                None => icon(
                                    if account.is_some() { "user" } else { "log-in" },
                                    14.0,
                                    if account.is_some() {
                                        p.text_secondary
                                    } else {
                                        p.accent
                                    },
                                )
                                .into_any_element(),
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
                let state = self.state.clone();
                return empty_state(
                    "circle-alert",
                    "Calcite couldn't open its data folder",
                    error,
                    Some(
                        Button::new("retry-backend")
                            .icon("refresh-cw")
                            .label("Try again")
                            .primary()
                            .on_click(move |_, _, cx| {
                                state.update(cx, |state, cx| state.retry_start(cx))
                            })
                            .into_any_element(),
                    ),
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
                let game = self.game_settings.clone();
                let storage = self.storage.clone();
                settings::render(self, section, move_in, game, storage, window, cx)
            }
            Route::User(_) => match &self.user_page {
                Some(page) => page.clone().into_any_element(),
                None => div().into_any_element(),
            },
            Route::Screenshots => match &self.screenshots {
                Some(page) => page.clone().into_any_element(),
                None => div().into_any_element(),
            },
            Route::Hosting => match &self.hosting {
                Some(page) => page.clone().into_any_element(),
                None => div().into_any_element(),
            },
            Route::Skins => match &self.skins {
                Some(page) => page.clone().into_any_element(),
                None => div().into_any_element(),
            },
            Route::Gallery => gallery::render(self, window, cx),
        }
    }

    fn notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = cx.palette().clone();
        let notice = self.state.read(cx).notice.clone()?;
        let accent = if notice.is_error { p.danger } else { p.accent };
        let key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            notice.title.hash(&mut h);
            notice.body.hash(&mut h);
            h.finish()
        };
        let toast = div()
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
            );
        Some(
            div()
                .absolute()
                .bottom(space(5.0))
                .right(space(5.0))
                .child(crate::ui::motion::rise(
                    toast,
                    SharedString::from(format!("notice-{key}")),
                    0,
                    12.0,
                ))
                .into_any_element(),
        )
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(id) = self.pending_change_version.take() {
            let dialog = cx.new(|cx| CreateInstanceDialog::change_version(id, window, cx));
            self.show_dialog(dialog, cx);
        }
        if let Some(id) = self.pending_switch_modpack.take() {
            let dialog =
                cx.new(|cx| crate::ui::version_picker::VersionPicker::modpack(id, window, cx));
            self.show_dialog(dialog, cx);
        }
        let p = cx.palette().clone();
        let page = self.page(window, cx);
        let notice = self.notice(cx);
        // During onboarding the title bar only holds the window controls.
        let title_bar = if self.onboarding_step.is_some() {
            TitleBar::new()
                .h(px(TITLE_BAR_HEIGHT))
                .bg(p.app)
                .border_color(p.app)
                .into_any_element()
        } else {
            self.title_bar(cx).into_any_element()
        };
        let dialog = self.dialog.clone();
        let drop_ring = p.accent;
        div()
            .id("shell-root")
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(p.app)
            // Files dropped anywhere on the window.
            .drag_over::<gpui_kit::ExternalPaths>(move |style, _, _, _| {
                style.border_2().border_color(drop_ring)
            })
            .on_drop(
                cx.listener(|this, paths: &gpui_kit::ExternalPaths, window, cx| {
                    this.handle_drop(paths.paths().to_vec(), window, cx)
                }),
            )
            .text_color(p.text)
            .font_family(theme::fonts::UI)
            .text_size(px(type_scale::BODY))
            .child(title_bar)
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
                            .child(crate::ui::motion::fade(
                                div().size_full().child(page),
                                SharedString::from(format!("page-{:?}", self.route)),
                                0,
                            ))
                            .children(notice),
                    ),
            )
            .when_some(self.palette.clone(), |d, palette| {
                let seq = self.overlay_seq;
                d.child(crate::ui::motion::fade(
                    div()
                        .id("palette-scrim")
                        .absolute()
                        .inset_0()
                        .bg(p.scrim)
                        .occlude()
                        .flex()
                        .justify_center()
                        .pt(px(96.0))
                        .child(crate::ui::motion::rise(
                            div()
                                .on_mouse_down_out(cx.listener(
                                    |this, _: &gpui_kit::MouseDownEvent, _, cx| {
                                        this.palette = None;
                                        cx.notify();
                                    },
                                ))
                                .child(palette),
                            ("palette-panel", seq),
                            0,
                            -8.0,
                        )),
                    ("palette-scrim-in", seq),
                    0,
                ))
            })
            .when_some(dialog, |d, dialog| {
                let seq = self.overlay_seq;
                d.child(crate::ui::motion::fade(
                    div()
                        .id("dialog-scrim")
                        .absolute()
                        .inset_0()
                        .bg(p.scrim)
                        .occlude()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(crate::ui::motion::rise(
                            div().child(dialog),
                            ("dialog-panel", seq),
                            0,
                            10.0,
                        )),
                    ("dialog-scrim-in", seq),
                    0,
                ))
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
