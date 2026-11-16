//! "Add to instance": pick which instance gets a mod, pack or shader. Each
//! instance says whether the project fits it, and why not when it doesn't.

use std::collections::HashSet;

use calcite_core::modrinth::{ProjectType, Version};
use gpui_kit::prelude::*;
use gpui_kit::{ClickEvent, Context, EventEmitter, FontWeight, SharedString, Window, div, px};

use crate::backend::Backend;
use crate::images::Images;
use crate::state::{AppState, InstanceInfo, loader_name};
use crate::theme::{PaletteExt, radius, space, type_scale};
use crate::ui::components::{Button, chip, icon, skeleton, thumb};
use crate::ui::create_instance::DialogEvent;
use crate::ui::pages::discover::Api;
use crate::ui::shell::instance_icon;

/// What the picker needs to know about a project.
#[derive(Clone, Debug)]
pub struct ProjectSummary {
    pub id: String,
    pub title: SharedString,
    pub icon_url: Option<String>,
    pub kind: ProjectType,
}

pub fn content_type(kind: ProjectType) -> theseus::data::ContentType {
    use theseus::data::ContentType;
    match kind {
        ProjectType::Mod => ContentType::Mod,
        ProjectType::Resourcepack => ContentType::ResourcePack,
        ProjectType::Shader => ContentType::Shader,
        ProjectType::Datapack => ContentType::DataPack,
        ProjectType::Plugin => ContentType::Plugin,
        ProjectType::Modpack => ContentType::ModPack,
    }
}

/// The loader name Modrinth uses for an instance's loader.
pub fn modrinth_loader(inst: &InstanceInfo) -> String {
    loader_name(inst.loader).to_ascii_lowercase()
}

/// Why a project doesn't fit an instance, or `None` if it does.
pub fn incompatibility(versions: &[Version], inst: &InstanceInfo) -> Option<String> {
    let loader = modrinth_loader(inst);
    if versions
        .iter()
        .any(|v| v.supports(&loader, &inst.game_version))
    {
        return None;
    }
    let runs_on_loader = versions.iter().any(|v| {
        v.loaders.contains(&loader)
            || v.loaders.iter().all(|l| {
                matches!(
                    l.as_str(),
                    "minecraft" | "iris" | "optifine" | "canvas" | "datapack"
                )
            })
    });
    Some(if runs_on_loader {
        format!("No version for {}", inst.game_version)
    } else {
        format!("Doesn't run on {}", loader_name(inst.loader))
    })
}

pub struct InstancePicker {
    project: ProjectSummary,
    versions: Option<Result<Vec<Version>, String>>,
    installed: HashSet<String>,
}

impl EventEmitter<DialogEvent> for InstancePicker {}

impl InstancePicker {
    pub fn new(project: ProjectSummary, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let client = cx.global::<Api>().0.clone();
        let id = project.id.clone();
        let instance_ids: Vec<String> = AppState::global(cx)
            .read(cx)
            .instances
            .iter()
            .map(|i| i.id.clone())
            .collect();
        let backend = Backend::global(cx).clone();
        let fetch = backend.run(async move {
            let versions = client.versions(&id).await.map_err(|e| format!("{e:#}"));
            let mut installed = HashSet::new();
            for instance in instance_ids {
                if let Ok(ids) = theseus::instance::get_installed_project_ids(&instance).await
                    && ids.contains(&id)
                {
                    installed.insert(instance);
                }
            }
            (versions, installed)
        });
        cx.observe(&AppState::global(cx), |_, _, cx| cx.notify())
            .detach();
        cx.spawn(async move |this, cx| {
            if let Ok((versions, installed)) = fetch.await {
                let _ = this.update(cx, |this, cx| {
                    this.versions = Some(versions);
                    this.installed = installed;
                    cx.notify();
                });
            }
        })
        .detach();
        InstancePicker {
            project,
            versions: None,
            installed: HashSet::new(),
        }
    }
}

