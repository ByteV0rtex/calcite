//! The 9/IB SMP screen: the server, your account and one play button.
//! Shown in 9/IB mode (normal Calcite, from the sidebar) and always in the
//! 9/IB edition. Its words are Turkish, like the server.

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, ClickEvent, Context, FontWeight, Hsla, SharedString, div, px};

use crate::backend::Backend;
use crate::ibsmp::{DEFAULT_NAME, DEFAULT_SEASON, EDITION};
use crate::state::IbStep;
use crate::theme::{PaletteExt, radius, space, type_scale};
use crate::ui::components::{Button, icon, progress_bar};
use crate::ui::motion::rise_slow;
use crate::ui::shell::Shell;

/// How long "Başlatılıyor…" may show before the button is usable again
/// (the game normally reports itself running well before this).
const STARTING_FOR_AT_MOST: std::time::Duration = std::time::Duration::from_secs(90);

/// What the big button shows.
enum Play {
    SignIn,
    Ready { first_time: bool, update: bool },
    Preparing,
    Installing { fraction: f32, phase: &'static str },
    Starting,
    Playing,
}

pub fn render(shell: &mut Shell, cx: &mut Context<Shell>) -> AnyElement {
    let p = cx.palette().clone();
    let state = shell.state().read(cx);
    let account = state.active_account().cloned();
    let face = account.as_ref().and_then(|a| state.face_for(a.id));
    let ib = state.ibsmp.clone();
    let instance = state.ibsmp_instance(cx);
    let running = instance
        .as_ref()
        .is_some_and(|id| state.running.contains_key(id));
    let job = match &ib.step {
        IbStep::Installing(id) => state.jobs.iter().find(|j| &j.id == id).cloned(),
        _ => None,
    };
    let have_version = cx
        .global::<crate::prefs::Prefs>()
        .ibsmp_pack_version
        .clone();

    let (name, season) = ib
        .manifest
        .as_ref()
        .map(|m| (m.name.clone(), m.season.clone()))
        .unwrap_or_else(|| (DEFAULT_NAME.to_string(), DEFAULT_SEASON.to_string()));
    let news = ib.manifest.as_ref().and_then(|m| m.news.clone());
    let update = match (&ib.manifest, &have_version) {
        (Some(m), Some(v)) => instance.is_some() && m.pack.version != *v,
        _ => false,
    };

    let play = if running {
        Play::Playing
    } else if account.is_none() {
        Play::SignIn
    } else {
        match &ib.step {
            IbStep::Preparing => Play::Preparing,
            IbStep::Installing(_) => Play::Installing {
                fraction: job
                    .as_ref()
                    .filter(|j| j.total > 0)
                    .map(|j| (j.current as f32 / j.total as f32).clamp(0.0, 1.0))
                    .unwrap_or(0.0),
                phase: job.as_ref().map(|j| j.phase).unwrap_or(""),
            },
            IbStep::Starting(at) if at.elapsed() < STARTING_FOR_AT_MOST => Play::Starting,
            _ => Play::Ready {
                first_time: instance.is_none(),
                update,
            },
        }
    };

    // "9/IB" big, "SMP" beside it, the season under it.
    let (big, rest) = name
        .split_once(' ')
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .unwrap_or((name.clone(), String::new()));
    let logo = div()
        .flex()
        .flex_col()
        .items_center()
        .gap(space(1.5))
        .child(
            div()
                .flex()
                .items_baseline()
                .gap(space(3.0))
                .child(
                    div()
                        .text_size(px(76.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(p.accent)
                        .child(big),
                )
                .when(!rest.is_empty(), |d| {
                    d.child(
                        div()
                            .text_size(px(40.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.text)
                            .child(rest),
                    )
                }),
        )
        .child(
            div()
                .px(space(3.0))
                .py(space(1.0))
                .rounded_full()
                .bg(p.accent_soft)
                .text_size(px(type_scale::BODY_LG))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(p.accent_text)
                .child(season),
        );

    // Server status: a dot and a line.
    let (dot, status): (Hsla, String) = match (ib.checked, ib.players) {
        (_, Some((online, max))) => (
            p.running,
            if online == 0 {
                format!("Sunucu açık · şu an kimse yok (en fazla {max})")
            } else {
                format!("Sunucu açık · {online}/{max} oyuncu")
            },
        ),
        (false, None) => (p.text_muted, "Sunucuya bakılıyor…".into()),
        (true, None) => (p.danger, "Sunucuya şu an ulaşılamıyor".into()),
    };
    let status_line = div()
        .flex()
        .items_center()
        .gap(space(2.0))
        .text_size(px(type_scale::BODY_LG))
        .text_color(p.text_secondary)
        .child(div().size(px(8.0)).rounded_full().bg(dot))
        .child(status);

    let button = play_button(&play, instance.clone(), cx);
    let below: Option<AnyElement> = match &play {
        Play::Installing { fraction, phase } => Some(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(space(1.5))
                .child(progress_bar(*fraction, cx))
                .child(
                    div()
                        .text_size(px(type_scale::SMALL))
                        .text_color(p.text_muted)
                        .child(install_phase(phase)),
                )
                .into_any_element(),
        ),
        Play::Ready {
            first_time: true, ..
        } => Some(hint(
            "İlk seferde modlar indirilir (yaklaşık 250 MB). Sonra direkt açılır.",
            cx,
        )),
        Play::Ready { update: true, .. } => Some(hint(
            "Sunucunun yeni paketi var: sadece değişenler güncellenir, dünyaların ve ayarların kalır.",
            cx,
        )),
        _ => None,
    };

    let account_row = match &account {
        Some(a) => div()
            .flex()
            .items_center()
            .gap(space(2.5))
            .child(crate::ui::components::thumb(face, &a.name, 28.0, 6.0, cx))
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
                            .child("Microsoft hesabı"),
                    ),
            )
            .child(div().flex_1())
            .child(
                Button::new("ibsmp-other-account")
                    .label("Başka hesap")
                    .ghost()
                    .small()
                    .on_click(|_, _, cx| crate::login::sign_in(cx)),
            )
            .into_any_element(),
        None => div()
            .text_size(px(type_scale::SMALL))
            .text_color(p.text_muted)
            .child("Oynamak için oyunu satın almış Microsoft hesabınla giriş yap. Şifreni Calcite hiç görmez.")
            .into_any_element(),
    };

    let mut footer = div().flex().items_center().justify_center().gap(space(2.0));
    if let Some(id) = instance.clone() {
        footer = footer.child(
            Button::new("ibsmp-folder")
                .icon("folder-open")
                .label("Oyun klasörü")
                .ghost()
                .small()
                .on_click(move |_, _, cx| open_instance_folder(id.clone(), cx)),
        );
    }
    if !EDITION {
        footer = footer.child(
            Button::new("ibsmp-leave")
                .icon("arrow-left")
                .label("Back to Calcite")
                .ghost()
                .small()
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.set_ibsmp_mode(false, window, cx)
                })),
        );
    }

    div()
        .id("ibsmp-page")
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .bg(p.app)
        .pb(px(24.0))
        .child(
            div()
                .w(px(420.0))
                .flex()
                .flex_col()
                .items_center()
                .gap(space(5.0))
                .child(rise_slow(logo, "ibsmp-logo", 0, 14.0))
                .child(status_line)
                .when_some(news, |d, news| {
                    d.child(
                        div()
                            .w_full()
                            .text_center()
                            .text_size(px(type_scale::BODY))
                            .text_color(p.text_secondary)
                            .child(news),
                    )
                })
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap(space(3.0))
                        .pt(space(2.0))
                        .child(button)
                        .children(below),
                )
                .child(
                    div()
                        .w_full()
                        .p(space(3.5))
                        .rounded(px(radius::LG))
                        .bg(p.surface)
                        .border_1()
                        .border_color(p.border)
                        .child(account_row),
                )
                .child(footer),
        )
        .into_any_element()
}

