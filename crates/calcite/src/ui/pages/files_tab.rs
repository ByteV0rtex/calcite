//! Instance › Files: browse the instance folder, edit config and text files
//! in place, rename, delete and make folders.

use gpui_kit::component::Sizable as _;
use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, ClickEvent, Context, Entity, FontWeight, SharedString, Window, div, px,
};
use theseus::instance::InstanceFileItem;

use crate::backend::Backend;
use crate::state::AppState;
use crate::theme::{PaletteExt, fonts, radius, space, type_scale};
use crate::ui::components::{Button, empty_state, icon, relative_time, skeleton};

/// Files Calcite opens in its own editor (anything else opens in the
/// app the system picks).
const TEXT: [&str; 14] = [
    "txt",
    "json",
    "json5",
    "toml",
    "properties",
    "cfg",
    "conf",
    "yml",
    "yaml",
    "ini",
    "log",
    "md",
    "mcmeta",
    "snbt",
];
/// Bigger files open outside Calcite.
const EDIT_LIMIT: u64 = 1024 * 1024;

fn editable(item: &InstanceFileItem) -> bool {
    item.kind == "file"
        && !item.read_only
        && item.size.is_some_and(|s| s <= EDIT_LIMIT)
        && item
            .name
            .rsplit_once('.')
            .is_some_and(|(_, ext)| TEXT.contains(&ext.to_ascii_lowercase().as_str()))
}

fn human(bytes: u64) -> String {
    calcite_core::transfer::human_bytes(bytes)
}

enum Editing {
    /// Loaded text waiting for the window to build an editor.
    Loaded { path: String, text: String },
    Open {
        path: String,
        input: Entity<TextareaState>,
        saved: bool,
    },
}

pub struct FilesBrowser {
    instance_id: String,
    dir: String,
    items: Option<Result<Vec<InstanceFileItem>, String>>,
    editing: Option<Editing>,
    renaming: Option<(String, Entity<InputState>)>,
    new_folder: Option<Entity<InputState>>,
    pending_rename: Option<String>,
    pending_folder: bool,
    confirm_delete: Option<String>,
}

impl FilesBrowser {
    pub fn new(instance_id: String, cx: &mut Context<Self>) -> Self {
        let mut browser = FilesBrowser {
            instance_id,
            dir: String::new(),
            items: None,
            editing: None,
            renaming: None,
            new_folder: None,
            pending_rename: None,
            pending_folder: false,
            confirm_delete: None,
        };
        browser.load(cx);
        browser
    }

