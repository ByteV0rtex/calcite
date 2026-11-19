//! Hosting: Minecraft servers that run on this computer, for you and
//! friends on the same network. Make one, start it, watch its console.

use std::path::PathBuf;

use calcite_core::local_server::{EULA_URL, Kind, LocalServer, NewServer};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button as KitButton, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, ClickEvent, Context, Entity, FontWeight, SharedString, Subscription,
    UniformListScrollHandle, Window, div, px, uniform_list,
};

use crate::hosting::{Hosting, RunState, STOP_GRACE};
use crate::theme::{PaletteExt, fonts, radius, space, type_scale};
use crate::ui::components::{Button, empty_state, icon, state_chip};

const MEMORY_CHOICES: [u32; 4] = [2048, 4096, 6144, 8192];

pub struct HostingPage {
    hosting: Entity<Hosting>,
    /// The server whose console is open.
    selected: Option<PathBuf>,
    /// The "New server" form, while it's open.
    form: Option<Form>,
    console_input: Option<Entity<InputState>>,
    console_scroll: UniformListScrollHandle,
    /// Lines in the console the last time it was drawn, to follow new ones.
    seen_lines: usize,
    confirm_delete: Option<PathBuf>,
    _subscriptions: Vec<Subscription>,
}

struct Form {
    name: Entity<InputState>,
    kind: Kind,
    version: Option<String>,
    memory_mb: u32,
}