/// The big button. It does one thing at a time and says what.
fn play_button(play: &Play, instance: Option<String>, cx: &mut Context<Shell>) -> AnyElement {
    let p = cx.palette().clone();
    let (glyph, label, enabled): (&str, SharedString, bool) = match play {
        Play::SignIn => ("log-in", "Microsoft ile giriş yap".into(), true),
        Play::Ready {
            first_time: true, ..
        } => ("download", "İNDİR VE OYNA".into(), true),
        Play::Ready { update: true, .. } => ("refresh-cw", "GÜNCELLE VE OYNA".into(), true),
        Play::Ready { .. } => ("play", "OYNA".into(), true),
        Play::Preparing => ("loader-circle", "Hazırlanıyor…".into(), false),
        Play::Installing { fraction, .. } => (
            "cloud-download",
            format!("Kuruluyor… %{}", (fraction * 100.0).round() as u32).into(),
            false,
        ),
        Play::Starting => ("loader-circle", "Oyun açılıyor…".into(), false),
        Play::Playing => ("gamepad-2", "Oyundasın".into(), false),
    };
    let sign_in = matches!(play, Play::SignIn);
    let (bg, fg) = if enabled {
        (p.accent, p.on_accent)
    } else {
        (p.raised, p.text_secondary)
    };
    let main = div()
        .id("ibsmp-play")
        .flex_1()
        .h(px(56.0))
        .rounded(px(radius::LG))
        .bg(bg)
        .flex()
        .items_center()
        .justify_center()
        .gap(space(2.5))
        .text_size(px(18.0))
        .font_weight(FontWeight::BOLD)
        .text_color(fg)
        .when(!enabled, |d| d.border_1().border_color(p.border))
        .when(enabled, |d| {
            d.cursor_pointer()
                .hover(|s| s.bg(p.accent_hover))
                .active(|s| s.bg(p.accent_pressed))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if sign_in {
                        crate::login::sign_in(cx);
                    } else {
                        this.state().update(cx, |s, cx| s.ibsmp_play(cx));
                    }
                }))
        })
        .child(icon(glyph, 20.0, fg))
        .child(label);
    let mut row = div().w_full().flex().gap(space(2.0)).child(main);
    if let (Play::Playing, Some(id)) = (play, instance) {
        row = row.child(
            div()
                .id("ibsmp-stop")
                .h(px(56.0))
                .px(space(4.0))
                .rounded(px(radius::LG))
                .bg(p.danger_soft)
                .flex()
                .items_center()
                .gap(space(2.0))
                .cursor_pointer()
                .text_color(p.danger)
                .font_weight(FontWeight::SEMIBOLD)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    let id = id.clone();
                    this.state().update(cx, |s, cx| s.stop(&id, cx));
                }))
                .child(icon("square", 16.0, p.danger))
                .child("Kapat"),
        );
    }
    row.into_any_element()
}

