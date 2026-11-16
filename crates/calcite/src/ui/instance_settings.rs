//! Per-instance settings: name, memory, Java, launch options, window,
//! hooks, plus duplicate / export / delete.

use gpui_kit::component::Sizable as _;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::prelude::*;
use gpui_kit::{
    ClickEvent, Context, Entity, EventEmitter, FontWeight, SharedString, Window, div, px,
};
use theseus::data::InstanceSyncedOption as Synced;
use theseus::data::{
    EditInstance, Hooks, InstanceLaunchOverridesPatch, MemorySettings, WindowSize,
};

use theseus::prelude::ReleaseChannel;

use crate::backend::Backend;
use crate::images::Images;
use crate::state::AppState;
use crate::theme::{PaletteExt, radius, space, type_scale};
use crate::ui::components::{Button, icon};
use crate::ui::create_instance::DialogEvent;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    General,
    Installation,
    Java,
    Window,
    Hooks,
    Syncing,
}

/// Which instance settings replace the defaults from Settings.
#[derive(Clone, Copy, Default)]
struct Overrides {
    memory: bool,
    java: bool,
    args: bool,
    env: bool,
}

/// Per-instance sharing, as Settings → Game describes it.
const SYNC_ROWS: [(Synced, &str, &str); 6] = [
    (
        Synced::GameOptions,
        "Game settings",
        "Video, controls, sound and the rest of options.txt.",
    ),
    (
        Synced::MultiplayerServers,
        "Server list",
        "The servers on the Multiplayer screen.",
    ),
    (
        Synced::ResourcePacks,
        "Resource packs",
        "Packs in the resourcepacks folder.",
    ),
    (
        Synced::CommandHistory,
        "Command history",
        "Commands you typed in chat.",
    ),
    (
        Synced::CreativeHotbars,
        "Saved hotbars",
        "Creative mode's saved toolbars.",
    ),
    (
        Synced::Screenshots,
        "Screenshots",
        "Pictures taken with F2.",
    ),
];

pub struct InstanceSettings {
    id: String,
    section: Section,
    loaded: bool,
    name: Entity<InputState>,
    memory_mb: Entity<InputState>,
    java_path: Entity<InputState>,
    jvm_args: Entity<InputState>,
    env_vars: Entity<InputState>,
    width: Entity<InputState>,
    height: Entity<InputState>,
    pre_launch: Entity<InputState>,
    wrapper: Entity<InputState>,
    post_exit: Entity<InputState>,
    fullscreen: Option<bool>,
    system_memory_mb: u64,
    confirm_delete: bool,
    saving: bool,
    /// Which updates count: releases only, or betas / alphas too.
    channel: Option<ReleaseChannel>,
    icon: Option<std::path::PathBuf>,
    /// The linked modpack: name, version, icon.
    pack: Option<(SharedString, Option<SharedString>, Option<String>)>,
    /// Asking before a modpack change: re-install (true) or unlink (false).
    confirm_pack: Option<bool>,
    overrides: Overrides,
    memory_slider: Option<Entity<SliderState>>,
    /// Memory from Settings, shown when this instance doesn't override it.
    default_memory_mb: u32,
    /// Java installs found on this computer: (label, path).
    javas: Option<Vec<(String, String)>>,
    synced: Option<theseus::data::InstanceSyncedOptions>,
    global_synced: Option<theseus::instance::GlobalSyncedOptions>,
    /// Sharing turned back on where both copies differ: which one wins?
    sync_ask: Option<Synced>,
    sync_busy: bool,
}

impl EventEmitter<DialogEvent> for InstanceSettings {}

fn input(
    window: &mut Window,
    cx: &mut Context<InstanceSettings>,
    placeholder: &str,
) -> Entity<InputState> {
    let placeholder = placeholder.to_string();
    cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
}

