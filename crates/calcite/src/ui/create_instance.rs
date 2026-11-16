//! The "New instance" dialog: name, loader, game version, loader version.

use std::collections::HashMap;

use gpui_kit::component::IndexPath;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::select::{SearchableVec, Select, SelectEvent, SelectState};
use gpui_kit::prelude::*;
use gpui_kit::{
    ClickEvent, Context, Entity, EventEmitter, FontWeight, SharedString, Subscription, Window, div,
    px,
};
use theseus::data::ModLoader;

use crate::backend::Backend;
use crate::state::{AppState, loader_name};
use crate::theme::{PaletteExt, radius, space, type_scale};
use crate::ui::components::{Button, icon, skeleton};
use crate::versions::{LoaderCatalog, manifest_name};

const LOADERS: [ModLoader; 5] = [
    ModLoader::Vanilla,
    ModLoader::Fabric,
    ModLoader::Quilt,
    ModLoader::NeoForge,
    ModLoader::Forge,
];

pub enum DialogEvent {
    Close,
    /// Saved something that changes the open instance's worlds/servers.
    WorldsChanged,
    /// Swap this dialog for "Change version" on that instance.
    ChangeVersion(String),
    /// Swap this dialog for "Switch modpack version" on that instance.
    SwitchModpack(String),
    /// Changed the open instance's content; close and reload it.
    ContentChanged,
}

type VersionSelect = SelectState<SearchableVec<SharedString>>;

pub struct CreateInstanceDialog {
    name: Entity<InputState>,
    loader: ModLoader,
    include_snapshots: bool,
    /// Every Minecraft version, newest first: (id, is_release).
    game_versions: Option<Vec<(String, bool)>>,
    catalogs: HashMap<&'static str, LoaderCatalog>,
    load_error: Option<String>,
    game_select: Entity<VersionSelect>,
    loader_select: Entity<VersionSelect>,
    game_version: Option<String>,
    loader_version: Option<String>,
    /// Set when changing an existing instance's version instead of
    /// creating one.
    changing: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DialogEvent> for CreateInstanceDialog {}

impl CreateInstanceDialog {
    /// Change an instance's Minecraft version or loader. Starts on what it
    /// has now; applying reinstalls the game files (mods stay).
    pub fn change_version(
        instance_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut dialog = Self::new(window, cx);
        if let Some(inst) = AppState::global(cx)
            .read(cx)
            .instance(&instance_id)
            .cloned()
        {
            dialog.loader = inst.loader;
            dialog.game_version = Some(inst.game_version.to_string());
        }
        dialog.changing = Some(instance_id);
        dialog
    }

