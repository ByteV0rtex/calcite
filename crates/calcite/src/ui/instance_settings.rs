//! Per-instance settings: name, memory, Java, launch options, window,
//! hooks, plus duplicate / export / delete.

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{
    ClickEvent, Context, Entity, EventEmitter, FontWeight, SharedString, Window, div, px,
};
use theseus::data::{
    EditInstance, Hooks, InstanceLaunchOverridesPatch, MemorySettings, WindowSize,
};

use crate::backend::Backend;
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
}

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
        };
        this.load(window, cx);
        this
    }

    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.id.clone();
        let fetch = Backend::global(cx).call(async move {
            let meta = theseus::instance::get(&id).await?;
            let max = theseus::jre::get_max_memory().await.unwrap_or(0);
            Ok((meta, max))
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok((Some(meta), max_kb)) = fetch.await else {
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
                this.system_memory_mb = max_kb / 1024;
                this.loaded = true;
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
        let memory = Self::value(&self.memory_mb, cx)
            .parse::<u32>()
            .ok()
            .filter(|m| *m >= 512);
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
            launch_overrides: Some(InstanceLaunchOverridesPatch {
                java_path: Some(opt(Self::value(&self.java_path, cx))),
                extra_launch_args: Some((!args.is_empty()).then_some(args)),
                custom_env_vars: Some((!env.is_empty()).then_some(env)),
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
        let file = format!(
            "{}.mrpack",
            if name.is_empty() { "instance" } else { &name }
        );
        let dir = dirs::download_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_default();
        let target = cx.prompt_for_new_path(&dir, Some(&file));
        let id = self.id.clone();
        let backend = Backend::global(cx).clone();
        let state = AppState::global(cx);
        cx.spawn(async move |_, cx| {
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
                            .and_then(|v| {
                                v.get("path").and_then(|p| p.as_str()).map(str::to_string)
                            })
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
            .flex()
            .flex_col()
            .gap(space(5.0))
            .child(info)
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
                .child(field("Name", None, Input::new(&self.name).into_any_element()))
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
            Section::Java => div()
                .flex()
                .flex_col()
                .gap(space(5.0))
                .child(field(
                    "Maximum memory (MB)",
                    Some(if self.system_memory_mb > 0 {
                        format!(
                            "This computer has {} MB. Leave empty to use the default from Settings.",
                            self.system_memory_mb
                        )
                    } else {
                        "Leave empty to use the default from Settings.".into()
                    }),
                    Input::new(&self.memory_mb).into_any_element(),
                ))
                .child(field(
                    "Java executable",
                    Some("Leave empty and Calcite picks and installs the right Java for this version.".into()),
                    Input::new(&self.java_path).into_any_element(),
                ))
                .child(field("Extra JVM arguments", None, Input::new(&self.jvm_args).into_any_element()))
                .child(field(
                    "Environment variables",
                    Some("KEY=value pairs separated by spaces.".into()),
                    Input::new(&self.env_vars).into_any_element(),
                ))
                .into_any_element(),
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