impl InstanceSettings {
    pub fn new(id: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = InstanceSettings {
            name: input(window, cx, "Instance name"),
            memory_mb: input(window, cx, "Use the default"),
            java_path: input(window, cx, "Automatic (recommended)"),
            jvm_args: input(window, cx, "e.g. -XX:+UseG1GC"),
            env_vars: input(window, cx, "e.g. MESA_GL_VERSION_OVERRIDE=4.6"),
            width: input(window, cx, "Default"),
            height: input(window, cx, "Default"),
            pre_launch: input(window, cx, "Runs before the game starts"),
            wrapper: input(window, cx, "e.g. gamemoderun"),
            post_exit: input(window, cx, "Runs after the game closes"),
            id,
            section: Section::General,
            loaded: false,
            fullscreen: None,
            system_memory_mb: 0,
            confirm_delete: false,
            saving: false,
            channel: None,
            icon: None,
            pack: None,
            confirm_pack: None,
            overrides: Overrides::default(),
            memory_slider: None,
            default_memory_mb: 0,
            javas: None,
            synced: None,
            global_synced: None,
            sync_ask: None,
            sync_busy: false,
        };
        cx.subscribe_in(
            &this.memory_mb,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if let InputEvent::Change = event
                    && let Ok(mb) = Self::value(&this.memory_mb, cx).parse::<f32>()
                    && let Some(slider) = &this.memory_slider
                {
                    slider.update(cx, |s, cx| s.set_value(mb, window, cx));
                }
            },
        )
        .detach();
        #[cfg(feature = "snapshot")]
        if let Ok(section) = std::env::var("CALCITE_SNAPSHOT_SECTION") {
            this.section = match section.as_str() {
                "installation" => Section::Installation,
                "java" => Section::Java,
                "window" => Section::Window,
                "hooks" => Section::Hooks,
                "syncing" => Section::Syncing,
                _ => Section::General,
            };
        }
        this.load(window, cx);
        this
    }

    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.id.clone();
        let fetch = Backend::global(cx).call(async move {
            let meta = theseus::instance::get(&id).await?;
            let max = theseus::jre::get_max_memory().await.unwrap_or(0);
            let default_memory = theseus::settings::get()
                .await
                .map(|s| s.memory.maximum)
                .unwrap_or(2048);
            let global = theseus::instance::get_global_synced_options().await.ok();
            Ok((meta, max, default_memory, global))
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok((Some(meta), max_kb, default_memory, global)) = fetch.await else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| {
                let o = &meta.launch_overrides;
                let set = |state: &Entity<InputState>,
                           value: String,
                           window: &mut Window,
                           cx: &mut Context<Self>| {
                    state.update(cx, |s, cx| s.set_value(value, window, cx));
                };
                set(&this.name, meta.instance.name.clone(), window, cx);
                set(
                    &this.memory_mb,
                    o.memory.map(|m| m.maximum.to_string()).unwrap_or_default(),
                    window,
                    cx,
                );
                set(
                    &this.java_path,
                    o.java_path.clone().unwrap_or_default(),
                    window,
                    cx,
                );
                set(
                    &this.jvm_args,
                    o.extra_launch_args.clone().unwrap_or_default().join(" "),
                    window,
                    cx,
                );
                set(
                    &this.env_vars,
                    o.custom_env_vars
                        .clone()
                        .unwrap_or_default()
                        .into_iter()
                        .map(|(k, v)| format!("{k}={v}"))
                        .collect::<Vec<_>>()
                        .join(" "),
                    window,
                    cx,
                );
                if let Some(WindowSize(w, h)) = o.game_resolution {
                    set(&this.width, w.to_string(), window, cx);
                    set(&this.height, h.to_string(), window, cx);
                }
                set(
                    &this.pre_launch,
                    o.hooks.pre_launch.clone().unwrap_or_default(),
                    window,
                    cx,
                );
                set(
                    &this.wrapper,
                    o.hooks.wrapper.clone().unwrap_or_default(),
                    window,
                    cx,
                );
                set(
                    &this.post_exit,
                    o.hooks.post_exit.clone().unwrap_or_default(),
                    window,
                    cx,
                );
                this.fullscreen = o.force_fullscreen;
                this.channel = Some(meta.instance.update_channel);
                this.icon = meta
                    .instance
                    .icon_path
                    .clone()
                    .map(std::path::PathBuf::from);
                if !matches!(meta.link, theseus::data::InstanceLink::Unmanaged) {
                    this.load_pack(cx);
                }
                this.system_memory_mb = max_kb / 1024;
                this.default_memory_mb = default_memory;
                this.overrides = Overrides {
                    memory: o.memory.is_some(),
                    java: o.java_path.is_some(),
                    args: o.extra_launch_args.is_some(),
                    env: o.custom_env_vars.is_some(),
                };
                this.synced = Some(meta.synced_options);
                this.global_synced = global;
                let top = if this.system_memory_mb >= 1024 {
                    this.system_memory_mb as f32
                } else {
                    16384.0
                };
                let start = o.memory.map(|m| m.maximum).unwrap_or(default_memory) as f32;
                let slider = cx.new(|_| {
                    SliderState::new()
                        .min(512.0)
                        .max(top)
                        .step(256.0)
                        .default_value(start.clamp(512.0, top))
                });
                cx.subscribe_in(
                    &slider,
                    window,
                    |this, _, event: &SliderEvent, window, cx| {
                        let SliderEvent::Change(value) = event else {
                            return;
                        };
                        let mb = value.start().round() as u32;
                        this.overrides.memory = true;
                        this.memory_mb
                            .update(cx, |s, cx| s.set_value(mb.to_string(), window, cx));
                        cx.notify();
                    },
                )
                .detach();
                if o.memory.is_none() {
                    set(&this.memory_mb, default_memory.to_string(), window, cx);
                }
                this.memory_slider = Some(slider);
                this.loaded = true;
                cx.notify();
            });
        })
        .detach();
    }

    fn load_pack(&mut self, cx: &mut Context<Self>) {
        let id = self.id.clone();
        let fetch = Backend::global(cx)
            .call(async move { theseus::instance::get_linked_modpack_info(&id, None).await });
        cx.spawn(async move |this, cx| {
            if let Ok(Some(info)) = fetch.await {
                let _ = this.update(cx, |this, cx| {
                    this.pack = Some((
                        info.project.title.clone().into(),
                        info.version
                            .as_ref()
                            .map(|v| v.version_number.clone().into()),
                        info.project.icon_url.clone(),
                    ));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Pick a picture for the instance (saved right away).
    fn choose_icon(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Use as icon".into()),
        });
        let id = self.id.clone();
        let backend = Backend::global(cx).clone();
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let result = backend
                .call(async move {
                    theseus::instance::edit_icon(&id, Some(&path)).await?;
                    theseus::instance::get(&id).await
                })
                .await;
            let _ = this.update(cx, |this, cx| this.icon_saved(result, cx));
        })
        .detach();
    }

    fn remove_icon(&mut self, cx: &mut Context<Self>) {
        let id = self.id.clone();
        let task = Backend::global(cx).call(async move {
            theseus::instance::edit_icon(&id, None).await?;
            theseus::instance::get(&id).await
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| this.icon_saved(result, cx));
        })
        .detach();
    }

    fn icon_saved(
        &mut self,
        result: anyhow::Result<Option<theseus::data::InstanceMetadata>>,
        cx: &mut Context<Self>,
    ) {
        let state = AppState::global(cx);
        match result {
            Ok(meta) => {
                self.icon = meta
                    .and_then(|m| m.instance.icon_path)
                    .map(std::path::PathBuf::from);
                state.update(cx, |s, cx| s.refresh_all(cx));
            }
            Err(error) => state.update(cx, |s, cx| {
                s.notify_error("Couldn't change the icon", format!("{error:#}"));
                cx.notify();
            }),
        }
        cx.notify();
    }

    /// Look for Java installs on this computer.
    fn detect_java(&mut self, cx: &mut Context<Self>) {
        self.javas = Some(Vec::new());
        cx.notify();
        let fetch =
            Backend::global(cx).call(async { theseus::jre::find_filtered_jres(None).await });
        cx.spawn(async move |this, cx| {
            let found = fetch.await.unwrap_or_default();
            let _ = this.update(cx, |this, cx| {
                let mut javas: Vec<(String, String)> = found
                    .into_iter()
                    .map(|j| (format!("Java {} · {}", j.version, j.architecture), j.path))
                    .collect();
                javas.dedup_by(|a, b| a.1 == b.1);
                this.javas = Some(javas);
                cx.notify();
            });
        })
        .detach();
    }

    fn browse_java(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Use this Java".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| {
                this.overrides.java = true;
                this.java_path.update(cx, |s, cx| {
                    s.set_value(path.display().to_string(), window, cx)
                });
                cx.notify();
            });
        })
        .detach();
    }

    /// Share (or stop sharing) one kind of file with the other instances.
    fn set_sync(
        &mut self,
        option: Synced,
        shared: bool,
        resolution: Option<theseus::instance::SyncedOptionJoinResolution>,
        cx: &mut Context<Self>,
    ) {
        if self.sync_busy {
            return;
        }
        self.sync_busy = true;
        self.sync_ask = None;
        cx.notify();
        let id = self.id.clone();
        let task = Backend::global(cx).call(async move {
            if shared && resolution.is_none() {
                let preview =
                    theseus::instance::get_synced_option_join_preview(&id, option).await?;
                if preview.action == theseus::instance::SyncedOptionJoinAction::RequiresResolution {
                    return Ok(None);
                }
            }
            theseus::instance::set_synced_option(&id, option, shared, resolution)
                .await
                .map(|meta| Some(meta.synced_options))
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.sync_busy = false;
                match result {
                    Ok(Some(options)) => this.synced = Some(options),
                    // Both copies differ: ask which one to keep.
                    Ok(None) => this.sync_ask = Some(option),
                    Err(error) => AppState::global(cx).update(cx, |s, cx| {
                        s.notify_error("Couldn't change sharing", format!("{error:#}"));
                        cx.notify();
                    }),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn value(state: &Entity<InputState>, cx: &App) -> String {
        state.read(cx).value().trim().to_string()
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let opt = |s: String| (!s.is_empty()).then_some(s);
        let name = Self::value(&self.name, cx);
        let o = self.overrides;
        let memory = Self::value(&self.memory_mb, cx)
            .parse::<u32>()
            .ok()
            .filter(|m| *m >= 512 && o.memory);
        let args: Vec<String> = Self::value(&self.jvm_args, cx)
            .split_whitespace()
            .map(str::to_string)
            .collect();
        let env: Vec<(String, String)> = Self::value(&self.env_vars, cx)
            .split_whitespace()
            .filter_map(|pair| pair.split_once('='))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let size = match (
            Self::value(&self.width, cx).parse::<u16>(),
            Self::value(&self.height, cx).parse::<u16>(),
        ) {
            (Ok(w), Ok(h)) if w >= 320 && h >= 240 => Some(WindowSize(w, h)),
            _ => None,
        };
        let hooks = Hooks {
            pre_launch: opt(Self::value(&self.pre_launch, cx)),
            wrapper: opt(Self::value(&self.wrapper, cx)),
            post_exit: opt(Self::value(&self.post_exit, cx)),
        };
        let patch = EditInstance {
            name: opt(name),
            update_channel: self.channel,
            launch_overrides: Some(InstanceLaunchOverridesPatch {
                java_path: Some(opt(Self::value(&self.java_path, cx)).filter(|_| o.java)),
                extra_launch_args: Some((o.args && !args.is_empty()).then_some(args)),
                custom_env_vars: Some((o.env && !env.is_empty()).then_some(env)),
                memory: Some(memory.map(|maximum| MemorySettings { maximum })),
                force_fullscreen: Some(self.fullscreen),
                game_resolution: Some(size),
                hooks: Some(hooks),
                visible_tabs: None,
            }),
            ..Default::default()
        };
        let id = self.id.clone();
        self.saving = true;
        cx.notify();
        let task =
            Backend::global(cx).call(async move { theseus::instance::edit(&id, patch).await });
        let state = AppState::global(cx);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            state.update(cx, |state, cx| {
                if let Err(error) = &result {
                    state.notify_error("Couldn't save settings", format!("{error:#}"));
                }
                state.refresh_all(cx);
            });
            let _ = this.update(cx, |this, cx| {
                this.saving = false;
                if result.is_ok() {
                    cx.emit(DialogEvent::Close);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn duplicate(&mut self, cx: &mut Context<Self>) {
        let id = self.id.clone();
        let task =
            Backend::global(cx).call(async move { theseus::install::duplicate_instance(id).await });
        let state = AppState::global(cx);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            state.update(cx, |state, cx| match result {
                Ok(snapshot) => state.track_job(snapshot, cx),
                Err(error) => state.notify_error("Couldn't duplicate", format!("{error:#}")),
            });
            let _ = this.update(cx, |_, cx| cx.emit(DialogEvent::Close));
        })
        .detach();
    }

    fn export(&mut self, cx: &mut Context<Self>) {
        let name = Self::value(&self.name, cx);
        export_instance(self.id.clone(), name, cx);
    }

    /// Re-download and re-check every game file (mods untouched).
    fn repair(&mut self, cx: &mut Context<Self>) {
        let id = self.id.clone();
        let task = Backend::global(cx)
            .call(async move { theseus::install::install_existing_instance(id, true).await });
        let state = AppState::global(cx);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            state.update(cx, |state, cx| match result {
                Ok(job) => state.track_job(job, cx),
                Err(error) => {
                    state.notify_error("Couldn't repair the instance", format!("{error:#}"))
                }
            });
            let _ = this.update(cx, |_, cx| cx.emit(DialogEvent::Close));
        })
        .detach();
    }

    /// A setting that is either the default from Settings or this
    /// instance's own: a switch, then the controls (dimmed when off).
    fn override_row(
        key: &'static str,
        title: &'static str,
        hint: String,
        on: bool,
        flip: fn(&mut Overrides),
        body: gpui_kit::AnyElement,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let p = cx.palette().clone();
        div()
            .flex()
            .flex_col()
            .gap(space(2.0))
            .child(
                div()
                    .id(key)
                    .flex()
                    .items_center()
                    .gap(space(3.0))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        flip(&mut this.overrides);
                        cx.notify();
                    }))
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
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_size(px(type_scale::CAPTION))
                                    .text_color(p.text_muted)
                                    .child(hint),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .child(crate::ui::pages::instance::toggle(on, cx)),
                    ),
            )
            .child(div().when(!on, |d| d.opacity(0.4)).child(body))
            .into_any_element()
    }

    fn java_section(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let p = cx.palette().clone();
        let o = self.overrides;
        let memory_hint = if o.memory {
            format!(
                "This instance uses its own amount.{}",
                if self.system_memory_mb > 0 {
                    format!(" This computer has {} MB.", self.system_memory_mb)
                } else {
                    String::new()
                }
            )
        } else {
            format!(
                "Uses {} MB, the default from Settings.",
                self.default_memory_mb
            )
        };
        let memory = div()
            .flex()
            .items_center()
            .gap(space(3.0))
            .child(
                div()
                    .text_size(px(type_scale::CAPTION))
                    .text_color(p.text_muted)
                    .child("512 MB"),
            )
            .child(
                div().flex_1().children(
                    self.memory_slider
                        .as_ref()
                        .map(|s| Slider::new(s).disabled(!o.memory)),
                ),
            )
            .child(
                div()
                    .text_size(px(type_scale::CAPTION))
                    .text_color(p.text_muted)
                    .child(format!("{} MB", self.system_memory_mb.max(1024))),
            )
            .child(
                div()
                    .w(px(90.0))
                    .child(Input::new(&self.memory_mb).small().disabled(!o.memory)),
            );

        let mut java = div().flex().flex_col().gap(space(2.0)).child(
            div()
                .flex()
                .gap(space(2.0))
                .child(
                    div()
                        .flex_1()
                        .child(Input::new(&self.java_path).disabled(!o.java)),
                )
                .child(
                    Button::new("is-java-detect")
                        .icon("search")
                        .label("Detect")
                        .disabled(!o.java)
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.detect_java(cx))),
                )
                .child(
                    Button::new("is-java-browse")
                        .icon("folder-open")
                        .label("Browse…")
                        .disabled(!o.java)
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.browse_java(window, cx)
                        })),
                ),
        );
        if let (Some(found), true) = (&self.javas, o.java) {
            let mut list = div()
                .flex()
                .flex_col()
                .rounded(px(radius::SM))
                .border_1()
                .border_color(p.border)
                .p(space(1.0));
            if found.is_empty() {
                list = list.child(
                    div()
                        .p(space(2.0))
                        .text_size(px(type_scale::SMALL))
                        .text_color(p.text_muted)
                        .child("Looking for Java… (none found yet)"),
                );
            }
            for (ix, (label, path)) in found.iter().enumerate() {
                let pick = path.clone();
                list = list.child(
                    div()
                        .id(SharedString::from(format!("is-java-{ix}")))
                        .flex()
                        .flex_col()
                        .px(space(2.0))
                        .py(space(1.5))
                        .rounded(px(radius::SM))
                        .cursor_pointer()
                        .hover(|s| s.bg(p.hover))
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.java_path
                                .update(cx, |s, cx| s.set_value(pick.clone(), window, cx));
                            this.javas = None;
                            cx.notify();
                        }))
                        .child(
                            div()
                                .text_size(px(type_scale::SMALL))
                                .text_color(p.text)
                                .child(label.clone()),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(px(type_scale::CAPTION))
                                .font_family(crate::theme::fonts::MONO)
                                .text_color(p.text_muted)
                                .child(path.clone()),
                        ),
                );
            }
            java = java.child(list);
        }

        div()
            .id("is-java")
            .h_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(space(5.0))
            .child(Self::override_row(
                "is-o-memory",
                "Custom memory",
                memory_hint,
                o.memory,
                |o| o.memory = !o.memory,
                memory.into_any_element(),
                cx,
            ))
            .child(Self::override_row(
                "is-o-java",
                "Custom Java",
                if o.java {
                    "Runs this instance with the Java you pick.".into()
                } else {
                    "Calcite picks and installs the right Java for this version.".into()
                },
                o.java,
                |o| o.java = !o.java,
                java.into_any_element(),
                cx,
            ))
            .child(Self::override_row(
                "is-o-args",
                "Custom JVM arguments",
                "Extra flags for Java, like -XX:+UseZGC.".into(),
                o.args,
                |o| o.args = !o.args,
                Input::new(&self.jvm_args)
                    .disabled(!o.args)
                    .into_any_element(),
                cx,
            ))
            .child(Self::override_row(
                "is-o-env",
                "Custom environment variables",
                "KEY=value pairs separated by spaces.".into(),
                o.env,
                |o| o.env = !o.env,
                Input::new(&self.env_vars)
                    .disabled(!o.env)
                    .into_any_element(),
                cx,
            ))
            .into_any_element()
    }

    fn syncing_section(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let p = cx.palette().clone();
        let mut body = div()
            .id("is-sharing")
            .h_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(space(1.0))
            .child(
            div()
                .pb(space(2.0))
                .text_size(px(type_scale::SMALL))
                .text_color(p.text_muted)
                .child("Turned on in Settings → Game, these are shared by every instance. Turn one off here to keep this instance's own copy. Changes apply right away."),
        );
        let (Some(synced), Some(global)) = (self.synced, self.global_synced) else {
            return body.into_any_element();
        };
        for (option, title, hint) in SYNC_ROWS {
            let everywhere = global.get(option);
            let shared = everywhere && synced_flag(&synced, option);
            let asking = self.sync_ask == Some(option);
            let mut row = div()
                .flex()
                .flex_col()
                .py(space(2.0))
                .border_b_1()
                .border_color(p.border)
                .child(
                    div()
                        .id(SharedString::from(format!("is-sync-{title}")))
                        .flex()
                        .items_center()
                        .gap(space(3.0))
                        .when(everywhere, |d| d.cursor_pointer())
                        .when(!everywhere, |d| d.opacity(0.5))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            if everywhere {
                                this.set_sync(option, !shared, None, cx);
                            }
                        }))
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
                                        .child(title),
                                )
                                .child(
                                    div()
                                        .text_size(px(type_scale::CAPTION))
                                        .text_color(p.text_muted)
                                        .child(if everywhere {
                                            if shared {
                                                format!("{hint} Shared with your other instances.")
                                            } else {
                                                format!("{hint} This instance keeps its own.")
                                            }
                                        } else {
                                            format!("{hint} Not shared: turn it on in Settings → Game first.")
                                        }),
                                ),
                        )
                        .child(div().flex_none().child(crate::ui::pages::instance::toggle(shared, cx))),
                );
            if asking {
                row = row.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space(2.0))
                        .mt(space(2.0))
                        .p(space(2.5))
                        .rounded(px(radius::SM))
                        .bg(p.warning_soft)
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(type_scale::SMALL))
                                .text_color(p.text)
                                .child("This instance's copy and the shared one are different. Which should win?"),
                        )
                        .child(
                            Button::new("is-sync-use-shared")
                                .label("Use the shared one")
                                .small()
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.set_sync(
                                        option,
                                        true,
                                        Some(theseus::instance::SyncedOptionJoinResolution::UseSynced),
                                        cx,
                                    )
                                })),
                        )
                        .child(
                            Button::new("is-sync-use-mine")
                                .label("Keep this one")
                                .small()
                                .primary()
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.set_sync(
                                        option,
                                        true,
                                        Some(theseus::instance::SyncedOptionJoinResolution::UseInstance),
                                        cx,
                                    )
                                })),
                        ),
                );
            }
            body = body.child(row);
        }
        body.into_any_element()
    }

    fn icon_editor(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let p = cx.palette().clone();
        let name = AppState::global(cx)
            .read(cx)
            .instance(&self.id)
            .map(|i| i.name.clone())
            .unwrap_or_default();
        let source = self
            .icon
            .as_deref()
            .and_then(|path| Images::get_file(path, 112, cx));
        let has_icon = self.icon.is_some();
        div()
            .flex()
            .items_center()
            .gap(space(2.0))
            .child(
                div()
                    .id("is-icon")
                    .cursor_pointer()
                    .rounded(px(radius::MD))
                    .border_1()
                    .border_color(p.border)
                    .hover(|s| s.border_color(p.border_strong))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.choose_icon(cx)))
                    .child(crate::ui::components::thumb(
                        source,
                        &name,
                        56.0,
                        radius::MD,
                        cx,
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(space(1.0))
                    .child(
                        Button::new("is-icon-change")
                            .icon("image")
                            .label("Change icon…")
                            .small()
                            .on_click(
                                cx.listener(|this, _: &ClickEvent, _, cx| this.choose_icon(cx)),
                            ),
                    )
                    .when(has_icon, |d| {
                        d.child(
                            Button::new("is-icon-remove")
                                .icon("x")
                                .label("Remove")
                                .small()
                                .ghost()
                                .on_click(
                                    cx.listener(|this, _: &ClickEvent, _, cx| this.remove_icon(cx)),
                                ),
                        )
                    }),
            )
            .into_any_element()
    }

    fn channel_picker(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let p = cx.palette().clone();
        let current = self.channel.unwrap_or(ReleaseChannel::Release);
        div()
            .flex()
            .gap(space(1.0))
            .children(
                [
                    (ReleaseChannel::Release, "Release"),
                    (ReleaseChannel::Beta, "Beta"),
                    (ReleaseChannel::Alpha, "Alpha"),
                ]
                .into_iter()
                .map(|(channel, label)| {
                    let active = current == channel;
                    div()
                        .id(SharedString::from(format!("is-channel-{label}")))
                        .flex()
                        .items_center()
                        .gap(space(1.5))
                        .h(px(30.0))
                        .px(space(3.0))
                        .rounded(px(radius::SM))
                        .text_size(px(type_scale::BODY))
                        .cursor_pointer()
                        .when(active, |d| {
                            d.bg(p.raised)
                                .text_color(p.text)
                                .border_1()
                                .border_color(p.border_strong)
                        })
                        .when(!active, |d| {
                            d.text_color(p.text_secondary).hover(|s| s.bg(p.hover))
                        })
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.channel = Some(channel);
                            cx.notify();
                        }))
                        .when(active, |d| d.child(icon("check", 12.0, p.accent)))
                        .child(label)
                }),
            )
            .into_any_element()
    }

    /// The modpack this instance follows: switch version, re-install, unlink.
    fn modpack_block(&self, cx: &mut Context<Self>) -> Option<gpui_kit::AnyElement> {
        let (title, version, icon_url) = self.pack.clone()?;
        let p = cx.palette().clone();
        let source = icon_url.as_deref().and_then(|u| Images::get(u, 64, cx));
        let note = |text: String| {
            div()
                .text_size(px(type_scale::CAPTION))
                .text_color(p.text_muted)
                .child(text)
        };
        let confirm = self.confirm_pack.map(|reinstall| {
            div()
                .flex()
                .items_center()
                .gap(space(2.0))
                .p(space(2.5))
                .rounded(px(radius::SM))
                .bg(if reinstall {
                    p.danger_soft
                } else {
                    p.warning_soft
                })
                .child(
                    div()
                        .flex_1()
                        .text_size(px(type_scale::SMALL))
                        .text_color(p.text)
                        .child(if reinstall {
                            "Re-install? Anything you added to this instance is removed."
                        } else {
                            "Unlink? This can't be undone."
                        }),
                )
                .child(
                    Button::new("is-pack-no")
                        .label("Cancel")
                        .small()
                        .ghost()
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.confirm_pack = None;
                            cx.notify();
                        })),
                )
                .child({
                    let b = Button::new("is-pack-yes")
                        .label(if reinstall { "Re-install" } else { "Unlink" })
                        .small();
                    (if reinstall { b.danger() } else { b.primary() }).on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| {
                            let id = this.id.clone();
                            let state = AppState::global(cx);
                            if reinstall {
                                state.update(cx, |s, cx| s.reinstall_modpack(id, cx));
                            } else {
                                state.update(cx, |s, cx| s.unlink_modpack(id, cx));
                            }
                            cx.emit(DialogEvent::Close);
                        },
                    ))
                })
        });
        Some(
            div()
                .flex()
                .flex_col()
                .gap(space(3.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space(3.0))
                        .p(space(2.5))
                        .rounded(px(radius::MD))
                        .bg(p.surface)
                        .border_1()
                        .border_color(p.border)
                        .child(crate::ui::components::thumb(source, &title, 36.0, radius::SM, cx))
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
                                        .child(title.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(type_scale::SMALL))
                                        .text_color(p.text_muted)
                                        .child(match &version {
                                            Some(v) => format!("Modpack · {v}"),
                                            None => "Modpack".into(),
                                        }),
                                ),
                        )
                        .child(
                            Button::new("is-pack-switch")
                                .icon("arrow-left-right")
                                .label("Switch version…")
                                .small()
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    cx.emit(DialogEvent::SwitchModpack(this.id.clone()))
                                })),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .gap(space(2.0))
                        .child(
                            Button::new("is-pack-reinstall")
                                .icon("rotate-ccw")
                                .label("Re-install modpack")
                                .small()
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.confirm_pack = Some(true);
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("is-pack-unlink")
                                .icon("unlink")
                                .label("Unlink modpack")
                                .small()
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.confirm_pack = Some(false);
                                    cx.notify();
                                })),
                        ),
                )
                .children(confirm)
                .child(note(format!(
                    "Re-install puts {title} back the way it ships (a snapshot is saved first). Unlink keeps everything installed and lets you change the Minecraft version and loader, but the pack's updates stop."
                )))
                .into_any_element(),
        )
    }

    fn installation(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let p = cx.palette().clone();
        let inst = AppState::global(cx).read(cx).instance(&self.id).cloned();
        let row = |label: &'static str, value: String| {
            div()
                .flex()
                .justify_between()
                .py(space(2.0))
                .border_b_1()
                .border_color(p.border)
                .text_size(px(type_scale::BODY))
                .child(div().text_color(p.text_muted).child(label))
                .child(div().text_color(p.text).child(value))
        };
        let note = |text: &'static str| {
            div()
                .text_size(px(type_scale::CAPTION))
                .text_color(p.text_muted)
                .child(text)
        };
        let mut info = div().flex().flex_col();
        if let Some(inst) = &inst {
            info = info
                .child(row(
                    "Mod loader",
                    crate::state::loader_name(inst.loader).to_string(),
                ))
                .child(row("Minecraft version", inst.game_version.to_string()))
                .when_some(inst.loader_version.clone(), |d, v| {
                    d.child(row("Loader version", v.to_string()))
                });
        }
        let modpack = inst.as_ref().is_some_and(|i| i.is_modpack);
        div()
            .id("is-installation")
            .flex()
            .flex_col()
            .gap(space(5.0))
            .h_full()
            .overflow_y_scroll()
            .child(info)
            .children(self.modpack_block(cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(space(2.0))
                    .child(
                        div().flex().child(
                            Button::new("is-change-version")
                                .icon("refresh-cw")
                                .label("Change version…")
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    cx.emit(DialogEvent::ChangeVersion(this.id.clone()))
                                })),
                        ),
                    )
                    .child(note(if modpack {
                        "This instance comes from a modpack; changing the version can break it. Update the modpack instead when you can."
                    } else {
                        "Switch Minecraft version or mod loader. Worlds and settings stay."
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(space(2.0))
                    .child(
                        div().flex().child(
                            Button::new("is-repair")
                                .icon("wrench")
                                .label("Repair")
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.repair(cx))),
                        ),
                    )
                    .child(note("If the game won't start or files went missing: checks and re-downloads Minecraft, the loader, libraries and Java. Your mods and worlds aren't touched.")),
            )
            .into_any_element()
    }

    fn delete(&mut self, cx: &mut Context<Self>) {
        let id = self.id.clone();
        let task = Backend::global(cx).call(async move { theseus::instance::remove(&id).await });
        let state = AppState::global(cx);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            state.update(cx, |state, cx| {
                if let Err(error) = result {
                    state.notify_error("Couldn't delete the instance", format!("{error:#}"));
                }
                state.refresh_all(cx);
            });
            let _ = this.update(cx, |_, cx| cx.emit(DialogEvent::Close));
        })
        .detach();
    }
}

