//! Skins: what your Minecraft character wears, with a 3D preview you can turn.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use calcite_core::skin3d::{self, Model, View};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Entity, FontWeight, ImageSource, MouseButton,
    MouseDownEvent, MouseMoveEvent, ObjectFit, PathPromptOptions, Pixels, Point, RenderImage,
    SharedString, Window, div, img, px,
};
use image::RgbaImage;
use theseus::minecraft_skins::{Bytes, Cape, MinecraftSkinVariant, Skin, SkinSource, UrlOrBlob};
use uuid::Uuid;

use crate::backend::Backend;
use crate::images::render_image;
use crate::state::AppState;
use crate::theme::{PaletteExt, radius, space, type_scale};
use crate::ui::components::{Button, empty_state, icon, panel, section_heading, skeleton};
use crate::ui::pages::discover::Api;

/// Preview and card sizes in logical pixels.
const PREVIEW: (f32, f32) = (260.0, 380.0);
const CARD: (f32, f32) = (84.0, 126.0);
/// Cape pictures keep the cape's 10:16 shape.
const CAPE_TILE: (f32, f32) = (30.0, 48.0);
/// The view cards are drawn from.
const CARD_VIEW: View = View {
    yaw: -0.4,
    pitch: 0.12,
};
/// Sections open from the start; the bigger packs open on request.
const OPEN_BY_DEFAULT: [&str; 2] = ["", "Default skins"];

/// A skin as the renderer needs it.
type SkinId = (Arc<str>, bool);

fn id_of(skin: &Skin) -> SkinId {
    (skin.texture_key.clone(), is_slim(skin))
}

fn is_slim(skin: &Skin) -> bool {
    matches!(skin.variant, MinecraftSkinVariant::Slim)
}

fn model(slim: bool) -> Model {
    if slim { Model::Slim } else { Model::Classic }
}

fn variant(slim: bool) -> MinecraftSkinVariant {
    if slim {
        MinecraftSkinVariant::Slim
    } else {
        MinecraftSkinVariant::Classic
    }
}

pub struct SkinsPage {
    state: Entity<AppState>,
    skins: Option<Result<Vec<Skin>, String>>,
    capes: Vec<Cape>,
    /// Normalized skin textures by texture key.
    textures: HashMap<Arc<str>, Arc<RgbaImage>>,
    cape_textures: HashMap<Uuid, Arc<RgbaImage>>,
    cape_thumbs: HashMap<Uuid, Arc<RenderImage>>,
    thumbs: HashMap<SkinId, Arc<RenderImage>>,
    loading_textures: HashSet<Arc<str>>,
    open_sections: HashSet<String>,
    view: View,
    preview: Option<Arc<RenderImage>>,
    /// What the preview shows now, and whether another render is queued.
    preview_of: Option<(SkinId, Option<Uuid>, View)>,
    rendering: bool,
    last_drag: Option<Point<Pixels>>,
    scale: f32,
    busy: bool,
}

