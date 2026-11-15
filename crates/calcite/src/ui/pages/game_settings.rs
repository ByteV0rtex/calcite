//! Settings › Game: Java runtimes and the defaults every instance starts from.

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{ClickEvent, Context, Entity, FontWeight, SharedString, Window, div, px};
use theseus::data::{MemorySettings, Settings, WindowSize};

use crate::backend::Backend;
use crate::state::AppState;
use crate::theme::{PaletteExt, radius, space, type_scale};
use crate::ui::components::{Button, icon, panel, section_heading, state_chip};

/// The Java versions Minecraft has needed over the years.
const JAVA_MAJORS: [(u32, &str); 4] = [
    (8, "Minecraft 1.16 and older"),
    (17, "Minecraft 1.17 to 1.20.4"),
    (21, "Minecraft 1.20.5 to 1.21"),
    (25, "Minecraft 26.1 and newer"),
];

pub struct GameSettings {
    settings: Option<Settings>,
    java: Vec<(u32, Option<String>)>,
    installing: Vec<u32>,
    system_memory_mb: u64,
    memory: Entity<InputState>,
    jvm_args: Entity<InputState>,
    env_vars: Entity<InputState>,
    width: Entity<InputState>,
    height: Entity<InputState>,
    saved: bool,
}

impl GameSettings {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let field = |placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
        };
        let mut this = GameSettings {
            settings: None,
            java: Vec::new(),
            installing: Vec::new(),
            system_memory_mb: 0,
            memory: field("e.g. 4096", window, cx),
            jvm_args: field("e.g. -XX:+UseZGC -XX:+ZGenerational", window, cx),
            env_vars: field("KEY=value KEY2=value", window, cx),
            width: field("854", window, cx),
            height: field("480", window, cx),
            saved: false,
        };
        this.load(window, cx);
        this
    }

    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let fetch = Backend::global(cx).call(async {
            let settings = theseus::settings::get().await?;
            let java = theseus::jre::get_java_versions().await?;
            let max = theseus::jre::get_max_memory().await.unwrap_or(0);
            let java: Vec<(u32, Option<String>)> = JAVA_MAJORS
                .iter()
                .map(|(major, _)| (*major, java.get(major).map(|j| j.path.clone())))
                .collect();
            Ok((settings, java, max))
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok((settings, java, max_kb)) = fetch.await else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| {
                let set = |state: &Entity<InputState>,
                           value: String,
                           window: &mut Window,
                           cx: &mut Context<Self>| {
                    state.update(cx, |s, cx| s.set_value(value, window, cx));
                };
                set(
                    &this.memory,
                    settings.memory.maximum.to_string(),
                    window,
                    cx,
                );
                set(
                    &this.jvm_args,
                    settings.extra_launch_args.join(" "),
                    window,
                    cx,
                );
                set(
                    &this.env_vars,
                    settings
                        .custom_env_vars
                        .iter()
                        .map(|(k, v)| format!("{k}={v}"))
                        .collect::<Vec<_>>()
                        .join(" "),
                    window,
                    cx,
                );
                set(
                    &this.width,
                    settings.game_resolution.0.to_string(),
                    window,
                    cx,
                );
                set(
                    &this.height,
                    settings.game_resolution.1.to_string(),
                    window,
                    cx,
                );
                this.settings = Some(settings);
                this.java = java;
                this.system_memory_mb = max_kb / 1024;
                cx.notify();
            });
        })
        .detach();
    }

    fn install_java(&mut self, major: u32, window: &mut Window, cx: &mut Context<Self>) {
        self.installing.push(major);
        cx.notify();
        let task = Backend::global(cx).call(async move {
            let path = theseus::jre::auto_install_java(major).await?;
            let version = theseus::jre::check_jre(path).await?;
            theseus::jre::set_java_version(version).await
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.installing.retain(|m| *m != major);
                if let Err(error) = result {
                    AppState::global(cx).update(cx, |s, cx| {
                        s.notify_error(
                            format!("Couldn't install Java {major}"),
                            format!("{error:#}"),
                        );
                        cx.notify();
                    });
                }
                this.load(window, cx);
            });
        })
        .detach();
    }

    fn update(&mut self, edit: impl FnOnce(&mut Settings), cx: &mut Context<Self>) {
        let Some(settings) = self.settings.as_mut() else {
            return;
        };
        edit(settings);
        let snapshot = settings.clone();
        self.saved = false;
        cx.notify();
        let task = Backend::global(cx).call(async move { theseus::settings::set(snapshot).await });
        cx.spawn(async move |this, cx| {
            let ok = task.await.is_ok();
            let _ = this.update(cx, |this, cx| {
                this.saved = ok;
                cx.notify();
            });
        })
        .detach();
    }

    fn save_defaults(&mut self, cx: &mut Context<Self>) {
        let read = |s: &Entity<InputState>, cx: &App| s.read(cx).value().trim().to_string();
        let memory = read(&self.memory, cx)
            .parse::<u32>()
            .ok()
            .filter(|m| *m >= 512);
        let args: Vec<String> = read(&self.jvm_args, cx)
            .split_whitespace()
            .map(str::to_string)
            .collect();
        let env: Vec<(String, String)> = read(&self.env_vars, cx)
            .split_whitespace()
            .filter_map(|p| p.split_once('='))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let size = (
            read(&self.width, cx).parse::<u16>(),
            read(&self.height, cx).parse::<u16>(),
        );
        self.update(
            move |s| {
                if let Some(maximum) = memory {
                    s.memory = MemorySettings { maximum };
                }
                s.extra_launch_args = args;
                s.custom_env_vars = env;
                if let (Ok(w), Ok(h)) = size
                    && w >= 320
                    && h >= 240
                {
                    s.game_resolution = WindowSize(w, h);
                }
            },
            cx,
        );
    }
}

