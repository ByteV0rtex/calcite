//! ⌘K / Ctrl+K: jump anywhere and do common things by typing.

use gpui_kit::component::Sizable as _;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{
    ClickEvent, Context, Entity, EventEmitter, FontWeight, KeyDownEvent, SharedString, Window, div,
    px,
};

use crate::state::AppState;
use crate::theme::{PaletteExt, radius, space, type_scale};
use crate::ui::components::icon;
use crate::ui::pages::settings::Section;
use crate::ui::shell::Route;

/// What picking an entry does; the shell carries it out.
#[derive(Clone, Debug)]
pub enum PaletteCommand {
    Go(Route),
    Play(String),
    OpenFolder(String),
    NewInstance,
    FindServer,
    SearchModrinth(String),
    OpenDataFolder,
}

pub enum PaletteEvent {
    Close,
    Run(PaletteCommand),
}

#[derive(Clone)]
struct Entry {
    icon: &'static str,
    label: SharedString,
    detail: Option<SharedString>,
    command: PaletteCommand,
}

/// Most entries listed at once.
const SHOWN: usize = 40;

pub struct CommandPalette {
    input: Entity<InputState>,
    selected: usize,
    scroll: gpui_kit::ScrollHandle,
}

impl EventEmitter<PaletteEvent> for CommandPalette {}

impl CommandPalette {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Play, open or go to anything…"));
        input.update(cx, |s, cx| s.focus(window, cx));
        cx.subscribe(&input, |this: &mut Self, _, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                this.selected = 0;
                cx.notify();
            }
        })
        .detach();
        CommandPalette {
            input,
            selected: 0,
            scroll: gpui_kit::ScrollHandle::new(),
        }
    }

    /// Dev hook: type a query.
    #[cfg_attr(not(feature = "snapshot"), allow(dead_code))]
    pub fn set_query(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let text = text.to_string();
        self.input.update(cx, |s, cx| s.set_value(text, window, cx));
        self.selected = 0;
        cx.notify();
    }

    fn entries(&self, cx: &App) -> Vec<Entry> {
        let query = self.input.read(cx).value().trim().to_string();
        let mut instances = AppState::global(cx).read(cx).instances.clone();
        instances.sort_by_key(|i| std::cmp::Reverse(i.last_played));

        let mut all: Vec<Entry> = Vec::new();
        for inst in &instances {
            let detail: SharedString = inst.loader_line().into();
            all.push(Entry {
                icon: "play",
                label: format!("Play {}", inst.name).into(),
                detail: Some(detail.clone()),
                command: PaletteCommand::Play(inst.id.clone()),
            });
            all.push(Entry {
                icon: "box",
                label: format!("Open {}", inst.name).into(),
                detail: Some(detail.clone()),
                command: PaletteCommand::Go(Route::Instance(inst.id.clone())),
            });
            all.push(Entry {
                icon: "folder-open",
                label: format!("Show {} in folder", inst.name).into(),
                detail: Some(detail),
                command: PaletteCommand::OpenFolder(inst.id.clone()),
            });
        }
        let page = |icon: &'static str, label: &'static str, route: Route| Entry {
            icon,
            label: label.into(),
            detail: Some("Go to".into()),
            command: PaletteCommand::Go(route),
        };
        let pages = [
            page("house", "Home", Route::Home),
            page("library-big", "Library", Route::Library),
            page("compass", "Discover", Route::Discover),
            page("shirt", "Skins", Route::Skins),
            page("image", "Screenshots", Route::Screenshots),
            page("server", "Hosting", Route::Hosting),
            page(
                "sun",
                "Settings · Appearance",
                Route::Settings(Section::Appearance),
            ),
            page(
                "gamepad-2",
                "Settings · Game",
                Route::Settings(Section::Game),
            ),
            page(
                "hard-drive",
                "Settings · Storage",
                Route::Settings(Section::Storage),
            ),
            page(
                "arrow-down-to-line",
                "Settings · Move in",
                Route::Settings(Section::MoveIn),
            ),
            page(
                "user",
                "Settings · Accounts",
                Route::Settings(Section::Accounts),
            ),
            page(
                "sliders-horizontal",
                "Settings · Advanced",
                Route::Settings(Section::Advanced),
            ),
            page("info", "About Calcite", Route::Settings(Section::About)),
        ];
        let actions = [
            Entry {
                icon: "plus",
                label: "New instance…".into(),
                detail: None,
                command: PaletteCommand::NewInstance,
            },
            Entry {
                icon: "server",
                label: "Find a server".into(),
                detail: None,
                command: PaletteCommand::FindServer,
            },
            Entry {
                icon: "folder-open",
                label: "Open Calcite's data folder".into(),
                detail: None,
                command: PaletteCommand::OpenDataFolder,
            },
        ];

        if query.is_empty() {
            // Recently played first, then where to go and what to do.
            let mut shown: Vec<Entry> = all
                .iter()
                .filter(|e| matches!(e.command, PaletteCommand::Play(_)))
                .take(5)
                .cloned()
                .collect();
            shown.extend(actions);
            shown.extend(pages);
            return shown;
        }
        all.extend(pages);
        all.extend(actions);
        let mut scored: Vec<(i32, Entry)> = all
            .into_iter()
            .filter_map(|e| calcite_core::fuzzy::score(&query, &e.label).map(|s| (s, e)))
            .collect();
        scored.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
        let mut shown: Vec<Entry> = scored.into_iter().take(SHOWN).map(|(_, e)| e).collect();
        shown.push(Entry {
            icon: "search",
            label: format!("Search Modrinth for \u{201c}{query}\u{201d}").into(),
            detail: None,
            command: PaletteCommand::SearchModrinth(query),
        });
        shown
    }

    fn run(&mut self, command: PaletteCommand, cx: &mut Context<Self>) {
        cx.emit(PaletteEvent::Run(command));
        cx.emit(PaletteEvent::Close);
    }

    fn key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let count = self.entries(cx).len();
        match event.keystroke.key.as_str() {
            "up" => {
                self.selected = self
                    .selected
                    .checked_sub(1)
                    .unwrap_or(count.saturating_sub(1));
            }
            "down" => {
                self.selected = if self.selected + 1 >= count {
                    0
                } else {
                    self.selected + 1
                }
            }
            "enter" => {
                if let Some(entry) = self.entries(cx).get(self.selected).cloned() {
                    self.run(entry.command, cx);
                }
            }
            "escape" => cx.emit(PaletteEvent::Close),
            _ => return,
        }
        self.scroll.scroll_to_item(self.selected);
        cx.stop_propagation();
        cx.notify();
    }
}