fn hint(text: &'static str, cx: &mut Context<Shell>) -> AnyElement {
    div()
        .w_full()
        .text_center()
        .text_size(px(type_scale::SMALL))
        .text_color(cx.palette().text_muted)
        .child(text)
        .into_any_element()
}

/// The backend's install phases, in Turkish.
fn install_phase(phase: &str) -> String {
    let lower = phase.to_ascii_lowercase();
    if lower.contains("java") {
        "Java hazırlanıyor".into()
    } else if lower.contains("mod") || lower.contains("download") || lower.contains("file") {
        "Modlar indiriliyor".into()
    } else if lower.contains("loader") || lower.contains("neoforge") || lower.contains("forge") {
        "NeoForge kuruluyor".into()
    } else if lower.contains("minecraft") || lower.contains("game") || lower.contains("librar") {
        "Minecraft indiriliyor".into()
    } else if lower.is_empty() {
        "Başlıyor".into()
    } else {
        format!("Kuruluyor ({phase})")
    }
}

fn open_instance_folder(id: String, cx: &mut gpui_kit::App) {
    let fetch =
        Backend::global(cx).call(async move { theseus::instance::get_full_path(&id).await });
    cx.spawn(async move |cx| {
        if let Ok(path) = fetch.await {
            cx.update(|cx| cx.open_with_system(&path));
        }
    })
    .detach();
}