    fn apply_change(&mut self, cx: &mut Context<Self>) {
        let (Some(id), Some(game_version)) = (self.changing.clone(), self.game_version.clone())
        else {
            return;
        };
        let loader = self.loader;
        let loader_version = self
            .loader_version
            .clone()
            .map(|v| v.trim_end_matches(" (beta)").to_string());
        let patch = theseus::data::EditInstance {
            content_set_patch: Some(theseus::data::AppliedContentSetPatch {
                loader: Some(loader),
                game_version: Some(game_version),
                loader_version: Some(loader_version.filter(|_| loader != ModLoader::Vanilla)),
                ..Default::default()
            }),
            ..Default::default()
        };
        let task = Backend::global(cx).call(async move {
            theseus::instance::edit(&id, patch).await?;
            theseus::install::install_existing_instance(id, false).await
        });
        let state = AppState::global(cx);
        cx.spawn(async move |_, cx| {
            let result = task.await;
            state.update(cx, |state, cx| {
                match result {
                    Ok(job) => state.track_job(job, cx),
                    Err(error) => {
                        state.notify_error("Couldn't change the version", format!("{error:#}"))
                    }
                }
                state.refresh_all(cx);
            });
        })
        .detach();
        cx.emit(DialogEvent::Close);
    }
}

impl CreateInstanceDialog {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("Name"));
        let game_select = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(Vec::<SharedString>::new()),
                None,
                window,
                cx,
            )
            .searchable(true)
        });
        let loader_select = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(Vec::<SharedString>::new()),
                None,
                window,
                cx,
            )
        });

        let subscriptions = vec![
            cx.subscribe_in(&game_select, window, |this, _, event, window, cx| {
                let SelectEvent::Confirm(value) = event;
                this.game_version = value.as_ref().map(|v| v.to_string());
                this.sync_loader_versions(window, cx);
                cx.notify();
            }),
            cx.subscribe_in(&loader_select, window, |this, _, event, _, cx| {
                let SelectEvent::Confirm(value) = event;
                this.loader_version = value.as_ref().map(|v| v.to_string());
                cx.notify();
            }),
        ];

        let mut dialog = CreateInstanceDialog {
            name,
            loader: ModLoader::Fabric,
            include_snapshots: false,
            game_versions: None,
            catalogs: HashMap::new(),
            load_error: None,
            game_select,
            loader_select,
            game_version: None,
            loader_version: None,
            changing: None,
            _subscriptions: subscriptions,
        };
        dialog.load(window, cx);
        dialog
    }

    /// Fetch the game version list and every loader's catalog once.
    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let fetch = Backend::global(cx).call(async {
            let versions = theseus::metadata::get_minecraft_versions().await?;
            let mut catalogs = Vec::new();
            for loader in ["fabric", "quilt", "forge", "neo"] {
                match theseus::metadata::get_loader_versions(loader).await {
                    Ok(manifest) => {
                        catalogs.push((loader, LoaderCatalog::from_manifest(&manifest)))
                    }
                    Err(error) => tracing::warn!("{loader} versions unavailable: {error}"),
                }
            }
            let versions = versions
                .versions
                .iter()
                .map(|v| {
                    (
                        v.id.as_str().to_string(),
                        matches!(
                            v.ty,
                            minecraft_meta_manifest::minecraft::VersionType::Release
                        ),
                    )
                })
                .collect::<Vec<_>>();
            Ok((versions, catalogs))
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = fetch.await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok((versions, catalogs)) => {
                        this.game_versions = Some(versions);
                        this.catalogs = catalogs.into_iter().collect();
                        this.sync_game_versions(window, cx);
                    }
                    Err(error) => this.load_error = Some(format!("{error:#}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn available_game_versions(&self) -> Vec<String> {
        let Some(all) = &self.game_versions else {
            return Vec::new();
        };
        let catalog = manifest_name(self.loader).and_then(|n| self.catalogs.get(n));
        all.iter()
            .filter(|(_, release)| *release || self.include_snapshots)
            .filter(|(id, _)| catalog.is_none_or(|c| c.supports(id)))
            .map(|(id, _)| id.clone())
            .collect()
    }

    fn sync_game_versions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let versions = self.available_game_versions();
        let keep = self
            .game_version
            .as_ref()
            .filter(|v| versions.contains(v))
            .cloned();
        self.game_version = keep.or_else(|| versions.first().cloned());
        let index = self
            .game_version
            .as_ref()
            .and_then(|v| versions.iter().position(|x| x == v));
        let items: Vec<SharedString> = versions.into_iter().map(Into::into).collect();
        self.game_select.update(cx, |select, cx| {
            select.set_items(SearchableVec::new(items), window, cx);
            if let Some(ix) = index {
                select.set_selected_index(Some(IndexPath::new(ix)), window, cx);
            }
        });
        self.sync_loader_versions(window, cx);
    }

    fn sync_loader_versions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let catalog = manifest_name(self.loader).and_then(|n| self.catalogs.get(n));
        let (versions, default) = match (catalog, &self.game_version) {
            (Some(catalog), Some(gv)) => (
                catalog.loaders_for(gv),
                catalog.default_loader(gv).map(|l| l.id),
            ),
            _ => (Vec::new(), None),
        };
        self.loader_version = default.clone();
        let index = default
            .as_ref()
            .and_then(|d| versions.iter().position(|v| &v.id == d));
        let items: Vec<SharedString> = versions
            .iter()
            .map(|v| {
                if v.stable {
                    v.id.clone().into()
                } else {
                    format!("{} (beta)", v.id).into()
                }
            })
            .collect();
        self.loader_select.update(cx, |select, cx| {
            select.set_items(SearchableVec::new(items), window, cx);
            select.set_selected_index(index.map(IndexPath::new), window, cx);
        });
    }

    fn set_loader(&mut self, loader: ModLoader, window: &mut Window, cx: &mut Context<Self>) {
        self.loader = loader;
        self.sync_game_versions(window, cx);
        cx.notify();
    }

    fn default_name(&self) -> String {
        match &self.game_version {
            Some(gv) => format!("{} {}", loader_name(self.loader), gv),
            None => loader_name(self.loader).to_string(),
        }
    }

    fn can_create(&self) -> bool {
        self.game_version.is_some()
            && (self.loader == ModLoader::Vanilla || self.loader_version.is_some())
    }

    fn create(&mut self, cx: &mut Context<Self>) {
        let Some(game_version) = self.game_version.clone() else {
            return;
        };
        let typed = self.name.read(cx).value().trim().to_string();
        let name = if typed.is_empty() {
            self.default_name()
        } else {
            typed
        };
        let loader = self.loader;
        let loader_version = self
            .loader_version
            .clone()
            .map(|v| v.trim_end_matches(" (beta)").to_string());
        AppState::global(cx).update(cx, |state, cx| {
            state.create_instance(name, loader, game_version, loader_version, cx)
        });
        cx.emit(DialogEvent::Close);
    }
}