impl Render for InstancePicker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();
        let state = AppState::global(cx);
        let instances: Vec<InstanceInfo> = state.read(cx).instances.clone();
        let source = self
            .project
            .icon_url
            .as_deref()
            .and_then(|u| Images::get(u, 96, cx));

        let mut list = div()
            .id("picker-list")
            .flex()
            .flex_col()
            .max_h(px(420.0))
            .overflow_y_scroll();

        match &self.versions {
            None => {
                for _ in 0..4 {
                    list = list.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(space(3.0))
                            .h(px(52.0))
                            .px(space(6.0))
                            .child(skeleton(px(32.0), px(32.0), cx))
                            .child(skeleton(px(200.0), px(14.0), cx)),
                    );
                }
            }
            Some(Err(error)) => {
                list = list.child(
                    div()
                        .px(space(6.0))
                        .py(space(4.0))
                        .text_size(px(type_scale::SMALL))
                        .text_color(p.danger)
                        .child(format!("Couldn't load versions: {error}")),
                );
            }
            Some(Ok(versions)) => {
                if instances.is_empty() {
                    list = list.child(
                        div()
                            .px(space(6.0))
                            .py(space(4.0))
                            .text_size(px(type_scale::BODY))
                            .text_color(p.text_secondary)
                            .child("You don't have any instances yet. Create one from the Library first."),
                    );
                }
                let mut rows: Vec<(InstanceInfo, Option<String>)> = instances
                    .into_iter()
                    .map(|i| {
                        let why = incompatibility(versions, &i);
                        (i, why)
                    })
                    .collect();
                rows.sort_by_key(|(_, why)| why.is_some());
                for (ix, (inst, why)) in rows.into_iter().enumerate() {
                    let installed = self.installed.contains(&inst.id);
                    let busy = state.read(cx).is_installing(&inst.id, &self.project.id);
                    let thumb_src = instance_icon(&inst);
                    let (instance_id, project_id, kind) =
                        (inst.id.clone(), self.project.id.clone(), self.project.kind);
                    let status: gpui_kit::AnyElement = if installed {
                        div()
                            .flex()
                            .items_center()
                            .gap(space(1.5))
                            .text_size(px(type_scale::SMALL))
                            .text_color(p.running)
                            .child(icon("check", 14.0, p.running))
                            .child("Installed")
                            .into_any_element()
                    } else if let Some(reason) = &why {
                        div()
                            .text_size(px(type_scale::SMALL))
                            .text_color(p.text_muted)
                            .child(reason.clone())
                            .into_any_element()
                    } else {
                        Button::new(SharedString::from(format!("add-to-{ix}")))
                            .icon(if busy { "loader-circle" } else { "plus" })
                            .label(if busy { "Adding…" } else { "Add" })
                            .small()
                            .primary()
                            .disabled(busy)
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                let (instance_id, project_id) =
                                    (instance_id.clone(), project_id.clone());
                                this.installed.insert(instance_id.clone());
                                AppState::global(cx).update(cx, |state, cx| {
                                    state.install_content(
                                        instance_id,
                                        project_id,
                                        None,
                                        content_type(kind),
                                        cx,
                                    )
                                });
                                cx.notify();
                            }))
                            .into_any_element()
                    };
                    list = list.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(space(3.0))
                            .h(px(52.0))
                            .px(space(6.0))
                            .when(why.is_some() && !installed, |d| d.opacity(0.6))
                            .child(thumb(thumb_src, &inst.name, 32.0, radius::SM, cx))
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
                                            .child(inst.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(type_scale::CAPTION))
                                            .text_color(p.text_muted)
                                            .child(inst.loader_line()),
                                    ),
                            )
                            .child(status),
                    );
                }
            }
        }

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
                    .gap(space(3.0))
                    .px(space(6.0))
                    .pt(space(5.0))
                    .pb(space(3.0))
                    .child(thumb(source, &self.project.title, 36.0, radius::MD, cx))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(type_scale::SECTION))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(p.text)
                                    .child(format!("Add {}", self.project.title)),
                            )
                            .child(
                                div()
                                    .text_size(px(type_scale::SMALL))
                                    .text_color(p.text_muted)
                                    .child(
                                        "Calcite picks the right version and adds what it needs.",
                                    ),
                            ),
                    )
                    .child(
                        Button::new("picker-close")
                            .icon("x")
                            .ghost()
                            .small()
                            .on_click(
                                cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(DialogEvent::Close)),
                            ),
                    ),
            )
            .child(div().h(px(1.0)).bg(p.border))
            .child(list)
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(space(2.0))
                    .px(space(6.0))
                    .py(space(4.0))
                    .border_t_1()
                    .border_color(p.border)
                    .child({
                        let n = AppState::global(cx).read(cx).instances.len();
                        chip(format!("{n} instance{}", if n == 1 { "" } else { "s" }), cx)
                    })
                    .child(div().flex_1())
                    .child(Button::new("picker-done").label("Done").on_click(
                        cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(DialogEvent::Close)),
                    )),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(loaders: &[&str], games: &[&str]) -> Version {
        serde_json::from_value(serde_json::json!({
            "id": "v", "project_id": "p", "name": "n", "version_number": "1",
            "date_published": "2026-01-01T00:00:00Z", "downloads": 0,
            "version_type": "release", "game_versions": games, "loaders": loaders, "files": []
        }))
        .unwrap()
    }

    fn instance(loader: theseus::data::ModLoader, game: &str) -> InstanceInfo {
        InstanceInfo {
            id: "i".into(),
            name: "Test".into(),
            icon_path: None,
            loader,
            game_version: game.to_string().into(),
            loader_version: None,
            last_played: None,
            created: chrono::Utc::now(),
            played_secs: 0,
            installed: true,
            group_ids: vec![],
            is_modpack: false,
            shared: None,
        }
    }

    #[test]
    fn explains_why_a_project_does_not_fit() {
        use theseus::data::ModLoader;
        let versions = vec![version(&["fabric"], &["1.21.4"])];
        assert_eq!(
            incompatibility(&versions, &instance(ModLoader::Fabric, "1.21.4")),
            None
        );
        assert_eq!(
            incompatibility(&versions, &instance(ModLoader::Fabric, "1.20.1")).as_deref(),
            Some("No version for 1.20.1")
        );
        assert_eq!(
            incompatibility(&versions, &instance(ModLoader::Forge, "1.21.4")).as_deref(),
            Some("Doesn't run on Forge")
        );
        // Resource packs fit any loader.
        let pack = vec![version(&["minecraft"], &["1.21.4"])];
        assert_eq!(
            incompatibility(&pack, &instance(ModLoader::Vanilla, "1.21.4")),
            None
        );
    }
}