use gpui_kit::App;

fn toggle_row(
    id: &'static str,
    label: &'static str,
    hint: &'static str,
    on: bool,
    cx: &mut Context<GameSettings>,
    flip: impl Fn(&mut Settings) + 'static,
) -> impl IntoElement {
    let p = cx.palette().clone();
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(space(3.0))
        .py(space(2.5))
        .cursor_pointer()
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.update(|s| flip(s), cx)))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .child(
                    div()
                        .text_size(px(type_scale::BODY))
                        .text_color(p.text)
                        .child(label),
                )
                .child(
                    div()
                        .text_size(px(type_scale::SMALL))
                        .text_color(p.text_muted)
                        .child(hint),
                ),
        )
        .child(
            div()
                .w(px(32.0))
                .h(px(18.0))
                .flex_none()
                .rounded_full()
                .p(px(2.0))
                .bg(if on { p.accent } else { p.border_strong })
                .flex()
                .when(on, |d| d.justify_end())
                .child(div().size(px(14.0)).rounded_full().bg(if on {
                    p.on_accent
                } else {
                    p.text_secondary
                })),
        )
}

impl Render for GameSettings {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();
        let Some(settings) = self.settings.clone() else {
            return div()
                .p(space(8.0))
                .text_color(p.text_muted)
                .child("Loading…")
                .into_any_element();
        };