    pub fn load(&mut self, cx: &mut Context<Self>) {
        let (id, dir) = (self.instance_id.clone(), self.dir.clone());
        let task = Backend::global(cx)
            .call(async move { theseus::instance::list_instance_files(&id, &dir).await });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.items = Some(
                    result
                        .map(|mut items| {
                            items.sort_by(|a, b| {
                                (b.kind == "directory")
                                    .cmp(&(a.kind == "directory"))
                                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                            });
                            items
                        })
                        .map_err(|e| format!("{e:#}")),
                );
                cx.notify();
            });
        })
        .detach();
    }

    fn go(&mut self, dir: String, cx: &mut Context<Self>) {
        self.dir = dir;
        self.items = None;
        self.confirm_delete = None;
        self.renaming = None;
        self.load(cx);
        cx.notify();
    }

    fn fail(&self, title: &'static str, error: impl std::fmt::Display, cx: &mut Context<Self>) {
        let error = error.to_string();
        AppState::global(cx).update(cx, |s, cx| {
            s.notify_error(title, error);
            cx.notify();
        });
    }

    /// Run a file change, then re-list the folder.
    fn change(
        &mut self,
        task: impl Future<Output = anyhow::Result<()>> + 'static,
        failure: &'static str,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = result {
                    this.fail(failure, format!("{error:#}"), cx);
                }
                this.load(cx);
            });
        })
        .detach();
    }

    fn open(&mut self, path: String, directory: bool, edit: bool, cx: &mut Context<Self>) {
        if directory {
            self.go(path, cx);
            return;
        }
        if edit {
            let id = self.instance_id.clone();
            let read_path = path.clone();
            let task = Backend::global(cx)
                .call(async move { theseus::instance::read_instance_file(&id, &read_path).await });
            cx.spawn(async move |this, cx| {
                let result = task.await;
                let _ = this.update(cx, |this, cx| match result {
                    Ok(bytes) => {
                        this.editing = Some(Editing::Loaded {
                            path,
                            text: String::from_utf8_lossy(&bytes).into_owned(),
                        });
                        cx.notify();
                    }
                    Err(error) => this.fail("Couldn't open that file", format!("{error:#}"), cx),
                });
            })
            .detach();
            return;
        }
        // Everything else opens in the app the system picks.
        let id = self.instance_id.clone();
        let task =
            Backend::global(cx).call(async move { theseus::instance::get_full_path(&id).await });
        cx.spawn(async move |_, cx| {
            if let Ok(root) = task.await {
                cx.update(|cx| cx.open_with_system(&root.join(&path)));
            }
        })
        .detach();
    }

    /// Dev hook: open a file by path (for snapshots).
    #[cfg_attr(not(feature = "snapshot"), allow(dead_code))]
    pub(crate) fn open_path(&mut self, path: String, cx: &mut Context<Self>) {
        self.open(path, false, true, cx);
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let Some(Editing::Open { path, input, .. }) = &self.editing else {
            return;
        };
        let (id, path, text) = (
            self.instance_id.clone(),
            path.clone(),
            input.read(cx).value().to_string(),
        );
        let task = Backend::global(cx).call(async move {
            theseus::instance::write_instance_file(&id, &path, text.as_bytes(), false).await
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        if let Some(Editing::Open { saved, .. }) = &mut this.editing {
                            *saved = true;
                        }
                    }
                    Err(error) => this.fail("Couldn't save that file", format!("{error:#}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn delete(&mut self, path: String, cx: &mut Context<Self>) {
        self.confirm_delete = None;
        let id = self.instance_id.clone();
        let task = Backend::global(cx)
            .call(async move { theseus::instance::delete_instance_file(&id, &path, true).await });
        self.change(task, "Couldn't delete that", cx);
    }

    fn finish_rename(&mut self, cx: &mut Context<Self>) {
        let Some((from, input)) = self.renaming.take() else {
            return;
        };
        let name = input.read(cx).value().trim().to_string();
        let current = from.rsplit('/').next().unwrap_or(&from).to_string();
        if name.is_empty() || name == current || name.contains('/') {
            cx.notify();
            return;
        }
        let to = match from.rsplit_once('/') {
            Some((parent, _)) => format!("{parent}/{name}"),
            None => name,
        };
        let id = self.instance_id.clone();
        let task = Backend::global(cx)
            .call(async move { theseus::instance::rename_instance_file(&id, &from, &to).await });
        self.change(task, "Couldn't rename that", cx);
    }

    fn finish_new_folder(&mut self, cx: &mut Context<Self>) {
        let Some(input) = self.new_folder.take() else {
            return;
        };
        let name = input.read(cx).value().trim().to_string();
        if name.is_empty() || name.contains('/') {
            cx.notify();
            return;
        }
        let path = if self.dir.is_empty() {
            name
        } else {
            format!("{}/{name}", self.dir)
        };
        let id = self.instance_id.clone();
        let task = Backend::global(cx)
            .call(async move { theseus::instance::create_instance_directory(&id, &path).await });
        self.change(task, "Couldn't make that folder", cx);
    }

    fn crumbs(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette().clone();
        let mut row = div()
            .flex()
            .items_center()
            .gap(space(1.0))
            .min_w_0()
            .text_size(px(type_scale::BODY));
        let crumb = |id: String,
                     label: SharedString,
                     target: String,
                     last: bool,
                     cx: &mut Context<Self>| {
            div()
                .id(SharedString::from(id))
                .px(space(1.0))
                .rounded(px(radius::SM))
                .when(last, |d| {
                    d.text_color(p.text).font_weight(FontWeight::MEDIUM)
                })
                .when(!last, |d| {
                    d.text_color(p.text_muted)
                        .cursor_pointer()
                        .hover(|s| s.text_color(p.text))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.go(target.clone(), cx)
                        }))
                })
                .child(label)
        };
        let parts: Vec<&str> = self.dir.split('/').filter(|s| !s.is_empty()).collect();
        row = row.child(crumb(
            "crumb-root".into(),
            "Instance folder".into(),
            String::new(),
            parts.is_empty(),
            cx,
        ));
        for (ix, part) in parts.iter().enumerate() {
            let target = parts[..=ix].join("/");
            row = row
                .child(icon("chevron-right", 12.0, p.text_disabled))
                .child(crumb(
                    format!("crumb-{ix}"),
                    part.to_string().into(),
                    target,
                    ix + 1 == parts.len(),
                    cx,
                ));
        }
        row.into_any_element()
    }

    fn editor(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = cx.palette().clone();
        let Some(Editing::Open { path, input, saved }) = &self.editing else {
            return None;
        };
        Some(
            div()
                .size_full()
                .flex()
                .flex_col()
                .gap(space(3.0))
                .px(space(8.0))
                .py(space(4.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space(2.0))
                        .child(
                            Button::new("files-editor-back")
                                .icon("arrow-left")
                                .ghost()
                                .small()
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.editing = None;
                                    this.load(cx);
                                    cx.notify();
                                })),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .font_family(fonts::MONO)
                                .text_size(px(type_scale::SMALL))
                                .text_color(p.text_secondary)
                                .child(path.clone()),
                        )
                        .when(*saved, |d| {
                            d.child(
                                div()
                                    .text_size(px(type_scale::SMALL))
                                    .text_color(p.running)
                                    .child("Saved"),
                            )
                        })
                        .child(
                            Button::new("files-editor-save")
                                .icon("check")
                                .label("Save")
                                .small()
                                .primary()
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.save(cx))),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .rounded(px(radius::MD))
                        .border_1()
                        .border_color(p.border)
                        .bg(p.surface)
                        .font_family(fonts::MONO)
                        .text_size(px(type_scale::SMALL))
                        .child(Textarea::new(input).h_full().appearance(false)),
                )
                .into_any_element(),
        )
    }
}