use gpui_kit::App;

/// Save an instance as a .mrpack (asks where). Worlds, logs and
/// screenshots stay out.
pub fn export_instance(id: String, name: String, cx: &mut App) {
    let file = format!(
        "{}.mrpack",
        if name.is_empty() { "instance" } else { &name }
    );
    let dir = dirs::download_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_default();
    let target = cx.prompt_for_new_path(&dir, Some(&file));
    let backend = Backend::global(cx).clone();
    let state = AppState::global(cx);
    cx.spawn(async move |cx| {
        let Ok(Ok(Some(path))) = target.await else {
            return;
        };
        let export_path = path.clone();
        let result = backend
            .call(async move {
                // Everything except worlds, logs and screenshots.
                let skip = ["saves", "logs", "crash-reports", "screenshots", "backups"];
                let candidates = theseus::instance::get_pack_export_candidates(&id).await?;
                let mut include = Vec::new();
                let mut exclude = Vec::new();
                for candidate in candidates {
                    let path = serde_json::to_value(&candidate)
                        .ok()
                        .and_then(|v| v.get("path").and_then(|p| p.as_str()).map(str::to_string))
                        .unwrap_or_default();
                    if skip
                        .iter()
                        .any(|s| path == *s || path.starts_with(&format!("{s}/")))
                    {
                        exclude.push(path);
                    } else {
                        include.push(path);
                    }
                }
                theseus::instance::export_mrpack(
                    &id,
                    export_path,
                    include,
                    exclude,
                    None,
                    None,
                    None,
                )
                .await
            })
            .await;
        state.update(cx, |state, cx| {
            match result {
                Ok(()) => {
                    state.notice = Some(crate::state::Notice {
                        title: "Exported".into(),
                        body: format!("Saved to {}", path.display()).into(),
                        is_error: false,
                    })
                }
                Err(error) => state.notify_error("Export didn't finish", format!("{error:#}")),
            }
            cx.notify();
        });
    })
    .detach();
}

