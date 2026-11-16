//! First run: a welcome, then pick a look, move in from another launcher,
//! sign in, and pick where to start.

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, ClickEvent, Context, Entity, FontWeight, SharedString, div, px};

use crate::theme::{PaletteExt, radius, space, type_scale};
use crate::ui::components::{Button, icon};
use crate::ui::look::{accent_picker, appearance_picker};
use crate::ui::motion::{fade, rise, rise_slow};
use crate::ui::pages::move_in::MoveInPage;
use crate::ui::shell::{Route, Shell};

pub const STEPS: [&str; 5] = ["Welcome", "Look", "Move in", "Sign in", "Ready"];

/// Height of the step body, the same on every step so nothing jumps.
const BODY_HEIGHT: f32 = 400.0;

pub fn render(
    shell: &mut Shell,
    step: usize,
    move_in: Option<Entity<MoveInPage>>,
    cx: &mut Context<Shell>,
) -> AnyElement {
    if step == 0 {
        return welcome(cx);
    }
    let p = cx.palette().clone();
    let account = shell.state().read(cx).active_account().cloned();
    let instances = shell.state().read(cx).instances.len();

    // A thin bar per step after the welcome.
    let mut progress = div().flex().gap(space(1.5));
    for ix in 1..STEPS.len() {
        progress = progress.child(div().h(px(3.0)).flex_1().rounded_full().bg(if ix <= step {
            p.accent
        } else {
            p.border_strong
        }));
    }

    let (title, subtitle, body): (&str, String, AnyElement) = match step {
        1 => (
            "Make it yours",
            "Pick a theme and an accent. You can change them any time in Settings.".into(),
            div()
                .flex()
                .flex_col()
                .gap(space(6.0))
                .child(appearance_picker(cx))
                .child(accent_picker(cx))
                .into_any_element(),
        ),
        2 => (
            "Bring your stuff",
            "Calcite found these launchers. Nothing in them is changed: pick what to bring over, or skip and do it later from Settings.".into(),
            div()
                .size_full()
                .children(move_in.map(|m| m.into_any_element()))
                .into_any_element(),
        ),
        3 => (
            "Sign in to play",
            match &account {
                Some(a) => format!("Signed in as {}. You can add more accounts later in Settings.", a.name),
                None => "Minecraft needs the Microsoft account that owns the game. Sign-in happens in Microsoft's own window, and Calcite never sees your password.".into(),
            },
            match &account {
                Some(a) => div()
                    .flex()
                    .items_center()
                    .gap(space(3.0))
                    .p(space(4.0))
                    .rounded(px(radius::LG))
                    .bg(p.surface)
                    .border_1()
                    .border_color(p.border)
                    .child(icon("circle-check", 20.0, p.running))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(type_scale::BODY_LG))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(p.text)
                                    .child(a.name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(type_scale::SMALL))
                                    .text_color(p.text_muted)
                                    .child("Ready to play"),
                            ),
                    )
                    .into_any_element(),
                None => div()
                    .flex()
                    .child(
                        Button::new("onboarding-sign-in")
                            .icon("log-in")
                            .label("Sign in with Microsoft")
                            .primary()
                            .on_click(|_, _, cx| crate::login::sign_in(cx)),
                    )
                    .into_any_element(),
            },
        ),
        _ => (
            "You're all set",
            if instances > 0 {
                format!(
                    "{instances} instance{} {} waiting in your library. Where to?",
                    if instances == 1 { "" } else { "s" },
                    if instances == 1 { "is" } else { "are" }
                )
            } else {
                "Where do you want to start?".into()
            },
            ready_choices(instances > 0, cx),
        ),
    };

    let last = step + 1 == STEPS.len();
    let skippable = step == 2 || (step == 3 && account.is_none());
    let key = |part: &str| SharedString::from(format!("onboarding-{step}-{part}"));
    div()
        .size_full()
        .flex()
        .justify_center()
        .bg(p.app)
        .child(
            div()
                .w(px(720.0))
                .pt(px(72.0))
                .flex()
                .flex_col()
                .gap(space(6.0))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(space(3.0))
                        .child(progress)
                        .child(
                            div()
                                .text_size(px(type_scale::SMALL))
                                .text_color(p.text_muted)
                                .child(format!("Step {step} of {}", STEPS.len() - 1)),
                        ),
                )
                .child(rise(
                    div()
                        .h(px(96.0))
                        .flex()
                        .flex_col()
                        .gap(space(2.0))
                        .child(
                            div()
                                .text_size(px(type_scale::HEADLINE))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(p.text)
                                .child(title),
                        )
                        .child(
                            div()
                                .text_size(px(type_scale::BODY_LG))
                                .text_color(p.text_secondary)
                                .child(subtitle),
                        ),
                    key("head"),
                    0,
                    8.0,
                ))
                .child(fade(
                    div().h(px(BODY_HEIGHT)).min_h_0().child(body),
                    key("body"),
                    80,
                ))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space(2.0))
                        .child(
                            Button::new("onboarding-back")
                                .icon("arrow-left")
                                .label("Back")
                                .ghost()
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.set_onboarding_step(step - 1, window, cx)
                                })),
                        )
                        .child(div().flex_1())
                        .when(skippable, |d| {
                            d.child(
                                Button::new("onboarding-skip")
                                    .label("Skip")
                                    .ghost()
                                    .on_click(cx.listener(
                                        move |this, _: &ClickEvent, window, cx| {
                                            this.set_onboarding_step(step + 1, window, cx)
                                        },
                                    )),
                            )
                        })
                        .child(
                            Button::new("onboarding-next")
                                .icon(if last { "check" } else { "arrow-right" })
                                .label(if last { "Done" } else { "Continue" })
                                .primary()
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    if last {
                                        this.finish_onboarding(window, cx)
                                    } else {
                                        this.set_onboarding_step(step + 1, window, cx)
                                    }
                                })),
                        ),
                ),
        )
        .into_any_element()
}