impl Render for FilesBrowser {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Inputs need the window, so they're built here.
        if matches!(self.editing, Some(Editing::Loaded { .. }))
            && let Some(Editing::Loaded { path, text }) = self.editing.take()
        {
            let input = cx.new(|cx| {
                let mut state = TextareaState::new(window, cx);
                state.set_value(text, window, cx);
                state
            });
            self.editing = Some(Editing::Open {
                path,
                input,
                saved: false,
            });
        }
        if let Some(path) = self.pending_rename.take() {
            let current = path.rsplit('/').next().unwrap_or(&path).to_string();
            let input = cx.new(|cx| {
                let mut state = InputState::new(window, cx);
                state.set_value(current, window, cx);
                state
            });
            input.update(cx, |s, cx| s.focus(window, cx));
            cx.subscribe(
                &input,
                |this, _, event: &gpui_kit::component::input::InputEvent, cx| {
                    if matches!(
                        event,
                        gpui_kit::component::input::InputEvent::PressEnter { .. }
                    ) {
                        this.finish_rename(cx);
                    }
                },
            )
            .detach();
            self.renaming = Some((path, input));
        }
        if self.pending_folder {
            self.pending_folder = false;
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Folder name"));
            input.update(cx, |s, cx| s.focus(window, cx));
            cx.subscribe(
                &input,
                |this, _, event: &gpui_kit::component::input::InputEvent, cx| {
                    if matches!(
                        event,
                        gpui_kit::component::input::InputEvent::PressEnter { .. }
                    ) {
                        this.finish_new_folder(cx);
                    }
                },
            )
            .detach();
            self.new_folder = Some(input);
        }
        if let Some(editor) = self.editor(cx) {
            return editor;
        }

