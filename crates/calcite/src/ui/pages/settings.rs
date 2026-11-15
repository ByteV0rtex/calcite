//! Settings: Appearance, Game, Storage, Move in, Accounts, About.

use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, ClickEvent, Context, Entity, FontWeight, SharedString, Window, div, px,
};

use crate::state::AppState;
use crate::theme::{PaletteExt, radius, space, type_scale};
use crate::ui::components::{Button, icon, panel, section_heading};
use crate::ui::look::{accent_picker, appearance_picker};
use crate::ui::pages::move_in::MoveInPage;
use crate::ui::shell::Shell;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Section {
    #[default]
    Appearance,
    Game,
    Storage,
    MoveIn,
    Accounts,
    About,
}

impl Section {
    const ALL: [Section; 6] = [
        Section::Appearance,
        Section::Game,
        Section::Storage,
        Section::MoveIn,
        Section::Accounts,
        Section::About,
    ];
    fn label(self) -> &'static str {
        match self {
            Section::Appearance => "Appearance",
            Section::Game => "Game",
            Section::Storage => "Storage",
            Section::MoveIn => "Move in",
            Section::Accounts => "Accounts",
            Section::About => "About",
        }
    }
    fn icon(self) -> &'static str {
        match self {
            Section::Appearance => "sun",
            Section::Game => "gamepad-2",
            Section::Storage => "hard-drive",
            Section::MoveIn => "arrow-down-to-line",
            Section::Accounts => "user",
            Section::About => "info",
        }
    }
}

pub fn render(
    shell: &mut Shell,
    section: Section,
    move_in: Option<Entity<MoveInPage>>,
    game: Option<Entity<crate::ui::pages::game_settings::GameSettings>>,
    storage: Option<Entity<crate::ui::pages::storage::StoragePage>>,
    _window: &mut Window,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let p = cx.palette().clone();

    let mut nav = div()
        .flex()
        .flex_col()
        .gap(space(0.5))
        .w(px(200.0))
        .flex_none()
        .pt(space(7.0))
        .px(space(3.0))
        .child(
            div()
                .px(space(2.5))
                .pb(space(3.0))
                .text_size(px(type_scale::TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(p.text)
                .child("Settings"),
        );
    for s in Section::ALL {
        let active = s == section;
        nav = nav.child(
            div()
                .id(SharedString::from(format!("settings-{}", s.label())))
                .flex()
                .items_center()
                .gap(space(2.5))
                .h(px(32.0))
                .px(space(2.5))
                .rounded(px(radius::SM))
                .text_size(px(type_scale::BODY))
                .font_weight(FontWeight::MEDIUM)
                .cursor_pointer()
                .when(active, |d| d.bg(p.hover).text_color(p.text))
                .when(!active, |d| {
                    d.text_color(p.text_secondary).hover(|st| st.bg(p.hover))
                })
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.open_settings(s, window, cx)
                }))
                .child(icon(
                    s.icon(),
                    16.0,
                    if active { p.accent } else { p.text_muted },
                ))
                .child(s.label()),
        );
    }

    let body: AnyElement = match section {
        Section::Game => match game {
            Some(page) => page.into_any_element(),
            None => div().into_any_element(),
        },
        Section::Storage => match storage {
            Some(page) => page.into_any_element(),
            None => div().into_any_element(),
        },
        Section::MoveIn => match move_in {
            Some(page) => page.into_any_element(),
            None => div().into_any_element(),
        },
        Section::Appearance => scroll_body(
            div()
                .flex()
                .flex_col()
                .gap(space(8.0))
                .child(group(
                    "Theme",
                    None,
                    appearance_picker(cx).into_any_element(),
                    cx,
                ))
                .child(group(
                    "Accent",
                    Some("Used for Play, progress and anything that needs your attention."),
                    accent_picker(cx).into_any_element(),
                    cx,
                )),
        ),
        Section::Accounts => accounts(shell, cx),
        Section::About => about(cx),
    };

    div()
        .size_full()
        .flex()
        .child(nav)
        .child(div().w(px(1.0)).h_full().bg(p.border))
        .child(div().flex_1().min_w_0().h_full().child(body))
        .into_any_element()
}

fn scroll_body(content: gpui_kit::Div) -> AnyElement {
    div()
        .id("settings-scroll")
        .size_full()
        .overflow_y_scroll()
        .child(
            div()
                .px(space(8.0))
                .py(space(8.0))
                .max_w(px(760.0))
                .child(content),
        )
        .into_any_element()
}

