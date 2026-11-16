//! Settings → Advanced: Discord status, download limits and the tools for
//! when something is wrong.

use gpui_kit::prelude::*;
use gpui_kit::{ClickEvent, Context, FontWeight, SharedString, Window, div, px};

use crate::backend::Backend;
use crate::state::AppState;
use crate::theme::{PaletteExt, space, type_scale};
use crate::ui::components::{Button, panel, section_heading};

/// Everything Modrinth caches about projects, versions and users. Clearing
/// it only means a slower first load while it fills up again.
const CACHES: [theseus::data::CacheValueType; 18] = {
    use theseus::data::CacheValueType::*;
    [
        Project,
        ProjectV3,
        Version,
        VersionV3,
        User,
        Team,
        Organization,
        File,
        LoaderManifest,
        MinecraftManifest,
        Categories,
        ReportTypes,
        Loaders,
        GameVersions,
        DonationPlatforms,
        FileHash,
        FileUpdate,
        ProjectVersions,
    ]
};

pub struct AdvancedSettings {
    settings: Option<theseus::data::Settings>,
    /// What the last troubleshooting action did, shown under it.
    status: Option<SharedString>,
}

impl AdvancedSettings {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let fetch = Backend::global(cx).call(async { theseus::settings::get().await });
        cx.spawn(async move |this, cx| {
            if let Ok(settings) = fetch.await {
                let _ = this.update(cx, |this, cx| {
                    this.settings = Some(settings);
                    cx.notify();
                });
            }
        })
        .detach();
        AdvancedSettings {
            settings: None,
            status: None,
        }
    }

    fn change(&mut self, edit: impl FnOnce(&mut theseus::data::Settings), cx: &mut Context<Self>) {
        let Some(settings) = &mut self.settings else {
            return;
        };
        edit(settings);
        let next = settings.clone();
        cx.notify();
        let task = Backend::global(cx).call(async move { theseus::settings::set(next).await });
        let state = AppState::global(cx);
        cx.spawn(async move |_, cx| {
            if let Err(error) = task.await {
                state.update(cx, |s, cx| {
                    s.notify_error("Couldn't save that setting", format!("{error:#}"));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn clear_cache(&mut self, cx: &mut Context<Self>) {
        self.status = Some("Clearing…".into());
        cx.notify();
        let task =
            Backend::global(cx).call(async { theseus::cache::purge_cache_types(&CACHES).await });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.status = Some(match result {
                    Ok(()) => {
                        "Cleared. Pages may load a little slower until it fills up again.".into()
                    }
                    Err(error) => format!("Couldn't clear it: {error:#}").into(),
                });
                cx.notify();
            });
        })
        .detach();
    }

    fn open_backups(&mut self, cx: &mut Context<Self>) {
        match theseus::prelude::app_db_backup_dir() {
            Ok(dir) => {
                let _ = std::fs::create_dir_all(&dir);
                cx.open_with_system(&dir);
            }
            Err(error) => {
                self.status = Some(format!("Couldn't find the backups: {error}").into());
                cx.notify();
            }
        }
    }

    /// Versions and paths for a bug report. No accounts, tokens or names.
    fn copy_system_info(&mut self, cx: &mut Context<Self>) {
        let instances = AppState::global(cx).read(cx).instances.len();
        let settings = self.settings.clone();
        let task = Backend::global(cx).call(async { theseus::jre::get_java_versions().await });
        cx.spawn(async move |this, cx| {
            let java = task
                .await
                .map(|v| {
                    let mut list: Vec<String> = v
                        .iter()
                        .map(|j| format!("{} ({})", j.version, j.architecture))
                        .collect();
                    list.sort();
                    list.join(", ")
                })
                .unwrap_or_default();
            let info = format!(
                "Calcite {}\nBackend: Modrinth app-lib {}\nSystem: {} {} ({})\nInstances: {instances}\nJava: {}\nDownloads at once: {}\nMemory default: {} MB",
                calcite_core::version(),
                env!("CALCITE_BACKEND_VERSION"),
                std::env::consts::OS,
                calcite_core::os_version(),
                std::env::consts::ARCH,
                if java.is_empty() { "none installed yet".into() } else { java },
                settings.as_ref().map(|s| s.max_concurrent_downloads).unwrap_or_default(),
                settings.as_ref().map(|s| s.memory.maximum).unwrap_or_default(),
            );
            let _ = this.update(cx, |this, cx| {
                cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(info));
                this.status = Some("Copied. Paste it into your bug report.".into());
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for AdvancedSettings {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();
        let text = |title: &'static str, detail: String| {
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
                        .text_size(px(type_scale::SMALL))
                        .text_color(p.text_muted)
                        .child(detail),
                )
        };
        let row = || {
            div()
                .flex()
                .items_center()
                .gap(space(3.0))
                .px(space(4.0))
                .py(space(3.0))
        };
        let line = || div().h(px(1.0)).bg(p.border);
        let Some(settings) = self.settings.clone() else {
            return div().into_any_element();
        };

        let stepper = |key: &'static str,
                       value: usize,
                       set: fn(&mut theseus::data::Settings, usize),
                       cx: &mut Context<Self>| {
            div()
                .flex()
                .items_center()
                .gap(space(1.0))
                .child(
                    Button::new(SharedString::from(format!("{key}-less")))
                        .icon("minus")
                        .small()
                        .ghost()
                        .disabled(value <= 1)
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.change(|s| set(s, value.saturating_sub(1).max(1)), cx)
                        })),
                )
                .child(
                    div()
                        .w(px(28.0))
                        .flex()
                        .justify_center()
                        .text_size(px(type_scale::BODY))
                        .text_color(p.text)
                        .child(value.to_string()),
                )
                .child(
                    Button::new(SharedString::from(format!("{key}-more")))
                        .icon("plus")
                        .small()
                        .ghost()
                        .disabled(value >= 20)
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.change(|s| set(s, (value + 1).min(20)), cx)
                        })),
                )
        };

        let discord = settings.discord_rpc;
        let presence = panel(cx).overflow_hidden().child(
            row()
                .id("adv-discord")
                .cursor_pointer()
                .hover(|s| s.bg(p.hover))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.change(|s| s.discord_rpc = !discord, cx)
                }))
                .child(text(
                    "Show what you're playing on Discord",
                    "Your Discord status shows the instance you're in while the game runs.".into(),
                ))
                .child(crate::ui::pages::instance::toggle(discord, cx)),
        );

        let downloads = panel(cx)
            .overflow_hidden()
            .child(
                row()
                    .child(text(
                        "Downloads at once",
                        "More is faster on a quick connection. Applies the next time Calcite starts.".into(),
                    ))
                    .child(stepper(
                        "adv-downloads",
                        settings.max_concurrent_downloads,
                        |s, v| s.max_concurrent_downloads = v,
                        cx,
                    )),
            )
            .child(line())
            .child(
                row()
                    .child(text(
                        "Files written at once",
                        "Lower it if installs make your computer stutter. Applies the next time Calcite starts.".into(),
                    ))
                    .child(stepper(
                        "adv-writes",
                        settings.max_concurrent_writes,
                        |s, v| s.max_concurrent_writes = v,
                        cx,
                    )),
            );

        let tools = panel(cx)
            .overflow_hidden()
            .child(
                row()
                    .child(text(
                        "Clear cached Modrinth data",
                        "Project pages, versions and search results Calcite saved. Fixes stale or wrong info.".into(),
                    ))
                    .child(
                        Button::new("adv-clear-cache")
                            .icon("trash")
                            .label("Clear")
                            .small()
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.clear_cache(cx))),
                    ),
            )
            .child(line())
            .child(
                row()
                    .child(text(
                        "Database backups",
                        "Calcite keeps copies of its database so your instances and accounts can be recovered.".into(),
                    ))
                    .child(
                        Button::new("adv-backups")
                            .icon("folder-open")
                            .label("Open folder")
                            .small()
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.open_backups(cx))),
                    ),
            )
            .child(line())
            .child(
                row()
                    .child(text(
                        "System info for a bug report",
                        "Versions of Calcite, your system and Java. Nothing about your accounts.".into(),
                    ))
                    .child(
                        Button::new("adv-sysinfo")
                            .icon("copy")
                            .label("Copy")
                            .small()
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.copy_system_info(cx)
                            })),
                    ),
            );

        div()
            .id("advanced-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                div()
                    .max_w(px(720.0))
                    .px(space(8.0))
                    .py(space(7.0))
                    .flex()
                    .flex_col()
                    .gap(space(8.0))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(space(3.0))
                            .child(section_heading("Discord", cx))
                            .child(presence),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(space(3.0))
                            .child(section_heading("Downloads", cx))
                            .child(downloads),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(space(3.0))
                            .child(section_heading("Troubleshooting", cx))
                            .child(tools)
                            .when_some(self.status.clone(), |d, s| {
                                d.child(
                                    div()
                                        .text_size(px(type_scale::SMALL))
                                        .text_color(p.text_secondary)
                                        .child(s),
                                )
                            }),
                    ),
            )
            .into_any_element()
    }
}