        let p = cx.palette().clone();
        let toolbar = div()
            .flex()
            .items_center()
            .gap(space(2.0))
            .px(space(8.0))
            .pt(space(4.0))
            .pb(space(2.0))
            .child(div().flex_1().min_w_0().child(self.crumbs(cx)))
            .child(
                Button::new("files-new-folder")
                    .icon("plus")
                    .label("New folder")
                    .ghost()
                    .small()
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.pending_folder = true;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("files-reveal")
                    .icon("folder-open")
                    .label("Open folder")
                    .ghost()
                    .small()
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        let (id, dir) = (this.instance_id.clone(), this.dir.clone());
                        let task = Backend::global(cx)
                            .call(async move { theseus::instance::get_full_path(&id).await });
                        cx.spawn(async move |_, cx| {
                            if let Ok(root) = task.await {
                                cx.update(|cx| cx.open_with_system(&root.join(&dir)));
                            }
                        })
                        .detach();
                    })),
            );

        let list: AnyElement = match &self.items {
            None => div()
                .flex()
                .flex_col()
                .gap(space(2.0))
                .px(space(8.0))
                .children((0..6).map(|_| skeleton(gpui_kit::relative(1.0), px(36.0), cx)))
                .into_any_element(),
            Some(Err(error)) => empty_state(
                "circle-alert",
                "Couldn't read this folder",
                error.clone(),
                None,
                cx,
            )
            .into_any_element(),
            Some(Ok(items)) if items.is_empty() && self.new_folder.is_none() => {
                empty_state("folder", "Empty folder", "Nothing in here yet.", None, cx)
                    .into_any_element()
            }
            Some(Ok(items)) => {
                let mut rows = div().flex().flex_col().px(space(8.0)).pb(space(8.0));
                if let Some(input) = &self.new_folder {
                    rows = rows.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(space(3.0))
                            .h(px(40.0))
                            .px(space(3.0))
                            .border_b_1()
                            .border_color(p.border)
                            .child(icon("folder", 16.0, p.accent))
                            .child(div().w(px(260.0)).child(Input::new(input).small()))
                            .child(
                                Button::new("files-new-folder-ok")
                                    .label("Create")
                                    .small()
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.finish_new_folder(cx)
                                    })),
                            ),
                    );
                }
                for (ix, item) in items.iter().enumerate() {
                    let dir = item.kind == "directory";
                    let detail = if dir {
                        item.count
                            .map(|n| format!("{n} item{}", if n == 1 { "" } else { "s" }))
                            .unwrap_or_default()
                    } else {
                        item.size.map(human).unwrap_or_default()
                    };
                    // Synced files are links Calcite can't stat; show no date rather than 1970.
                    let when = (item.modified > 0)
                        .then(|| chrono::DateTime::from_timestamp(item.modified as i64, 0))
                        .flatten()
                        .map(relative_time)
                        .unwrap_or_default();
                    let (open_path, open_edit) = (item.path.clone(), editable(item));
                    let (rename_path, delete_path) = (item.path.clone(), item.path.clone());
                    let renaming_this = self
                        .renaming
                        .as_ref()
                        .filter(|(p, _)| p == &item.path)
                        .map(|(_, i)| i.clone());
                    let confirming = self.confirm_delete.as_deref() == Some(item.path.as_str());
                    rows = rows.child(
                        div()
                            .id(SharedString::from(format!("file-{ix}")))
                            .flex()
                            .items_center()
                            .gap(space(3.0))
                            .h(px(40.0))
                            .px(space(3.0))
                            .border_b_1()
                            .border_color(p.border)
                            .hover(|s| s.bg(p.surface))
                            .when(renaming_this.is_none(), |d| {
                                d.cursor_pointer().on_click(cx.listener(
                                    move |this, _: &ClickEvent, _, cx| {
                                        this.open(open_path.clone(), dir, open_edit, cx)
                                    },
                                ))
                            })
                            .child(icon(
                                if dir {
                                    "folder"
                                } else if editable(item) {
                                    "file-text"
                                } else {
                                    "file-archive"
                                },
                                16.0,
                                if dir { p.accent } else { p.text_muted },
                            ))
                            .child(match renaming_this {
                                Some(input) => div()
                                    .w(px(260.0))
                                    .child(Input::new(&input).small())
                                    .into_any_element(),
                                None => div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(type_scale::BODY))
                                    .text_color(p.text)
                                    .child(item.name.clone())
                                    .into_any_element(),
                            })
                            .child(div().flex_1())
                            .child(
                                div()
                                    .w(px(90.0))
                                    .text_right()
                                    .text_size(px(type_scale::SMALL))
                                    .text_color(p.text_muted)
                                    .child(detail),
                            )
                            .child(
                                div()
                                    .w(px(110.0))
                                    .text_right()
                                    .text_size(px(type_scale::SMALL))
                                    .text_color(p.text_muted)
                                    .child(when),
                            )
                            .when(!item.read_only && !confirming, |d| {
                                d.child(
                                    Button::new(SharedString::from(format!("file-rename-{ix}")))
                                        .icon("pencil")
                                        .ghost()
                                        .small()
                                        .on_click(cx.listener(
                                            move |this, _: &ClickEvent, _, cx| {
                                                cx.stop_propagation();
                                                this.pending_rename = Some(rename_path.clone());
                                                cx.notify();
                                            },
                                        )),
                                )
                                .child(
                                    Button::new(SharedString::from(format!("file-delete-{ix}")))
                                        .icon("trash")
                                        .ghost()
                                        .small()
                                        .on_click(cx.listener(
                                            move |this, _: &ClickEvent, _, cx| {
                                                cx.stop_propagation();
                                                this.confirm_delete = Some(delete_path.clone());
                                                cx.notify();
                                            },
                                        )),
                                )
                            })
                            .when(confirming, |d| {
                                let path = item.path.clone();
                                d.child(
                                    Button::new(SharedString::from(format!(
                                        "file-delete-yes-{ix}"
                                    )))
                                    .label("Delete")
                                    .danger()
                                    .small()
                                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                        cx.stop_propagation();
                                        this.delete(path.clone(), cx)
                                    })),
                                )
                                .child(
                                    Button::new(SharedString::from(format!("file-delete-no-{ix}")))
                                        .label("Keep")
                                        .ghost()
                                        .small()
                                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                            cx.stop_propagation();
                                            this.confirm_delete = None;
                                            cx.notify();
                                        })),
                                )
                            }),
                    );
                }
                div()
                    .id("files-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(rows)
                    .into_any_element()
            }
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(toolbar)
            .child(list)
            .into_any_element()
    }
}