impl SkinsPage {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut page = SkinsPage {
            state: AppState::global(cx),
            skins: None,
            capes: Vec::new(),
            textures: HashMap::new(),
            cape_textures: HashMap::new(),
            cape_thumbs: HashMap::new(),
            thumbs: HashMap::new(),
            loading_textures: HashSet::new(),
            open_sections: OPEN_BY_DEFAULT.iter().map(|s| s.to_string()).collect(),
            view: View::default(),
            preview: None,
            preview_of: None,
            rendering: false,
            last_drag: None,
            scale: 2.0,
            busy: false,
        };
        page.load(cx);
        page
    }

    fn signed_in(&self, cx: &App) -> bool {
        !self.state.read(cx).accounts.is_empty()
    }

    fn equipped(&self) -> Option<&Skin> {
        self.skins
            .as_ref()?
            .as_ref()
            .ok()?
            .iter()
            .find(|s| s.is_equipped)
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let task = Backend::global(cx).call(async {
            let skins = theseus::minecraft_skins::get_available_skins().await?;
            // Capes need a signed-in account; no capes is fine.
            let capes = theseus::minecraft_skins::get_available_capes()
                .await
                .unwrap_or_default();
            Ok((skins, capes))
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok((skins, capes)) => {
                        this.skins = Some(Ok(skins));
                        this.capes = capes;
                    }
                    Err(error) => this.skins = Some(Err(format!("{error:#}"))),
                }
                this.fetch_textures(cx);
                this.fetch_capes(cx);
                // What's equipped may have changed (skin, cape or arms).
                this.draw_thumbs(cx);
                this.draw_preview(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Fetch and decode the textures of the skins that are showing.
    fn fetch_textures(&mut self, cx: &mut Context<Self>) {
        let Some(Ok(skins)) = &self.skins else { return };
        let wanted: Vec<(Arc<str>, Arc<url::Url>)> = skins
            .iter()
            .filter(|s| s.is_equipped || self.open_sections.contains(section_of(s)))
            .filter(|s| !self.textures.contains_key(&s.texture_key))
            .filter(|s| !self.loading_textures.contains(&s.texture_key))
            .map(|s| (s.texture_key.clone(), s.texture.clone()))
            .collect::<HashMap<_, _>>()
            .into_iter()
            .collect();
        if wanted.is_empty() {
            return;
        }
        for (key, _) in &wanted {
            self.loading_textures.insert(key.clone());
        }
        let task = Backend::global(cx).run(async move {
            let fetches = wanted.into_iter().map(|(key, url)| async move {
                let bytes = theseus::minecraft_skins::normalize_skin_texture(&UrlOrBlob::Url(
                    (*url).clone(),
                ))
                .await
                .ok()?;
                let decoded = image::load_from_memory(&bytes).ok()?.to_rgba8();
                Some((key, Arc::new(skin3d::normalize(&decoded)?)))
            });
            futures::future::join_all(fetches)
                .await
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let Ok(found) = task.await else { return };
            let _ = this.update(cx, |this, cx| {
                for (key, texture) in found {
                    this.loading_textures.remove(&key);
                    this.textures.insert(key, texture);
                }
                this.draw_thumbs(cx);
                this.draw_preview(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn fetch_capes(&mut self, cx: &mut Context<Self>) {
        let wanted: Vec<(Uuid, String)> = self
            .capes
            .iter()
            .filter(|c| !self.cape_textures.contains_key(&c.id))
            .map(|c| (c.id, c.texture.to_string()))
            .collect();
        if wanted.is_empty() {
            return;
        }
        let client = cx.global::<Api>().0.clone();
        let task = Backend::global(cx).run(async move {
            let mut out = Vec::new();
            for (id, url) in wanted {
                if let Ok(bytes) = client.bytes(&url).await
                    && let Ok(decoded) = image::load_from_memory(&bytes)
                {
                    out.push((id, Arc::new(decoded.to_rgba8())));
                }
            }
            out
        });
        cx.spawn(async move |this, cx| {
            let Ok(found) = task.await else { return };
            let _ = this.update(cx, |this, cx| {
                for (id, texture) in &found {
                    this.cape_thumbs.insert(*id, cape_picture(texture));
                }
                this.cape_textures.extend(found);
                this.draw_preview(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Draw card thumbnails for loaded textures that don't have one yet.
    fn draw_thumbs(&mut self, cx: &mut Context<Self>) {
        let Some(Ok(skins)) = &self.skins else { return };
        let scale = self.scale;
        let jobs: Vec<(SkinId, Arc<RgbaImage>)> = skins
            .iter()
            .filter(|s| self.open_sections.contains(section_of(s)))
            .map(id_of)
            .collect::<HashSet<_>>()
            .into_iter()
            .filter(|id| !self.thumbs.contains_key(id))
            .filter_map(|id| Some((id.clone(), self.textures.get(&id.0)?.clone())))
            .collect();
        if jobs.is_empty() {
            return;
        }
        let task = cx.background_spawn(async move {
            let (w, h) = ((CARD.0 * scale) as u32, (CARD.1 * scale) as u32);
            jobs.into_iter()
                .map(|(id, texture)| {
                    let image =
                        skin3d::render_with(&texture, None, model(id.1), CARD_VIEW, w, h, 1);
                    (id, render_image(image))
                })
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let thumbs = task.await;
            let _ = this.update(cx, |this, cx| {
                this.thumbs.extend(thumbs);
                cx.notify();
            });
        })
        .detach();
    }

    /// Re-draw the big preview if what it should show has changed. Only one
    /// render runs at a time; the latest request wins.
    fn draw_preview(&mut self, cx: &mut Context<Self>) {
        let Some(skin) = self.equipped().cloned() else {
            return;
        };
        let id = id_of(&skin);
        let want = (id.clone(), skin.cape_id, self.view);
        if self.rendering || self.preview_of.as_ref() == Some(&want) {
            return;
        }
        let Some(texture) = self.textures.get(&id.0).cloned() else {
            return;
        };
        let cape = skin
            .cape_id
            .and_then(|c| self.cape_textures.get(&c).cloned());
        self.rendering = true;
        let scale = self.scale;
        let view = self.view;
        let task = cx.background_spawn(async move {
            let (w, h) = ((PREVIEW.0 * scale) as u32, (PREVIEW.1 * scale) as u32);
            let ss = if scale >= 2.0 { 1 } else { 2 };
            render_image(skin3d::render_with(
                &texture,
                cape.as_deref(),
                model(id.1),
                view,
                w,
                h,
                ss,
            ))
        });
        cx.spawn(async move |this, cx| {
            let image = task.await;
            let _ = this.update(cx, |this, cx| {
                this.preview = Some(image);
                this.preview_of = Some(want);
                this.rendering = false;
                // Catch up with anything that changed meanwhile.
                this.draw_preview(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Dev hook for snapshots: look at the back (capes).
    #[cfg_attr(not(feature = "snapshot"), allow(dead_code))]
    pub(crate) fn look_from_behind(&mut self, cx: &mut Context<Self>) {
        self.view = View {
            yaw: std::f32::consts::PI - 0.5,
            pitch: 0.15,
        };
        self.draw_preview(cx);
    }

    fn turn(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if let Some(last) = self.last_drag {
            let dx = f32::from(position.x - last.x);
            let dy = f32::from(position.y - last.y);
            self.view.yaw += dx * 0.012;
            self.view.pitch = (self.view.pitch + dy * 0.008).clamp(-0.7, 0.9);
            self.draw_preview(cx);
        }
        self.last_drag = Some(position);
    }

    fn tell_error(
        &self,
        title: &'static str,
        error: impl std::fmt::Display,
        cx: &mut Context<Self>,
    ) {
        let error = error.to_string();
        self.state.update(cx, |s, cx| {
            s.notify_error(title, error);
            cx.notify();
        });
    }

    /// Run a skin change, then reload the list.
    fn change(
        &mut self,
        task: impl Future<Output = anyhow::Result<()>> + 'static,
        failure: &'static str,
        cx: &mut Context<Self>,
    ) {
        self.busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                if let Err(error) = result {
                    this.tell_error(failure, format!("{error:#}"), cx);
                } else {
                    // The avatar shows the skin's face: refresh it.
                    AppState::global(cx).update(cx, |s, cx| {
                        s.face = None;
                        s.load_face(cx);
                    });
                }
                this.load(cx);
            });
        })
        .detach();
    }

    fn equip(&mut self, skin: Skin, cx: &mut Context<Self>) {
        // Show it right away; the list reload confirms it.
        if let Some(Ok(skins)) = &mut self.skins {
            for s in skins.iter_mut() {
                s.is_equipped = s.texture_key == skin.texture_key && s.variant == skin.variant;
            }
        }
        self.draw_preview(cx);
        let task = Backend::global(cx)
            .call(async move { theseus::minecraft_skins::equip_skin(skin).await });
        self.change(task, "Couldn't change your skin", cx);
    }

    /// Wear the current skin with another cape (or none), or other arms.
    fn restyle(&mut self, cape: Option<Option<Uuid>>, slim: Option<bool>, cx: &mut Context<Self>) {
        let Some(skin) = self.equipped().cloned() else {
            return;
        };
        let cape_id = cape.unwrap_or(skin.cape_id);
        let cape = cape_id.and_then(|id| self.capes.iter().find(|c| c.id == id).cloned());
        let slim = slim.unwrap_or(is_slim(&skin));
        // Show the change right away; the reload afterwards confirms it.
        if let Some(Ok(skins)) = &mut self.skins
            && let Some(worn) = skins.iter_mut().find(|s| s.is_equipped)
        {
            worn.cape_id = cape_id;
            worn.variant = variant(slim);
        }
        self.draw_preview(cx);
        let task = Backend::global(cx).call(async move {
            let texture: Bytes = theseus::minecraft_skins::normalize_skin_texture(&UrlOrBlob::Url(
                (*skin.texture).clone(),
            ))
            .await?;
            if is_custom(&skin) {
                let saved = theseus::minecraft_skins::save_custom_skin(
                    skin,
                    texture,
                    variant(slim),
                    cape,
                    false,
                )
                .await?;
                theseus::minecraft_skins::equip_skin(saved).await
            } else {
                theseus::minecraft_skins::add_and_equip_custom_skin(texture, variant(slim), cape)
                    .await
                    .map(|_| ())
            }
        });
        self.change(task, "Couldn't change your skin", cx);
    }

    fn add_skin(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Add skin".into()),
        });
        let backend = Backend::global(cx).clone();
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let checked = (|| -> anyhow::Result<(Vec<u8>, bool)> {
                let bytes = std::fs::read(&path)?;
                let decoded = image::load_from_memory(&bytes)
                    .map_err(|_| anyhow::anyhow!("That file isn't a PNG image."))?
                    .to_rgba8();
                let normal = skin3d::normalize(&decoded).ok_or_else(|| {
                    anyhow::anyhow!(
                        "Skins are 64×64 or 64×32 pixels; this one is {}×{}.",
                        decoded.width(),
                        decoded.height()
                    )
                })?;
                Ok((bytes, skin3d::looks_slim(&normal)))
            })();
            let _ = this.update(cx, |this, cx| match checked {
                Ok((bytes, slim)) => {
                    let task = backend.call(async move {
                        theseus::minecraft_skins::add_and_equip_custom_skin(
                            Bytes::from(bytes),
                            variant(slim),
                            None,
                        )
                        .await
                        .map(|_| ())
                    });
                    this.change(task, "Couldn't add that skin", cx);
                }
                Err(error) => this.tell_error("Couldn't add that skin", error, cx),
            });
        })
        .detach();
    }

    fn remove(&mut self, skin: Skin, cx: &mut Context<Self>) {
        let task = Backend::global(cx)
            .call(async move { theseus::minecraft_skins::remove_custom_skin(skin).await });
        self.change(task, "Couldn't remove that skin", cx);
    }

    fn open_section(&mut self, name: String, cx: &mut Context<Self>) {
        self.open_sections.insert(name);
        self.fetch_textures(cx);
        self.draw_thumbs(cx);
        cx.notify();
    }

    fn preview_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette().clone();
        let equipped = self.equipped().cloned();
        let picture: AnyElement = match &self.preview {
            Some(image) => img(ImageSource::Render(image.clone()))
                .w(px(PREVIEW.0))
                .h(px(PREVIEW.1))
                .object_fit(ObjectFit::Contain)
                .into_any_element(),
            None => div()
                .w(px(PREVIEW.0))
                .h(px(PREVIEW.1))
                .flex()
                .items_center()
                .justify_center()
                .child(skeleton(px(120.0), px(300.0), cx))
                .into_any_element(),
        };
        let stage = div()
            .id("skin-preview")
            .flex()
            .flex_col()
            .items_center()
            .cursor_grab()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, _, cx| {
                    this.last_drag = Some(e.position);
                    if e.click_count == 2 {
                        this.view = View::default();
                        this.draw_preview(cx);
                    }
                }),
            )
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                if e.pressed_button == Some(MouseButton::Left) {
                    this.turn(e.position, cx);
                } else {
                    this.last_drag = None;
                }
            }))
            .child(picture)
            .child(
                div()
                    .pb(space(3.0))
                    .text_size(px(type_scale::CAPTION))
                    .text_color(p.text_muted)
                    .child("Drag to turn · double-click to reset"),
            );

        let name: SharedString = equipped
            .as_ref()
            .map(|s| match (&s.name, &s.source) {
                (Some(n), _) => n.to_string(),
                (None, SkinSource::Custom) => "Your skin".to_string(),
                (None, _) => "Current skin".to_string(),
            })
            .unwrap_or_default()
            .into();
        let can_change = self.signed_in(cx) && !self.busy && equipped.is_some();
        let slim = equipped.as_ref().is_some_and(is_slim);
        let cape_now = equipped.as_ref().and_then(|s| s.cape_id);

        let option = |id: SharedString, label: SharedString, active: bool| {
            div()
                .id(id)
                .h(px(28.0))
                .px(space(2.5))
                .flex()
                .items_center()
                .rounded(px(radius::SM))
                .border_1()
                .text_size(px(type_scale::SMALL))
                .font_weight(FontWeight::MEDIUM)
                .when(active, |d| {
                    d.border_color(p.accent)
                        .bg(p.accent_soft)
                        .text_color(p.text)
                })
                .when(!active, |d| {
                    d.border_color(p.border)
                        .text_color(p.text_secondary)
                        .hover(|s| s.bg(p.hover))
                })
                .when(can_change && !active, |d| d.cursor_pointer())
                .child(label)
        };
        let arms =
            div()
                .flex()
                .gap(space(1.5))
                .child(
                    option("arms-classic".into(), "Classic arms".into(), !slim).when(
                        can_change && slim,
                        |d| {
                            d.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.restyle(None, Some(false), cx)
                            }))
                        },
                    ),
                )
                .child(option("arms-slim".into(), "Slim arms".into(), slim).when(
                    can_change && !slim,
                    |d| {
                        d.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.restyle(None, Some(true), cx)
                        }))
                    },
                ));
        // Capes as pictures of their outer side, named underneath.
        let tile = |id: SharedString, name: SharedString, picture: AnyElement, active: bool| {
            div()
                .id(id)
                .flex()
                .flex_col()
                .items_center()
                .gap(space(1.0))
                .w(px(66.0))
                .pt(space(2.0))
                .pb(space(1.5))
                .rounded(px(radius::MD))
                .border_1()
                .when(active, |d| d.border_color(p.accent).bg(p.accent_soft))
                .when(!active, |d| {
                    d.border_color(p.border).hover(|s| s.bg(p.hover))
                })
                .when(can_change && !active, |d| d.cursor_pointer())
                .child(picture)
                .child(
                    div()
                        .w_full()
                        .px(space(1.0))
                        .truncate()
                        .text_center()
                        .text_size(px(type_scale::CAPTION))
                        .text_color(if active { p.text } else { p.text_secondary })
                        .child(name),
                )
        };
        let empty_picture = || {
            div()
                .w(px(CAPE_TILE.0))
                .h(px(CAPE_TILE.1))
                .rounded(px(2.0))
                .bg(p.raised)
                .flex()
                .items_center()
                .justify_center()
        };
        let mut capes = div().flex().flex_wrap().gap(space(1.5)).child(
            tile(
                "cape-none".into(),
                "No cape".into(),
                empty_picture()
                    .child(icon("ban", 14.0, p.text_muted))
                    .into_any_element(),
                cape_now.is_none(),
            )
            .when(can_change && cape_now.is_some(), |d| {
                d.on_click(
                    cx.listener(|this, _: &ClickEvent, _, cx| this.restyle(Some(None), None, cx)),
                )
            }),
        );
        for cape in &self.capes {
            let id = cape.id;
            let active = cape_now == Some(id);
            let picture = match self.cape_thumbs.get(&id) {
                Some(image) => img(ImageSource::Render(image.clone()))
                    .w(px(CAPE_TILE.0))
                    .h(px(CAPE_TILE.1))
                    .into_any_element(),
                None => empty_picture().into_any_element(),
            };
            capes = capes.child(
                tile(
                    SharedString::from(format!("cape-{id}")),
                    cape.name.to_string().into(),
                    picture,
                    active,
                )
                .when(can_change && !active, |d| {
                    d.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.restyle(Some(Some(id)), None, cx)
                    }))
                }),
            );
        }

        div()
            .flex()
            .flex_col()
            .gap(space(4.0))
            .w(px(PREVIEW.0 + 32.0))
            .flex_none()
            .child(panel(cx).overflow_hidden().child(stage))
            .child(
                div()
                    .text_size(px(type_scale::SECTION))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(p.text)
                    .child(name),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(space(2.0))
                    .child(section_heading("Model", cx))
                    .child(arms),
            )
            .when(!self.capes.is_empty(), |d| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(space(2.0))
                        .child(section_heading("Cape", cx))
                        .child(capes),
                )
            })
            .into_any_element()
    }

    fn card(&self, skin: &Skin, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette().clone();
        let thumb = self.thumbs.get(&id_of(skin)).cloned();
        let equipped = skin.is_equipped;
        let can_change = self.signed_in(cx) && !self.busy;
        let base = skin
            .name
            .as_ref()
            .map(|n| n.to_string())
            .unwrap_or_else(|| {
                if is_custom(skin) {
                    // Number saved skins so they can be told apart.
                    let number = self
                        .skins
                        .as_ref()
                        .and_then(|r| r.as_ref().ok())
                        .map(|all| {
                            all.iter()
                                .filter(|s| is_custom(s) && s.name.is_none())
                                .position(|s| id_of(s) == id_of(skin))
                                .map_or(1, |n| n + 1)
                        })
                        .unwrap_or(1);
                    format!("Saved skin {number}")
                } else {
                    "Skin".into()
                }
            });
        // Default skins come in both arm widths under one name.
        let label: SharedString = if is_slim(skin) && !is_custom(skin) {
            format!("{base} · slim").into()
        } else {
            base.into()
        };
        let wear = skin.clone();
        let removable = is_custom(skin) && !equipped && can_change;
        let gone = skin.clone();
        div()
            .id(SharedString::from(format!(
                "skin-{}-{ix}",
                skin.texture_key
            )))
            .relative()
            .flex()
            .flex_col()
            .items_center()
            .gap(space(1.0))
            .w(px(CARD.0 + 16.0))
            .pt(space(2.0))
            .pb(space(2.0))
            .rounded(px(radius::LG))
            .border_1()
            .when(equipped, |d| d.border_color(p.accent).bg(p.accent_soft))
            .when(!equipped, |d| {
                d.border_color(p.border)
                    .bg(p.surface)
                    .hover(|s| s.bg(p.hover))
            })
            .when(can_change && !equipped, |d| {
                d.cursor_pointer().on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| this.equip(wear.clone(), cx)),
                )
            })
            .child(match thumb {
                Some(image) => img(ImageSource::Render(image))
                    .w(px(CARD.0))
                    .h(px(CARD.1))
                    .into_any_element(),
                None => div()
                    .w(px(CARD.0))
                    .h(px(CARD.1))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(skeleton(px(36.0), px(96.0), cx))
                    .into_any_element(),
            })
            .child(
                div()
                    .w_full()
                    .px(space(1.5))
                    .text_center()
                    .truncate()
                    .text_size(px(type_scale::CAPTION))
                    .text_color(if equipped { p.text } else { p.text_secondary })
                    .child(label),
            )
            .when(removable, |d| {
                d.child(
                    div()
                        .id(SharedString::from(format!("skin-remove-{ix}")))
                        .absolute()
                        .top(px(4.0))
                        .right(px(4.0))
                        .size(px(20.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(radius::SM))
                        .cursor_pointer()
                        .hover(|s| s.bg(p.raised))
                        .child(icon("x", 12.0, p.text_muted))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            cx.stop_propagation();
                            this.remove(gone.clone(), cx)
                        })),
                )
            })
            .into_any_element()
    }

    fn gallery(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette().clone();
        let Some(Ok(skins)) = &self.skins else {
            return div()
                .flex()
                .flex_wrap()
                .gap(space(2.0))
                .children((0..12).map(|_| skeleton(px(CARD.0 + 16.0), px(CARD.1 + 36.0), cx)))
                .into_any_element();
        };
        // Sections in the order the backend lists them; your own first.
        let mut order: Vec<&str> = vec![""];
        for skin in skins {
            let s = section_of(skin);
            if !order.contains(&s) {
                order.push(s);
            }
        }
        let mut out = div().flex().flex_col().gap(space(6.0));
        for section in order {
            let in_section: Vec<(usize, &Skin)> = skins
                .iter()
                .enumerate()
                .filter(|(_, s)| section_of(s) == section)
                .collect();
            let title = if section.is_empty() {
                "Your skins"
            } else {
                section
            };
            let open = self.open_sections.contains(section);
            let mut grid = div().flex().flex_wrap().gap(space(2.0));
            if section.is_empty() {
                grid = grid.child(
                    div()
                        .id("skin-add")
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap(space(2.0))
                        .w(px(CARD.0 + 16.0))
                        .h(px(CARD.1 + 36.0))
                        .rounded(px(radius::LG))
                        .border_1()
                        .border_dashed()
                        .border_color(p.border_strong)
                        .text_size(px(type_scale::SMALL))
                        .text_color(p.text_secondary)
                        .when(self.signed_in(cx) && !self.busy, |d| {
                            d.cursor_pointer().hover(|s| s.bg(p.hover)).on_click(
                                cx.listener(|this, _: &ClickEvent, _, cx| this.add_skin(cx)),
                            )
                        })
                        .child(icon("plus", 20.0, p.text_muted))
                        .child("Add skin"),
                );
            }
            if open {
                for (ix, skin) in &in_section {
                    grid = grid.child(self.card(skin, *ix, cx));
                }
            }
            let name = section.to_string();
            out = out.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(space(3.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(space(2.0))
                            .child(section_heading(title, cx))
                            .when(!open, |d| {
                                d.child(
                                    Button::new(SharedString::from(format!("open-section-{name}")))
                                        .label(format!("Show {}", in_section.len()))
                                        .small()
                                        .ghost()
                                        .on_click(cx.listener(
                                            move |this, _: &ClickEvent, _, cx| {
                                                this.open_section(name.clone(), cx)
                                            },
                                        )),
                                )
                            }),
                    )
                    .when(open || section.is_empty(), |d| d.child(grid)),
            );
        }
        out.into_any_element()
    }
}