fn group(
    title: &'static str,
    hint: Option<&'static str>,
    body: AnyElement,
    cx: &App,
) -> gpui_kit::Div {
    let p = cx.palette();
    div()
        .flex()
        .flex_col()
        .gap(space(3.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(space(1.0))
                .child(section_heading(title, cx))
                .when_some(hint, |d, h| {
                    d.child(
                        div()
                            .text_size(px(type_scale::SMALL))
                            .text_color(p.text_muted)
                            .child(h),
                    )
                }),
        )
        .child(body)
}

fn accounts(shell: &mut Shell, cx: &mut Context<Shell>) -> AnyElement {
    let p = cx.palette().clone();
    let state = shell.state().read(cx);
    let active = state.active_account().map(|a| a.id);
    let accounts = state.accounts.clone();

    let mut list = panel(cx).overflow_hidden();
    if accounts.is_empty() {
        list = list.child(
            div()
                .p(space(4.0))
                .text_size(px(type_scale::BODY))
                .text_color(p.text_secondary)
                .child(
                    "No accounts yet. You need a Microsoft account that owns Minecraft to play.",
                ),
        );
    }
    for (ix, account) in accounts.into_iter().enumerate() {
        let id = account.id;
        let is_active = Some(id) == active;
        list = list.child(
            div()
                .flex()
                .items_center()
                .gap(space(3.0))
                .h(px(52.0))
                .px(space(4.0))
                .when(ix > 0, |d| d.border_t_1().border_color(p.border))
                .child(icon("user", 16.0, p.text_muted))
                .child(
                    div()
                        .flex_1()
                        .text_size(px(type_scale::BODY))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(p.text)
                        .child(account.name.clone()),
                )
                .when(is_active, |d| {
                    d.child(
                        div()
                            .text_size(px(type_scale::SMALL))
                            .text_color(p.accent_text)
                            .child("In use"),
                    )
                })
                .when(!is_active, |d| {
                    d.child(
                        Button::new(SharedString::from(format!("use-{id}")))
                            .label("Use")
                            .small()
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.state()
                                    .update(cx, |s, cx| s.set_default_account(id, cx))
                            })),
                    )
                })
                .child(
                    Button::new(SharedString::from(format!("signout-{id}")))
                        .icon("log-out")
                        .label("Sign out")
                        .ghost()
                        .small()
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.state().update(cx, |s, cx| s.remove_account(id, cx))
                        })),
                ),
        );
    }

    scroll_body(
        div().flex().flex_col().gap(space(4.0)).child(group(
            "Microsoft accounts",
            Some("Sign-in happens in a small Microsoft window. Calcite never sees your password."),
            div()
                .flex()
                .flex_col()
                .gap(space(3.0))
                .child(list)
                .child(
                    div().child(
                        Button::new("settings-add-account")
                            .icon("user-plus")
                            .label("Add Microsoft account")
                            .on_click(|_, _, cx| crate::login::sign_in(cx)),
                    ),
                )
                .into_any_element(),
            cx,
        )),
    )
}

fn about(cx: &mut Context<Shell>) -> AnyElement {
    let p = cx.palette().clone();
    let data_dir = crate::backend::data_dir();
    let row = |label: &'static str, value: String| {
        div()
            .flex()
            .gap(space(4.0))
            .py(space(2.5))
            .border_b_1()
            .border_color(p.border)
            .child(
                div()
                    .w(px(140.0))
                    .flex_none()
                    .text_size(px(type_scale::BODY))
                    .text_color(p.text_muted)
                    .child(label),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(type_scale::BODY))
                    .text_color(p.text)
                    .child(value),
            )
    };
    let instances = AppState::global(cx).read(cx).instances.len();
    scroll_body(
        div()
            .flex()
            .flex_col()
            .gap(space(6.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space(3.0))
                    .child(
                        gpui_kit::svg()
                            .path(SharedString::from("brand/mark.svg"))
                            .size(px(36.0))
                            .text_color(p.accent),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(type_scale::TITLE))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(p.text)
                                    .child("Calcite"),
                            )
                            .child(
                                div()
                                    .text_size(px(type_scale::SMALL))
                                    .text_color(p.text_muted)
                                    .child(format!("Version {}", calcite_core::version())),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(row("Promise", "No ads. No purchases. No telemetry.".into()))
                    .child(row("Instances", instances.to_string()))
                    .child(row(
                        "Data folder",
                        data_dir
                            .as_ref()
                            .map(|d| d.display().to_string())
                            .unwrap_or_default(),
                    ))
                    .child(row(
                        "Backend",
                        format!("Modrinth launcher backend {}", env!("CALCITE_BACKEND_VERSION")),
                    ))
                    .child(row("License", "GPL-3.0".into())),
            )
            .when_some(data_dir, |d, dir| {
                d.child(
                    div().child(
                        Button::new("open-data-folder")
                            .icon("folder-open")
                            .label("Open data folder")
                            .on_click(move |_, _, cx| cx.open_with_system(&dir)),
                    ),
                )
            })
            .child(
                div()
                    .text_size(px(type_scale::SMALL))
                    .text_color(p.text_muted)
                    .child("Not affiliated with Modrinth, Mojang or Microsoft. Fonts: Inter and JetBrains Mono (SIL OFL). Icons: Lucide (ISC)."),
            ),
    )
}

use gpui_kit::App;