use gpui_kit::App;

impl Render for CommandPalette {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette().clone();
        let entries = self.entries(cx);
        let selected = self.selected.min(entries.len().saturating_sub(1));

        let mut list = div()
            .id("palette-list")
            .flex()
            .flex_col()
            .max_h(px(380.0))
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .p(space(1.5));
        for (ix, entry) in entries.into_iter().enumerate() {
            let active = ix == selected;
            let command = entry.command.clone();
            list =
                list.child(
                    div()
                        .id(SharedString::from(format!("palette-{ix}")))
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(space(3.0))
                        .h(px(38.0))
                        .px(space(3.0))
                        .rounded(px(radius::SM))
                        .cursor_pointer()
                        .when(active, |d| d.bg(p.hover))
                        .on_mouse_move(cx.listener(
                            move |this, _: &gpui_kit::MouseMoveEvent, _, cx| {
                                if this.selected != ix {
                                    this.selected = ix;
                                    cx.notify();
                                }
                            },
                        ))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.run(command.clone(), cx)
                        }))
                        .child(icon(
                            entry.icon,
                            15.0,
                            if active { p.accent } else { p.text_muted },
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(type_scale::BODY))
                                .font_weight(if active {
                                    FontWeight::MEDIUM
                                } else {
                                    FontWeight::NORMAL
                                })
                                .text_color(p.text)
                                .child(entry.label),
                        )
                        .children(entry.detail.map(|d| {
                            div()
                                .flex_none()
                                .text_size(px(type_scale::SMALL))
                                .text_color(p.text_muted)
                                .child(d)
                        })),
                );
        }

        div()
            .w(px(620.0))
            .flex()
            .flex_col()
            .rounded(px(radius::XL))
            .bg(p.overlay)
            .border_1()
            .border_color(p.border_strong)
            .shadow_lg()
            .overflow_hidden()
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| this.key(event, cx)))
            .child(
                div()
                    .p(space(2.0))
                    .border_b_1()
                    .border_color(p.border)
                    .child(
                        Input::new(&self.input)
                            .prefix(icon("search", 15.0, p.text_muted))
                            .appearance(false)
                            .large(),
                    ),
            )
            .child(list)
            .child(
                div()
                    .flex()
                    .gap(space(4.0))
                    .px(space(4.0))
                    .py(space(2.0))
                    .border_t_1()
                    .border_color(p.border)
                    .text_size(px(type_scale::CAPTION))
                    .text_color(p.text_muted)
                    .child("↑ ↓ move")
                    .child("Enter run")
                    .child("Esc close"),
            )
    }
}