/// A cape's outer side (texels 1,1 to 11,17 of a 64×32 texture, scaled for
/// HD capes), enlarged with crisp pixels.
fn cape_picture(texture: &RgbaImage) -> Arc<RenderImage> {
    let k = (texture.width() / 64).max(1);
    let (x0, y0, w, h) = (k, k, 10 * k, 16 * k);
    let crop = image::imageops::crop_imm(
        texture,
        x0,
        y0,
        w.min(texture.width().saturating_sub(x0)),
        h.min(texture.height().saturating_sub(y0)),
    )
    .to_image();
    let scaled = image::imageops::resize(
        &crop,
        CAPE_TILE.0 as u32 * 2,
        CAPE_TILE.1 as u32 * 2,
        image::imageops::FilterType::Nearest,
    );
    render_image(scaled)
}

fn is_custom(skin: &Skin) -> bool {
    matches!(skin.source, SkinSource::Custom)
}

fn section_of(skin: &Skin) -> &str {
    if is_custom(skin) {
        return "";
    }
    skin.section.as_deref().unwrap_or("")
}

impl Render for SkinsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();
        let scale = window.scale_factor().clamp(1.0, 3.0);
        if (scale - self.scale).abs() > 0.01 {
            self.scale = scale;
            self.thumbs.clear();
            self.preview_of = None;
            self.draw_thumbs(cx);
            self.draw_preview(cx);
        }
        if let Some(Err(error)) = &self.skins {
            return div()
                .size_full()
                .child(empty_state(
                    "triangle-alert",
                    "Couldn't load your skins",
                    error.clone(),
                    None,
                    cx,
                ))
                .into_any_element();
        }
        let signed_in = self.signed_in(cx);
        let preview = self.preview_panel(cx);
        let gallery = self.gallery(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .px(space(8.0))
                    .pt(space(7.0))
                    .pb(space(4.0))
                    .flex()
                    .flex_col()
                    .gap(space(1.0))
                    .child(
                        div()
                            .text_size(px(type_scale::TITLE))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.text)
                            .child("Skins"),
                    )
                    .child(
                        div()
                            .text_size(px(type_scale::SMALL))
                            .text_color(p.text_muted)
                            .child(if signed_in {
                                "Changes show in game the next time you join a world or server."
                            } else {
                                "Add a Microsoft account to change your skin. You can still look around."
                            }),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .gap(space(8.0))
                    .px(space(8.0))
                    .child(
                        div()
                            .id("skins-preview-column")
                            .flex_none()
                            .h_full()
                            .overflow_y_scroll()
                            .pb(space(8.0))
                            .child(preview),
                    )
                    .child(
                        div()
                            .id("skins-gallery")
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_y_scroll()
                            .pb(space(8.0))
                            .child(gallery),
                    ),
            )
            .into_any_element()
    }
}