impl HostingPage {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let hosting = Hosting::global(cx);
        hosting.update(cx, |h, cx| h.reload(cx));
        let subscriptions = vec![cx.observe(&hosting, |this: &mut Self, hosting, cx| {
            // A server that just appeared (made from the form) gets opened.
            let h = hosting.read(cx);
            if this
                .selected
                .as_ref()
                .is_none_or(|dir| h.server(dir).is_none())
            {
                this.selected = h.servers.first().map(|s| s.dir.clone());
            }
            cx.notify();
        })];
        let selected = hosting.read(cx).servers.first().map(|s| s.dir.clone());
        HostingPage {
            hosting,
            selected,
            form: None,
            console_input: None,
            console_scroll: UniformListScrollHandle::new(),
            seen_lines: 0,
            confirm_delete: None,
            _subscriptions: subscriptions,
        }
    }

    fn open_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.hosting.read(cx).servers.len();
        let name = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder("Server name");
            state.set_value(
                if count == 0 {
                    "My server".to_string()
                } else {
                    format!("My server {}", count + 1)
                },
                window,
                cx,
            );
            state
        });
        name.update(cx, |s, cx| s.focus(window, cx));
        self.hosting.update(cx, |h, cx| h.load_versions(cx));
        self.form = Some(Form {
            name,
            kind: Kind::Fabric,
            version: None,
            memory_mb: 4096,
        });
        cx.notify();
    }

    fn create(&mut self, cx: &mut Context<Self>) {
        let Some(form) = &self.form else { return };
        let versions = self.hosting.read(cx).versions.clone().unwrap_or_default();
        let Some(version) = form.version.clone().or_else(|| versions.first().cloned()) else {
            return;
        };
        let new = NewServer {
            name: form.name.read(cx).value().to_string(),
            kind: form.kind,
            game_version: version,
            memory_mb: form.memory_mb,
        };
        self.hosting.update(cx, |h, cx| h.create(new, cx));
        self.form = None;
        cx.notify();
    }

    fn make_console_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.console_input.is_some() {
            return;
        }
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Type a command, like say hello or op Steve")
        });
        self._subscriptions.push(cx.subscribe_in(
            &input,
            window,
            |this, input, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = event
                    && let Some(dir) = this.selected.clone()
                {
                    let command = input.read(cx).value().to_string();
                    this.hosting.update(cx, |h, cx| h.send(&dir, &command, cx));
                    input.update(cx, |s, cx| s.set_value("", window, cx));
                }
            },
        ));
        self.console_input = Some(input);
    }

    fn server_row(&self, ix: usize, server: &LocalServer, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette().clone();
        let hosting = self.hosting.read(cx);
        let running = hosting.running.get(&server.dir);
        let selected = self.selected.as_ref() == Some(&server.dir);
        let (label, fg, bg) = match running.map(|r| r.state) {
            Some(RunState::Online) => {
                let players = running.map(|r| r.players.len()).unwrap_or(0);
                (
                    if players == 0 {
                        "Online".to_string()
                    } else {
                        format!("Online · {players}")
                    },
                    p.running,
                    p.running_soft,
                )
            }
            Some(RunState::Starting) => ("Starting".into(), p.info, p.info_soft),
            Some(RunState::Stopping) => ("Stopping".into(), p.warning, p.warning_soft),
            None => ("Offline".into(), p.text_muted, p.raised),
        };
        let dir = server.dir.clone();
        div()
            .id(SharedString::from(format!("hosted-{ix}")))
            .flex()
            .items_center()
            .gap(space(3.0))
            .p(space(3.0))
            .rounded(px(radius::MD))
            .cursor_pointer()
            .when(selected, |d| d.bg(p.raised))
            .when(!selected, |d| d.hover(|s| s.bg(p.hover)))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.selected = Some(dir.clone());
                this.confirm_delete = None;
                this.seen_lines = 0;
                cx.notify();
            }))
            .child(
                div()
                    .flex_none()
                    .size(px(36.0))
                    .rounded(px(radius::MD))
                    .bg(p.accent_soft)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("server", 18.0, p.accent_text)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .truncate()
                            .text_size(px(type_scale::BODY))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(p.text)
                            .child(server.config.name.clone()),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(px(type_scale::CAPTION))
                            .text_color(p.text_muted)
                            .child(format!(
                                "{} {}",
                                server.config.kind.label(),
                                server.config.game_version
                            )),
                    ),
            )
            .child(state_chip(label, fg, bg))
            .into_any_element()
    }

    fn form_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette().clone();
        let Some(form) = &self.form else {
            return div().into_any_element();
        };
        let versions = self.hosting.read(cx).versions.clone();
        let version_label: SharedString = match (&form.version, &versions) {
            (Some(v), _) => v.clone().into(),
            (None, Some(list)) if !list.is_empty() => format!("{} (newest)", list[0]).into(),
            (None, Some(_)) => "Couldn't load versions".into(),
            (None, None) => "Loading versions…".into(),
        };

        let mut kinds = div().flex().gap(space(2.0));
        for kind in Kind::ALL {
            let active = form.kind == kind;
            kinds = kinds.child(
                div()
                    .id(SharedString::from(format!("host-kind-{}", kind.label())))
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(space(1.0))
                    .p(space(3.0))
                    .rounded(px(radius::MD))
                    .border_1()
                    .cursor_pointer()
                    .when(active, |d| d.border_color(p.accent).bg(p.accent_soft))
                    .when(!active, |d| {
                        d.border_color(p.border).hover(|s| s.bg(p.hover))
                    })
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        if let Some(form) = &mut this.form {
                            form.kind = kind;
                        }
                        cx.notify();
                    }))
                    .child(
                        div()
                            .text_size(px(type_scale::BODY))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.text)
                            .child(kind.label()),
                    )
                    .child(
                        div()
                            .text_size(px(type_scale::CAPTION))
                            .text_color(p.text_muted)
                            .child(kind.blurb()),
                    ),
            );
        }

        let version_menu = {
            let list = versions.clone().unwrap_or_default();
            let entity = cx.entity().downgrade();
            KitButton::new("host-version")
                .outline()
                .small()
                .label(version_label)
                .dropdown_menu(move |mut menu, _, _| {
                    for version in list.iter().take(60) {
                        let entity = entity.clone();
                        let version = version.clone();
                        menu = menu.item(PopupMenuItem::new(version.clone()).on_click(
                            move |_, _, cx| {
                                let _ = entity.update(cx, |this, cx| {
                                    if let Some(form) = &mut this.form {
                                        form.version = Some(version.clone());
                                    }
                                    cx.notify();
                                });
                            },
                        ));
                    }
                    menu
                })
        };

        let mut memory = div().flex().gap(space(1.0));
        for mb in MEMORY_CHOICES {
            let active = form.memory_mb == mb;
            memory = memory.child(
                div()
                    .id(SharedString::from(format!("host-mem-{mb}")))
                    .h(px(28.0))
                    .px(space(2.5))
                    .flex()
                    .items_center()
                    .rounded(px(radius::SM))
                    .text_size(px(type_scale::SMALL))
                    .cursor_pointer()
                    .when(active, |d| d.bg(p.raised).text_color(p.text))
                    .when(!active, |d| {
                        d.text_color(p.text_secondary)
                            .hover(|s| s.text_color(p.text))
                    })
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        if let Some(form) = &mut this.form {
                            form.memory_mb = mb;
                        }
                        cx.notify();
                    }))
                    .child(format!("{} GB", mb / 1024)),
            );
        }

        let field = |label: &'static str, control: AnyElement| {
            div()
                .flex()
                .items_center()
                .gap(space(4.0))
                .child(
                    div()
                        .w(px(96.0))
                        .flex_none()
                        .text_size(px(type_scale::SMALL))
                        .text_color(p.text_secondary)
                        .child(label),
                )
                .child(div().flex_1().child(control))
        };

        div()
            .flex()
            .flex_col()
            .gap(space(4.0))
            .p(space(5.0))
            .rounded(px(radius::LG))
            .bg(p.surface)
            .border_1()
            .border_color(p.border)
            .child(
                div()
                    .text_size(px(type_scale::SECTION))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(p.text)
                    .child("New server"),
            )
            .child(field("Name", Input::new(&form.name).into_any_element()))
            .child(kinds)
            .child(field("Version", version_menu.into_any_element()))
            .child(field("Memory", memory.into_any_element()))
            .child(
                div()
                    .flex()
                    .gap(space(2.0))
                    .justify_end()
                    .child(Button::new("host-cancel").label("Cancel").ghost().on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.form = None;
                            cx.notify();
                        }),
                    ))
                    .child(
                        Button::new("host-create")
                            .icon("plus")
                            .label("Make server")
                            .primary()
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.create(cx))),
                    ),
            )
            .into_any_element()
    }

    fn detail(&mut self, server: &LocalServer, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette().clone();
        let hosting = self.hosting.read(cx);
        let running = hosting.running.get(&server.dir);
        let state = running.map(|r| r.state);
        let eula = server.eula_accepted();
        let last_exit = hosting.last_exit.get(&server.dir).cloned();
        let players = running.map(|r| r.players.clone()).unwrap_or_default();
        let uptime = running
            .filter(|r| r.state == RunState::Online)
            .map(|r| crate::ui::components::short_duration(r.since.elapsed().as_secs()));
        let lines: Vec<SharedString> = running
            .map(|r| r.lines.iter().cloned().collect())
            .unwrap_or_default();
        let stuck = running
            .and_then(|r| r.stop_asked)
            .is_some_and(|t| t.elapsed() > STOP_GRACE);
        let dir = server.dir.clone();

        // Follow new console lines.
        if lines.len() != self.seen_lines {
            self.seen_lines = lines.len();
            if !lines.is_empty() {
                self.console_scroll
                    .scroll_to_item(lines.len() - 1, gpui_kit::ScrollStrategy::Top);
            }
        }

        let main_button = match state {
            None => {
                let dir = dir.clone();
                Button::new("host-start")
                    .icon("play")
                    .label("Start")
                    .primary()
                    .disabled(!eula)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.hosting.update(cx, |h, cx| h.start(&dir, cx))
                    }))
            }
            Some(RunState::Stopping) if stuck => {
                let dir = dir.clone();
                Button::new("host-kill")
                    .icon("power")
                    .label("Force stop")
                    .danger()
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.hosting.update(cx, |h, cx| h.kill(&dir, cx))
                    }))
            }
            Some(RunState::Stopping) => Button::new("host-stopping")
                .icon("loader-circle")
                .label("Stopping…")
                .disabled(true),
            Some(_) => {
                let dir = dir.clone();
                Button::new("host-stop")
                    .icon("square")
                    .label("Stop")
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.hosting.update(cx, |h, cx| h.stop(&dir, cx))
                    }))
            }
        };

        let address = server.address();
        let more = {
            let dir = dir.clone();
            let address = address.clone();
            let entity = cx.entity().downgrade();
            let memory_mb = server.config.memory_mb;
            let can_add_voice = server.config.kind != Kind::Vanilla && !server.has_voice_chat();
            KitButton::new("host-more")
                .ghost()
                .small()
                .child(icon("ellipsis", 14.0, p.text_secondary))
                .dropdown_menu(move |mut menu, _, _| {
                    let (open, copy) = (dir.clone(), address.clone());
                    menu = menu
                        .item(
                            PopupMenuItem::new("Copy address").on_click(move |_, _, cx| {
                                cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(
                                    copy.clone(),
                                ))
                            }),
                        )
                        .item(
                            PopupMenuItem::new("Open folder")
                                .on_click(move |_, _, cx| cx.open_with_system(&open)),
                        );
                    if can_add_voice {
                        let (entity, dir) = (entity.clone(), dir.clone());
                        menu = menu.item(PopupMenuItem::new("Add Simple Voice Chat").on_click(
                            move |_, _, cx| {
                                let _ = entity.update(cx, |this, cx| {
                                    this.hosting.update(cx, |h, cx| h.add_voice_chat(&dir, cx))
                                });
                            },
                        ));
                    }
                    menu = menu.separator().label("Memory");
                    for mb in MEMORY_CHOICES {
                        let (entity, dir) = (entity.clone(), dir.clone());
                        menu = menu.item(
                            PopupMenuItem::new(format!(
                                "{} GB{}",
                                mb / 1024,
                                if mb == memory_mb { "  ✓" } else { "" }
                            ))
                            .on_click(move |_, _, cx| {
                                let _ = entity.update(cx, |this, cx| {
                                    this.hosting.update(cx, |h, cx| h.set_memory(&dir, mb, cx))
                                });
                            }),
                        );
                    }
                    let (entity, dir) = (entity.clone(), dir.clone());
                    menu.separator()
                        .item(
                            PopupMenuItem::new("Delete server…").on_click(move |_, _, cx| {
                                let _ = entity.update(cx, |this, cx| {
                                    this.confirm_delete = Some(dir.clone());
                                    cx.notify();
                                });
                            }),
                        )
                })
        };

        let header = div()
            .flex()
            .items_center()
            .gap(space(3.0))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(space(0.5))
                    .child(
                        div()
                            .truncate()
                            .text_size(px(type_scale::SECTION))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.text)
                            .child(server.config.name.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(type_scale::SMALL))
                            .text_color(p.text_muted)
                            .child(format!(
                                "{} {}{} · {} GB · join at {address}{}{}",
                                server.config.kind.label(),
                                server.config.game_version,
                                server
                                    .config
                                    .loader_version
                                    .as_ref()
                                    .filter(|_| server.config.kind == Kind::Fabric)
                                    .map(|l| format!(" (loader {l})"))
                                    .unwrap_or_default(),
                                server.config.memory_mb / 1024,
                                server
                                    .lan_address()
                                    .map(|lan| format!(", friends on your network at {lan}"))
                                    .unwrap_or_default(),
                                if server.has_voice_chat() {
                                    " · voice chat on"
                                } else {
                                    ""
                                },
                            )),
                    ),
            )
            .child(more)
            .child(main_button);

        let delete_bar = (self.confirm_delete.as_ref() == Some(&dir)).then(|| {
            let dir = dir.clone();
            div()
                .flex()
                .items_center()
                .gap(space(3.0))
                .p(space(3.0))
                .rounded(px(radius::MD))
                .bg(p.danger_soft)
                .child(
                    div()
                        .flex_1()
                        .text_size(px(type_scale::SMALL))
                        .text_color(p.text)
                        .child(if state.is_some() {
                            "Stop the server before deleting it."
                        } else {
                            "Delete this server and its world for good? This can't be undone."
                        }),
                )
                .child(
                    Button::new("host-delete-cancel")
                        .label("Keep it")
                        .small()
                        .ghost()
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.confirm_delete = None;
                            cx.notify();
                        })),
                )
                .when(state.is_none(), |d| {
                    d.child(
                        Button::new("host-delete")
                            .label("Delete")
                            .small()
                            .danger()
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.confirm_delete = None;
                                this.hosting.update(cx, |h, cx| h.delete(&dir, cx));
                            })),
                    )
                })
        });

        // Mojang's EULA has to be accepted by the player, in their own click.
        let eula_card = (!eula).then(|| {
            let dir = dir.clone();
            div()
                .flex()
                .items_center()
                .gap(space(3.0))
                .p(space(4.0))
                .rounded(px(radius::MD))
                .bg(p.surface)
                .border_1()
                .border_color(p.warning_soft)
                .child(icon("file-text", 18.0, p.warning))
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap(space(0.5))
                        .child(
                            div()
                                .text_size(px(type_scale::BODY))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.text)
                                .child("Accept the Minecraft EULA to start"),
                        )
                        .child(
                            div()
                                .text_size(px(type_scale::SMALL))
                                .text_color(p.text_muted)
                                .child("Every Minecraft server needs it. Read it first, then agree if you do."),
                        ),
                )
                .child(
                    Button::new("host-eula-read")
                        .label("Read the EULA")
                        .small()
                        .ghost()
                        .on_click(|_, _, cx| cx.open_url(EULA_URL)),
                )
                .child(
                    Button::new("host-eula-agree")
                        .label("I agree")
                        .small()
                        .primary()
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.hosting.update(cx, |h, cx| h.accept_eula(&dir, cx))
                        })),
                )
        });

        let voice = hosting
            .voice_status
            .get(&server.dir)
            .cloned()
            .map(|(message, is_error)| {
                div()
                    .p(space(3.0))
                    .rounded(px(radius::MD))
                    .bg(if is_error { p.danger_soft } else { p.surface })
                    .border_1()
                    .border_color(if is_error { p.danger_soft } else { p.border })
                    .text_size(px(type_scale::SMALL))
                    .text_color(p.text)
                    .child(message)
            });
        let notice = last_exit.filter(|_| state.is_none()).map(|message| {
            div()
                .p(space(3.0))
                .rounded(px(radius::MD))
                .bg(p.danger_soft)
                .text_size(px(type_scale::SMALL))
                .text_color(p.text)
                .child(message)
        });

        let players_row = uptime.map(|uptime| {
            let mut row = div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(space(1.5))
                .child(
                    div()
                        .text_size(px(type_scale::SMALL))
                        .text_color(p.text_muted)
                        .child(if players.is_empty() {
                            format!("Up for {uptime} · nobody playing yet")
                        } else {
                            format!("Up for {uptime} · playing now:")
                        }),
                );
            for name in players {
                row = row.child(state_chip(name, p.text, p.raised));
            }
            row
        });

        let console: AnyElement = if lines.is_empty() {
            empty_state(
                "square-terminal",
                "The console shows up here",
                if eula {
                    "Start the server to see what it says and to type commands into it."
                } else {
                    "Accept the EULA above, then start the server."
                },
                None,
                cx,
            )
            .size_full()
            .into_any_element()
        } else {
            uniform_list("host-console", lines.len(), move |range, _, cx| {
                let p = cx.palette().clone();
                range
                    .map(|ix| {
                        let line = &lines[ix];
                        let color = if line.contains("ERROR") || line.contains("Exception") {
                            p.danger
                        } else if line.contains("WARN") {
                            p.warning
                        } else if line.starts_with("> ") {
                            p.accent_text
                        } else {
                            p.text_secondary
                        };
                        div()
                            .id(ix)
                            .h(px(18.0))
                            .px(space(2.0))
                            .font_family(fonts::MONO)
                            .text_size(px(12.0))
                            .truncate()
                            .text_color(color)
                            .child(line.clone())
                    })
                    .collect()
            })
            .track_scroll(&self.console_scroll)
            .size_full()
            .into_any_element()
        };

        let can_type = matches!(state, Some(RunState::Online | RunState::Starting));
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(space(3.0))
            .child(header)
            .children(delete_bar)
            .children(eula_card)
            .children(voice)
            .children(notice)
            .children(players_row)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .py(space(2.0))
                    .rounded(px(radius::MD))
                    .bg(p.surface)
                    .border_1()
                    .border_color(p.border)
                    .child(console),
            )
            .when(can_type, |d| {
                d.children(self.console_input.as_ref().map(|input| {
                    Input::new(input).prefix(icon("chevron-right", 14.0, p.text_muted))
                }))
            })
            .into_any_element()
    }
}

