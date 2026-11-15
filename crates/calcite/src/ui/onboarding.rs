//! First run: pick a look, move in from another launcher, sign in.

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, ClickEvent, Context, Entity, FontWeight, SharedString, div, px};

use crate::theme::{PaletteExt, radius, space, type_scale};
use crate::ui::components::{Button, icon};
use crate::ui::look::{accent_picker, appearance_picker};
use crate::ui::pages::move_in::MoveInPage;
use crate::ui::shell::Shell;

pub const STEPS: [&str; 4] = ["Look", "Move in", "Sign in", "Ready"];

pub fn render(
    shell: &mut Shell,
    step: usize,
    move_in: Option<Entity<MoveInPage>>,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let p = cx.palette().clone();
    let account = shell.state().read(cx).active_account().cloned();
    let instances = shell.state().read(cx).instances.len();

    let mut dots = div().flex().items_center().gap(space(2.0));
    for (ix, name) in STEPS.iter().enumerate() {
        let done = ix < step;
        let current = ix == step;
        dots = dots
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space(1.5))
                    .child(
                        div()
                            .size(px(18.0))
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(if current || done { p.accent } else { p.raised })
                            .text_size(px(type_scale::CAPTION))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(if current || done {
                                p.on_accent
                            } else {
                                p.text_muted
                            })
                            .child(if done {
                                icon("check", 11.0, p.on_accent).into_any_element()
                            } else {
                                format!("{}", ix + 1).into_any_element()
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(type_scale::SMALL))
                            .text_color(if current { p.text } else { p.text_muted })
                            .child(*name),
                    ),
            )
            .when(ix + 1 < STEPS.len(), |d| {
                d.child(div().w(px(24.0)).h(px(1.0)).bg(p.border_strong))
            });
    }

    let (title, subtitle, body): (&str, String, AnyElement) = match step {
        0 => (
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
        1 => (
            "Bring your stuff",
            "Calcite found these launchers. Nothing in them is changed; choose what to bring over, or skip.".into(),
            div()
                .h(px(360.0))
                .rounded(px(radius::LG))
                .border_1()
                .border_color(p.border)
                .overflow_hidden()
                .children(move_in.map(|m| m.into_any_element()))
                .into_any_element(),
        ),
        2 => (
            "Sign in to play",
            match &account {
                Some(a) => format!("Signed in as {}. Add more accounts later in Settings.", a.name),
                None => "Minecraft needs a Microsoft account that owns the game. Sign-in happens in Microsoft's own window; Calcite never sees your password.".into(),
            },
            match &account {
                Some(_) => div()
                    .flex()
                    .items_center()
                    .gap(space(2.0))
                    .child(icon("circle-check", 18.0, p.running))
                    .child(
                        div()
                            .text_size(px(type_scale::BODY))
                            .text_color(p.text)
                            .child("You're ready to play."),
                    )
                    .into_any_element(),
                None => div()
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
                format!("{instances} instances are waiting in your library.")
            } else {
                "Install a modpack from Discover or make your first instance.".into()
            },
            div()
                .flex()
                .flex_col()
                .gap(space(2.0))
                .children(
                    [
                        ("shield-check", "No ads, no purchases, no telemetry."),
                        ("hard-drive", "Files shared between instances instead of copied."),
                        ("gauge", "A native app: low memory, no background CPU."),
                    ]
                    .into_iter()
                    .map(|(glyph, text)| {
                        div()
                            .flex()
                            .items_center()
                            .gap(space(2.5))
                            .child(icon(glyph, 16.0, p.accent))
                            .child(
                                div()
                                    .text_size(px(type_scale::BODY))
                                    .text_color(p.text_secondary)
                                    .child(text),
                            )
                    }),
                )
                .into_any_element(),
        ),
    };

    let last = step + 1 == STEPS.len();
    let skippable = step == 1 || (step == 2 && account.is_none());
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(p.app)
        .child(
            div()
                .w(px(680.0))
                .flex()
                .flex_col()
                .gap(space(6.0))
                .child(dots)
                .child(
                    div()
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
                )
                .child(body)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space(2.0))
                        .pt(space(2.0))
                        .when(step > 0, |d| {
                            d.child(
                                Button::new("onboarding-back")
                                    .icon("arrow-left")
                                    .label("Back")
                                    .ghost()
                                    .on_click(cx.listener(
                                        move |this, _: &ClickEvent, window, cx| {
                                            this.set_onboarding_step(step - 1, window, cx)
                                        },
                                    )),
                            )
                        })
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
                            Button::new(SharedString::from("onboarding-next"))
                                .icon(if last { "play" } else { "arrow-right" })
                                .label(if last { "Start playing" } else { "Continue" })
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
