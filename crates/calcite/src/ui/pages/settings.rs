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
    Advanced,
    About,
}

impl Section {
    const ALL: [Section; 7] = [
        Section::Appearance,
        Section::Game,
        Section::Storage,
        Section::MoveIn,
        Section::Accounts,
        Section::Advanced,
        Section::About,
    ];
    fn label(self) -> &'static str {
        match self {
            Section::Appearance => "Appearance",
            Section::Game => "Game",
            Section::Storage => "Storage",
            Section::MoveIn => "Move in",
            Section::Accounts => "Accounts",
            Section::Advanced => "Advanced",
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
            Section::Advanced => "sliders-horizontal",
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
                ))
                .child(group("Behavior", None, behavior(cx), cx)),
        ),
        Section::Accounts => accounts(shell, cx),
        Section::Advanced => match shell.advanced_page() {
            Some(page) => page.into_any_element(),
            None => div().into_any_element(),
        },
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

    let modrinth = modrinth_account(shell, cx);
    scroll_body(
        div().flex().flex_col().gap(space(8.0)).child(group(
            "Microsoft accounts",
            Some("Sign-in happens in a small Microsoft window. Calcite never sees your password."),
            div()
                .flex()
                .flex_col()
                .gap(space(3.0))
                .child(list)
                .child(
                    div().flex().child(
                        Button::new("settings-add-account")
                            .icon("user-plus")
                            .label("Add Microsoft account")
                            .on_click(|_, _, cx| crate::login::sign_in(cx)),
                    ),
                )
                .into_any_element(),
            cx,
        ))
        .child(group(
            "Modrinth account",
            Some("Optional. Lets you see friends, share instances with them, and play together. Sign-in happens in your browser."),
            modrinth,
            cx,
        )),
    )
}

fn modrinth_account(shell: &mut Shell, cx: &mut Context<Shell>) -> AnyElement {
    let p = cx.palette().clone();
    let state = shell.state().read(cx);
    let user = state.modrinth.clone();
    let waiting = state.modrinth_signing_in.is_some();
    let row = div()
        .flex()
        .items_center()
        .gap(space(3.0))
        .px(space(4.0))
        .py(space(3.0));
    let body = match (user, waiting) {
        (Some(user), _) => {
            let avatar = user
                .avatar_url
                .as_deref()
                .and_then(|u| crate::images::Images::get(u, 64, cx));
            row.child(crate::ui::components::thumb(
                avatar,
                &user.username,
                32.0,
                radius::MD,
                cx,
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
                            .child(user.username.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(type_scale::SMALL))
                            .text_color(p.text_muted)
                            .child("Signed in to Modrinth"),
                    ),
            )
            .child(
                Button::new("modrinth-sign-out")
                    .icon("log-out")
                    .label("Sign out")
                    .ghost()
                    .small()
                    .on_click(|_, _, cx| crate::modrinth_login::sign_out(cx)),
            )
        }
        (None, true) => row
            .child(icon("loader-circle", 16.0, p.accent))
            .child(
                div()
                    .flex_1()
                    .text_size(px(type_scale::BODY))
                    .text_color(p.text_secondary)
                    .child("Finish signing in in your browser, then come back here."),
            )
            .child(
                Button::new("modrinth-cancel")
                    .label("Cancel")
                    .small()
                    .on_click(|_, _, cx| crate::modrinth_login::cancel(cx)),
            ),
        (None, false) => row
            .child(icon("globe", 16.0, p.text_muted))
            .child(
                div()
                    .flex_1()
                    .text_size(px(type_scale::BODY))
                    .text_color(p.text_secondary)
                    .child("Not signed in."),
            )
            .child(
                Button::new("modrinth-sign-in")
                    .icon("log-in")
                    .label("Sign in to Modrinth")
                    .small()
                    .on_click(|_, _, cx| crate::modrinth_login::sign_in(cx)),
            ),
    };
    panel(cx).overflow_hidden().child(body).into_any_element()
}

/// Switches for how Calcite behaves around the game.
fn behavior(cx: &mut Context<Shell>) -> AnyElement {
    use crate::prefs::{Prefs, StartPage};
    let p = cx.palette().clone();
    let prefs = cx.global::<Prefs>().clone();
    let row = |id: &'static str,
               title: &'static str,
               detail: &'static str,
               on: bool,
               flip: fn(&mut Prefs),
               cx: &mut Context<Shell>| {
        div()
            .id(id)
            .flex()
            .items_center()
            .gap(space(3.0))
            .px(space(4.0))
            .py(space(3.0))
            .cursor_pointer()
            .hover(|s| s.bg(p.hover))
            .on_click(move |_, _, cx| {
                let prefs = cx.global_mut::<Prefs>();
                flip(prefs);
                prefs.save();
                cx.refresh_windows();
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
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
                    ),
            )
            .child(crate::ui::pages::instance::toggle(on, cx))
    };
    let start = |page: StartPage, label: &'static str| {
        let active = prefs.start_page == page;
        div()
            .id(SharedString::from(format!("start-{label}")))
            .h(px(28.0))
            .px(space(2.5))
            .flex()
            .items_center()
            .rounded(px(radius::SM))
            .border_1()
            .text_size(px(type_scale::SMALL))
            .when(active, |d| {
                d.border_color(p.accent)
                    .bg(p.accent_soft)
                    .text_color(p.text)
            })
            .when(!active, |d| {
                d.border_color(p.border)
                    .text_color(p.text_secondary)
                    .cursor_pointer()
                    .hover(|s| s.bg(p.hover))
            })
            .on_click(move |_, _, cx| {
                let prefs = cx.global_mut::<Prefs>();
                prefs.start_page = page;
                prefs.save();
                cx.refresh_windows();
            })
            .child(label)
    };
    panel(cx)
        .overflow_hidden()
        .child(row(
            "pref-minimize",
            "Minimize when the game starts",
            "Calcite gets out of the way while you play.",
            prefs.minimize_on_launch,
            |p| p.minimize_on_launch = !p.minimize_on_launch,
            cx,
        ))
        .child(div().h(px(1.0)).bg(p.border))
        .child(row(
            "pref-refocus",
            "Come back when the game closes",
            "Bring Calcite to the front after you quit Minecraft.",
            prefs.refocus_on_exit,
            |p| p.refocus_on_exit = !p.refocus_on_exit,
            cx,
        ))
        .child(div().h(px(1.0)).bg(p.border))
        .child(row(
            "pref-backups",
            "Back up worlds when the game closes",
            "Keeps the last few copies of each world you played.",
            prefs.backup_worlds_on_exit,
            |p| p.backup_worlds_on_exit = !p.backup_worlds_on_exit,
            cx,
        ))
        .child(div().h(px(1.0)).bg(p.border))
        .child(
            div()
                .flex()
                .items_center()
                .gap(space(3.0))
                .px(space(4.0))
                .py(space(3.0))
                .child(
                    div()
                        .flex_1()
                        .text_size(px(type_scale::BODY))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(p.text)
                        .child("Open Calcite on"),
                )
                .child(
                    div()
                        .flex()
                        .gap(space(1.5))
                        .child(start(StartPage::Home, "Home"))
                        .child(start(StartPage::Library, "Library")),
                ),
        )
        .into_any_element()
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