impl Render for HostingPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.make_console_input(window, cx);
        let p = cx.palette().clone();
        let hosting = self.hosting.read(cx);
        let servers = hosting.servers.clone();
        let creating = hosting.creating.clone();
        let create_error = hosting.create_error.clone();

        let header = div()
            .flex()
            .items_center()
            .gap(space(3.0))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(space(1.0))
                    .child(
                        div()
                            .text_size(px(type_scale::TITLE))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.text)
                            .child("Hosting"),
                    )
                    .child(
                        div()
                            .text_size(px(type_scale::BODY))
                            .text_color(p.text_secondary)
                            .child("Run a server on this computer for you and friends on the same network."),
                    ),
            )
            .when(
                self.form.is_none() && creating.is_none() && !servers.is_empty(),
                |d| {
                d.child(
                    Button::new("host-new")
                        .icon("plus")
                        .label("New server")
                        .primary()
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.open_form(window, cx)
                        })),
                )
                },
            );

        let status = creating.map(|line| {
            div()
                .flex()
                .items_center()
                .gap(space(2.0))
                .p(space(3.0))
                .rounded(px(radius::MD))
                .bg(p.surface)
                .border_1()
                .border_color(p.border)
                .text_size(px(type_scale::SMALL))
                .text_color(p.text_secondary)
                .child(icon("loader-circle", 14.0, p.accent))
                .child(format!("Making the server: {line}…"))
        });
        let error = create_error.map(|message| {
            div()
                .p(space(3.0))
                .rounded(px(radius::MD))
                .bg(p.danger_soft)
                .text_size(px(type_scale::SMALL))
                .text_color(p.text)
                .child(format!("Couldn't make the server: {message}"))
        });

        let making_first = servers.is_empty() && status.is_some();
        let body: AnyElement = if making_first {
            div().into_any_element()
        } else if servers.is_empty() && self.form.is_none() {
            empty_state(
                "server",
                "No servers yet",
                "Make one in a minute: pick Vanilla, Fabric or Paper and a version. It runs right here, and friends on your network can join.",
                Some(
                    Button::new("host-new-empty")
                        .icon("plus")
                        .label("New server")
                        .primary()
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.open_form(window, cx)
                        }))
                        .into_any_element(),
                ),
                cx,
            )
            .into_any_element()
        } else {
            let mut list = div().flex().flex_col().gap(space(1.0));
            for (ix, server) in servers.iter().enumerate() {
                list = list.child(self.server_row(ix, server, cx));
            }
            let selected = self
                .selected
                .as_ref()
                .and_then(|dir| servers.iter().find(|s| &s.dir == dir))
                .cloned();
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .gap(space(5.0))
                .child(
                    div()
                        .id("hosted-list")
                        .w(px(280.0))
                        .flex_none()
                        .overflow_y_scroll()
                        .child(list),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .children(selected.map(|server| self.detail(&server, cx))),
                )
                .into_any_element()
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(space(4.0))
            .px(space(8.0))
            .pt(space(7.0))
            .pb(space(6.0))
            .child(header)
            .children(status)
            .children(error)
            .when(self.form.is_some(), |d| d.child(self.form_view(cx)))
            .child(body)
    }
}