impl Render for CreateInstanceDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();
        let loaded = self.game_versions.is_some();

        let field = |label: &'static str, body: gpui_kit::AnyElement| {
            div()
                .flex()
                .flex_col()
                .gap(space(2.0))
                .child(
                    div()
                        .text_size(px(type_scale::SMALL))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(p.text_secondary)
                        .child(label),
                )
                .child(body)
        };

        let mut loaders = div()
            .flex()
            .gap(space(1.0))
            .p(space(1.0))
            .rounded(px(radius::MD))
            .bg(p.app);
        for loader in LOADERS {
            let active = loader == self.loader;
            loaders = loaders.child(
                div()
                    .id(SharedString::from(format!(
                        "loader-{}",
                        loader_name(loader)
                    )))
                    .flex_1()
                    .h(px(30.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(radius::SM))
                    .text_size(px(type_scale::BODY))
                    .font_weight(FontWeight::MEDIUM)
                    .cursor_pointer()
                    .when(active, |d| {
                        d.bg(p.raised)
                            .text_color(p.text)
                            .border_1()
                            .border_color(p.border_strong)
                    })
                    .when(!active, |d| {
                        d.text_color(p.text_secondary)
                            .hover(|s| s.text_color(p.text))
                    })
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.set_loader(loader, window, cx)
                    }))
                    .child(loader_name(loader)),
            );
        }

        let snapshots_toggle = div()
            .id("include-snapshots")
            .flex()
            .items_center()
            .gap(space(2.0))
            .cursor_pointer()
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                this.include_snapshots = !this.include_snapshots;
                this.sync_game_versions(window, cx);
                cx.notify();
            }))
            .child(
                div()
                    .size(px(16.0))
                    .rounded(px(4.0))
                    .border_1()
                    .border_color(if self.include_snapshots {
                        p.accent
                    } else {
                        p.border_strong
                    })
                    .bg(if self.include_snapshots {
                        p.accent
                    } else {
                        p.surface
                    })
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(self.include_snapshots, |d| {
                        d.child(icon("check", 12.0, p.on_accent))
                    }),
            )
            .child(
                div()
                    .text_size(px(type_scale::SMALL))
                    .text_color(p.text_secondary)
                    .child("Show snapshots"),
            );

        let versions: gpui_kit::AnyElement = if let Some(error) = &self.load_error {
            div()
                .text_size(px(type_scale::SMALL))
                .text_color(p.danger)
                .child(format!(
                    "Couldn't load version lists. Check your connection. ({error})"
                ))
                .into_any_element()
        } else if !loaded {
            div()
                .flex()
                .gap(space(3.0))
                .child(skeleton(gpui_kit::relative(0.5), px(32.0), cx))
                .child(skeleton(gpui_kit::relative(0.5), px(32.0), cx))
                .into_any_element()
        } else {
            div()
                .flex()
                .gap(space(3.0))
                .child(
                    div().flex_1().child(field(
                        "Minecraft version",
                        Select::new(&self.game_select)
                            .placeholder("Choose a version")
                            .into_any_element(),
                    )),
                )
                .when(self.loader != ModLoader::Vanilla, |d| {
                    d.child(
                        div().flex_1().child(field(
                            "Loader version",
                            Select::new(&self.loader_select)
                                .placeholder("No versions for this game version")
                                .into_any_element(),
                        )),
                    )
                })
                .into_any_element()
        };

        let can_create = self.can_create();
        div()
            .w(px(520.0))
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
                    .child(
                        div()
                            .text_size(px(type_scale::SECTION))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.text)
                            .child(if self.changing.is_some() { "Change version" } else { "New instance" }),
                    )
                    .child(
                        Button::new("dialog-close")
                            .icon("x")
                            .ghost()
                            .small()
                            .on_click(
                                cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(DialogEvent::Close)),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(space(5.0))
                    .px(space(6.0))
                    .py(space(5.0))
                    .when(self.changing.is_none(), |d| {
                        d.child(field("Name", Input::new(&self.name).into_any_element()))
                    })
                    .when(self.changing.is_some(), |d| {
                        d.child(
                            div()
                                .text_size(px(type_scale::SMALL))
                                .text_color(p.text_secondary)
                                .child("Calcite reinstalls the game for the new version. Your worlds and settings stay; mods that don't fit the new version won't load until you update them."),
                        )
                    })
                    .child(field("Mod loader", loaders.into_any_element()))
                    .child(versions)
                    .child(snapshots_toggle),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(space(2.0))
                    .px(space(6.0))
                    .py(space(4.0))
                    .border_t_1()
                    .border_color(p.border)
                    .child(Button::new("dialog-cancel").label("Cancel").on_click(
                        cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(DialogEvent::Close)),
                    ))
                    .child(
                        Button::new("dialog-create")
                            .icon(if self.changing.is_some() { "refresh-cw" } else { "plus" })
                            .label(if self.changing.is_some() { "Change version" } else { "Create" })
                            .primary()
                            .disabled(!can_create)
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                if this.changing.is_some() {
                                    this.apply_change(cx)
                                } else {
                                    this.create(cx)
                                }
                            })),
                    ),
            )
    }
}