/// The first thing anyone sees: the mark assembling, then the name and
/// what Calcite promises.
fn welcome(cx: &mut Context<Shell>) -> AnyElement {
    let p = cx.palette().clone();
    let promise = |glyph: &'static str, text: &'static str| {
        div()
            .flex()
            .items_center()
            .gap(space(1.5))
            .text_size(px(type_scale::SMALL))
            .text_color(p.text_secondary)
            .child(icon(glyph, 14.0, p.accent))
            .child(text)
    };
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(space(6.0))
        .bg(p.app)
        .pb(px(48.0))
        .child(crate::ui::logo::gem_assembling(132.0, "welcome-gem"))
        .child(rise_slow(
            div()
                .text_size(px(44.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(p.text)
                .child("Calcite"),
            "welcome-name",
            650,
            14.0,
        ))
        .child(rise_slow(
            div()
                .text_size(px(type_scale::SECTION))
                .text_color(p.text_secondary)
                .child("A fast, native Minecraft launcher."),
            "welcome-line",
            800,
            12.0,
        ))
        .child(rise_slow(
            div()
                .flex()
                .gap(space(5.0))
                .child(promise("shield-check", "No ads or tracking"))
                .child(promise("gauge", "Light on memory"))
                .child(promise("hard-drive", "Shares files between instances")),
            "welcome-promises",
            950,
            10.0,
        ))
        .child(rise_slow(
            div().pt(space(2.0)).child(
                Button::new("onboarding-start")
                    .icon("arrow-right")
                    .label("Get started")
                    .primary()
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.set_onboarding_step(1, window, cx)
                    })),
            ),
            "welcome-start",
            1100,
            10.0,
        ))
        .into_any_element()
}

/// The last step: three places to go, each finishing the setup.
fn ready_choices(has_instances: bool, cx: &mut Context<Shell>) -> AnyElement {
    let p = cx.palette().clone();
    let card = |key: &'static str,
                glyph: &'static str,
                title: &'static str,
                detail: &'static str,
                route: Option<Route>,
                cx: &mut Context<Shell>| {
        div()
            .id(key)
            .flex_1()
            .flex()
            .flex_col()
            .gap(space(3.0))
            .p(space(5.0))
            .rounded(px(radius::LG))
            .bg(p.surface)
            .border_1()
            .border_color(p.border)
            .cursor_pointer()
            .hover(|s| s.border_color(p.accent).bg(p.raised))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.finish_onboarding(window, cx);
                match &route {
                    Some(route) => this.navigate(route.clone(), window, cx),
                    None => this.open_servers(window, cx),
                }
            }))
            .child(
                div()
                    .size(px(36.0))
                    .rounded(px(radius::MD))
                    .bg(p.accent_soft)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon(glyph, 18.0, p.accent_text)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(space(1.0))
                    .child(
                        div()
                            .text_size(px(type_scale::BODY_LG))
                            .font_weight(FontWeight::SEMIBOLD)
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
    };
    let mut row = div().flex().gap(space(3.0));
    if has_instances {
        row = row.child(card(
            "ready-library",
            "library-big",
            "Your library",
            "Everything you brought over, ready to play.",
            Some(Route::Library),
            cx,
        ));
    }
    row.child(card(
        "ready-discover",
        "compass",
        "Browse modpacks",
        "Thousands of packs, one click to install.",
        Some(Route::Discover),
        cx,
    ))
    .child(card(
        "ready-servers",
        "server",
        "Find a server",
        "Join a community with the right mods set up for you.",
        None,
        cx,
    ))
    .into_any_element()
}