impl Render for InstanceSettings {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();
        let field = |label: &'static str, hint: Option<String>, body: gpui_kit::AnyElement| {
            div()
                .flex()
                .flex_col()
                .gap(space(1.5))
                .child(
                    div()
                        .text_size(px(type_scale::SMALL))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(p.text_secondary)
                        .child(label),
                )
                .child(body)
                .when_some(hint, |d, h| {
                    d.child(
                        div()
                            .text_size(px(type_scale::CAPTION))
                            .text_color(p.text_muted)
                            .child(h),
                    )
                })
        };

        let mut nav = div()
            .flex()
            .flex_col()
            .gap(space(0.5))
            .w(px(150.0))
            .flex_none();
        for (section, label, glyph) in [
            (Section::General, "General", "settings"),
            (Section::Installation, "Installation", "wrench"),
            (Section::Java, "Java & memory", "memory-stick"),
            (Section::Window, "Window", "monitor"),
            (Section::Hooks, "Launch hooks", "terminal"),
            (Section::Syncing, "Sharing", "refresh-cw"),
        ] {
            let active = section == self.section;
            nav = nav.child(
                div()
                    .id(SharedString::from(format!("is-{label}")))
                    .flex()
                    .items_center()
                    .gap(space(2.0))
                    .h(px(30.0))
                    .px(space(2.5))
                    .rounded(px(radius::SM))
                    .text_size(px(type_scale::BODY))
                    .cursor_pointer()
                    .when(active, |d| d.bg(p.hover).text_color(p.text))
                    .when(!active, |d| {
                        d.text_color(p.text_secondary).hover(|s| s.bg(p.hover))
                    })
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.section = section;
                        cx.notify();
                    }))
                    .child(icon(
                        glyph,
                        14.0,
                        if active { p.accent } else { p.text_muted },
                    ))
                    .child(label),
            );
        }

        let body: gpui_kit::AnyElement = match self.section {
            Section::General => div()
                .flex()
                .flex_col()
                .gap(space(5.0))
                .child(
                    div()
                        .flex()
                        .items_end()
                        .gap(space(4.0))
                        .child(div().flex_1().child(field("Name", None, Input::new(&self.name).into_any_element())))
                        .child(self.icon_editor(cx)),
                )
                .child(field(
                    "Updates",
                    Some(match self.channel {
                        Some(ReleaseChannel::Alpha) => "Offers every version, even early alphas. Expect things to break.".into(),
                        Some(ReleaseChannel::Beta) => "Offers releases and betas when checking for updates.".into(),
                        _ => "Offers only full releases when checking for updates.".into(),
                    }),
                    self.channel_picker(cx),
                ))
                .child(field(
                    "Copy or share",
                    None,
                    div()
                        .flex()
                        .gap(space(2.0))
                        .child(
                            Button::new("is-duplicate")
                                .icon("copy")
                                .label("Duplicate")
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.duplicate(cx))),
                        )
                        .child(
                            Button::new("is-export")
                                .icon("share-2")
                                .label("Export .mrpack")
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.export(cx))),
                        )
                        .into_any_element(),
                ))
                .child(field(
                    "Delete",
                    Some("Removes the instance and its worlds from this computer. This can't be undone.".into()),
                    if self.confirm_delete {
                        div()
                            .flex()
                            .gap(space(2.0))
                            .child(
                                Button::new("is-delete-confirm")
                                    .icon("trash")
                                    .label("Yes, delete it")
                                    .danger()
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.delete(cx))),
                            )
                            .child(
                                Button::new("is-delete-cancel")
                                    .label("Keep it")
                                    .ghost()
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.confirm_delete = false;
                                        cx.notify();
                                    })),
                            )
                            .into_any_element()
                    } else {
                        div()
                            .flex()
                            .child(
                                Button::new("is-delete")
                                    .icon("trash")
                                    .label("Delete instance")
                                    .danger()
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.confirm_delete = true;
                                        cx.notify();
                                    })),
                            )
                            .into_any_element()
                    },
                ))
                .into_any_element(),
            Section::Installation => self.installation(cx),
            Section::Java => self.java_section(cx),
            Section::Syncing => self.syncing_section(cx),
            Section::Window => {
                let full = self.fullscreen;
                div()
                    .flex()
                    .flex_col()
                    .gap(space(5.0))
                    .child(field(
                        "Start fullscreen",
                        None,
                        div()
                            .flex()
                            .gap(space(1.0))
                            .children([("Default", None), ("On", Some(true)), ("Off", Some(false))].into_iter().map(
                                |(label, value)| {
                                    let active = full == value;
                                    div()
                                        .id(SharedString::from(format!("fs-{label}")))
                                        .h(px(30.0))
                                        .px(space(3.0))
                                        .flex()
                                        .items_center()
                                        .rounded(px(radius::SM))
                                        .text_size(px(type_scale::BODY))
                                        .cursor_pointer()
                                        .when(active, |d| d.bg(p.raised).text_color(p.text).border_1().border_color(p.border_strong))
                                        .when(!active, |d| d.text_color(p.text_secondary))
                                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                            this.fullscreen = value;
                                            cx.notify();
                                        }))
                                        .child(label)
                                },
                            ))
                            .into_any_element(),
                    ))
                    .child(field(
                        "Window size",
                        Some("Width and height in pixels. Leave empty for the default.".into()),
                        div()
                            .flex()
                            .items_center()
                            .gap(space(2.0))
                            .child(div().w(px(120.0)).child(Input::new(&self.width)))
                            .child(div().text_color(p.text_muted).child("×"))
                            .child(div().w(px(120.0)).child(Input::new(&self.height)))
                            .into_any_element(),
                    ))
                    .into_any_element()
            }
            Section::Hooks => div()
                .flex()
                .flex_col()
                .gap(space(5.0))
                .child(field("Before launch", None, Input::new(&self.pre_launch).into_any_element()))
                .child(field(
                    "Wrapper",
                    Some("A program the game runs inside, like gamemoderun or prime-run.".into()),
                    Input::new(&self.wrapper).into_any_element(),
                ))
                .child(field("After exit", None, Input::new(&self.post_exit).into_any_element()))
                .into_any_element(),
        };

        div()
            .w(px(680.0))
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
                    .pb(space(3.0))
                    .child(
                        div()
                            .text_size(px(type_scale::SECTION))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.text)
                            .child("Instance settings"),
                    )
                    .child(Button::new("is-close").icon("x").ghost().small().on_click(
                        cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(DialogEvent::Close)),
                    )),
            )
            .child(
                div()
                    .flex()
                    .gap(space(5.0))
                    .px(space(6.0))
                    .py(space(3.0))
                    .h(px(400.0))
                    .child(nav)
                    .child(div().flex_1().min_w_0().child(body)),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(space(2.0))
                    .px(space(6.0))
                    .py(space(4.0))
                    .border_t_1()
                    .border_color(p.border)
                    .child(Button::new("is-cancel").label("Cancel").on_click(
                        cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(DialogEvent::Close)),
                    ))
                    .child(
                        Button::new("is-save")
                            .icon("check")
                            .label(if self.saving { "Saving…" } else { "Save" })
                            .primary()
                            .disabled(!self.loaded || self.saving)
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.save(cx))),
                    ),
            )
    }
}

fn synced_flag(options: &theseus::data::InstanceSyncedOptions, option: Synced) -> bool {
    match option {
        Synced::GameOptions => options.game_options,
        Synced::CommandHistory => options.command_history,
        Synced::MultiplayerServers => options.multiplayer_servers,
        Synced::CreativeHotbars => options.creative_hotbars,
        Synced::Screenshots => options.screenshots,
        Synced::ResourcePacks => options.resource_packs,
        Synced::DataPacks => options.data_packs,
    }
}