        let mut java_list = panel(cx).overflow_hidden();
        for (ix, (major, path)) in self.java.clone().into_iter().enumerate() {
            let note = JAVA_MAJORS
                .iter()
                .find(|(m, _)| *m == major)
                .map(|(_, n)| *n)
                .unwrap_or("");
            let busy = self.installing.contains(&major);
            java_list = java_list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(space(3.0))
                    .h(px(56.0))
                    .px(space(4.0))
                    .when(ix > 0, |d| d.border_t_1().border_color(p.border))
                    .child(icon(
                        "coffee",
                        16.0,
                        if path.is_some() {
                            p.accent
                        } else {
                            p.text_muted
                        },
                    ))
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
                                    .child(format!("Java {major}")),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(type_scale::SMALL))
                                    .text_color(p.text_muted)
                                    .child(path.clone().unwrap_or_else(|| note.to_string())),
                            ),
                    )
                    .child(match &path {
                        Some(_) => {
                            state_chip("Installed", p.running, p.running_soft).into_any_element()
                        }
                        None => Button::new(SharedString::from(format!("java-install-{major}")))
                            .icon(if busy {
                                "loader-circle"
                            } else {
                                "arrow-down-to-line"
                            })
                            .label(if busy { "Installing…" } else { "Install" })
                            .small()
                            .disabled(busy)
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                this.install_java(major, window, cx)
                            }))
                            .into_any_element(),
                    }),
            );
        }

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

        div()
            .id("game-settings-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(space(8.0))
                    .px(space(8.0))
                    .py(space(8.0))
                    .max_w(px(760.0))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(space(3.0))
                            .child(section_heading("Java", cx))
                            .child(
                                div()
                                    .text_size(px(type_scale::SMALL))
                                    .text_color(p.text_muted)
                                    .child("Calcite installs the right Java automatically when you play. Install them ahead of time here if you like."),
                            )
                            .child(java_list),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(space(4.0))
                            .child(section_heading("Defaults for every instance", cx))
                            .child(field(
                                "Memory (MB)",
                                Some(format!(
                                    "This computer has {} MB. Instances can override this in their own settings.",
                                    self.system_memory_mb
                                )),
                                div().w(px(200.0)).child(Input::new(&self.memory)).into_any_element(),
                            ))
                            .child(field("Extra JVM arguments", None, Input::new(&self.jvm_args).into_any_element()))
                            .child(field("Environment variables", None, Input::new(&self.env_vars).into_any_element()))
                            .child(field(
                                "Window size",
                                None,
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(space(2.0))
                                    .child(div().w(px(110.0)).child(Input::new(&self.width)))
                                    .child(div().text_color(p.text_muted).child("×"))
                                    .child(div().w(px(110.0)).child(Input::new(&self.height)))
                                    .into_any_element(),
                            ))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(space(3.0))
                                    .child(
                                        Button::new("save-defaults")
                                            .icon("check")
                                            .label("Save defaults")
                                            .primary()
                                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.save_defaults(cx))),
                                    )
                                    .when(self.saved, |d| {
                                        d.child(div().text_size(px(type_scale::SMALL)).text_color(p.running).child("Saved"))
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(section_heading("While you play", cx))
                            .child(toggle_row(
                                "toggle-fullscreen",
                                "Start games fullscreen",
                                "Instances can override this.",
                                settings.force_fullscreen,
                                cx,
                                |s| s.force_fullscreen = !s.force_fullscreen,
                            ))
                            .child(toggle_row(
                                "toggle-hide",
                                "Hide Calcite while the game runs",
                                "It comes back when the game closes.",
                                settings.hide_on_process_start,
                                cx,
                                |s| s.hide_on_process_start = !s.hide_on_process_start,
                            ))
                            .child({
                                let on = cx.global::<crate::prefs::Prefs>().backup_worlds_on_exit;
                                div()
                                    .id("toggle-backups")
                                    .flex()
                                    .items_center()
                                    .gap(space(3.0))
                                    .py(space(2.5))
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                                        crate::prefs::Prefs::set_backup_worlds(!on, cx);
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .flex_1()
                                            .child(div().text_size(px(type_scale::BODY)).text_color(p.text).child("Back up worlds when the game closes"))
                                            .child(div().text_size(px(type_scale::SMALL)).text_color(p.text_muted).child("Only the worlds you played. The last five backups of each are kept.")),
                                    )
                                    .child(
                                        div()
                                            .w(px(32.0))
                                            .h(px(18.0))
                                            .flex_none()
                                            .rounded_full()
                                            .p(px(2.0))
                                            .bg(if on { p.accent } else { p.border_strong })
                                            .flex()
                                            .when(on, |d| d.justify_end())
                                            .child(div().size(px(14.0)).rounded_full().bg(if on { p.on_accent } else { p.text_secondary })),
                                    )
                            })
                            .child(toggle_row(
                                "toggle-discord",
                                "Show what you're playing on Discord",
                                "Uses Discord Rich Presence when Discord is open.",
                                settings.discord_rpc,
                                cx,
                                |s| s.discord_rpc = !s.discord_rpc,
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(space(2.0))
                            .child(icon("shield-check", 14.0, p.running))
                            .child(
                                div()
                                    .text_size(px(type_scale::SMALL))
                                    .text_color(p.text_muted)
                                    .child(format!(
                                        "Telemetry is {}. Calcite never sends usage data of its own.",
                                        if settings.telemetry { "on in the backend" } else { "off" }
                                    )),
                            ),
                    )
                    .child(div().h(px(1.0)).rounded(px(radius::SM))),
            )
            .into_any_element()
    }
}
