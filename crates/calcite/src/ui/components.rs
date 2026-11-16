//! Calcite's own building blocks. Small, stateless, and painted only from
//! the palette so every screen reads as one system.

use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, ElementId, FontWeight, Hsla, ImageSource, ObjectFit, Pixels,
    SharedString, Window, div, img, px, relative, svg,
};

use crate::assets;
use crate::theme::{Palette, PaletteExt, radius, space, type_scale};

// ---------------------------------------------------------------- icons

/// A Lucide icon from the embedded set, tinted with `color`.
pub fn icon(name: &str, size: f32, color: Hsla) -> impl IntoElement {
    svg()
        .path(assets::icon(name))
        .size(px(size))
        .flex_none()
        .text_color(color)
}

// ---------------------------------------------------------------- text

/// The muted, uppercase-free section heading used above lists.
pub fn section_heading(text: impl Into<SharedString>, cx: &App) -> gpui_kit::Div {
    let p = cx.palette();
    div()
        .text_size(px(type_scale::SMALL))
        .font_weight(FontWeight::MEDIUM)
        .text_color(p.text_muted)
        .child(text.into())
}

// ---------------------------------------------------------------- buttons

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ButtonKind {
    /// The one most important action on a screen (Play, Install).
    Primary,
    /// Everything else that's a real button.
    #[default]
    Secondary,
    /// Low-emphasis action that only shows its surface on hover.
    Ghost,
    /// Destructive.
    Danger,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ButtonSize {
    Small,
    #[default]
    Medium,
    Large,
}

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// A text button with an optional leading icon.
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: Option<SharedString>,
    icon: Option<&'static str>,
    kind: ButtonKind,
    size: ButtonSize,
    disabled: bool,
    on_click: Option<ClickHandler>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Button {
            id: id.into(),
            label: None,
            icon: None,
            kind: ButtonKind::Secondary,
            size: ButtonSize::Medium,
            disabled: false,
            on_click: None,
        }
    }
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }
    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }
    pub fn kind(mut self, kind: ButtonKind) -> Self {
        self.kind = kind;
        self
    }
    pub fn primary(self) -> Self {
        self.kind(ButtonKind::Primary)
    }
    pub fn ghost(self) -> Self {
        self.kind(ButtonKind::Ghost)
    }
    pub fn danger(self) -> Self {
        self.kind(ButtonKind::Danger)
    }
    pub fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }
    pub fn small(self) -> Self {
        self.size(ButtonSize::Small)
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(f));
        self
    }
}

struct ButtonColors {
    bg: Hsla,
    hover: Hsla,
    active: Hsla,
    fg: Hsla,
    border: Option<Hsla>,
}

fn button_colors(kind: ButtonKind, p: &Palette) -> ButtonColors {
    let transparent = Hsla::transparent_black();
    match kind {
        ButtonKind::Primary => ButtonColors {
            bg: p.accent,
            hover: p.accent_hover,
            active: p.accent_pressed,
            fg: p.on_accent,
            border: None,
        },
        ButtonKind::Secondary => ButtonColors {
            bg: p.raised,
            hover: p.hover,
            active: p.pressed,
            fg: p.text,
            border: Some(p.border),
        },
        ButtonKind::Ghost => ButtonColors {
            bg: transparent,
            hover: p.hover,
            active: p.pressed,
            fg: p.text_secondary,
            border: None,
        },
        ButtonKind::Danger => ButtonColors {
            bg: p.danger_soft,
            hover: Hsla {
                a: 0.24,
                ..p.danger
            },
            active: Hsla {
                a: 0.32,
                ..p.danger
            },
            fg: p.danger,
            border: None,
        },
    }
}

impl RenderOnce for Button {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = cx.palette().clone();
        let colors = button_colors(self.kind, &p);
        let (height, pad_x, text_size, icon_size) = match self.size {
            ButtonSize::Small => (28.0, 10.0, type_scale::SMALL, 14.0),
            ButtonSize::Medium => (32.0, 12.0, type_scale::BODY, 16.0),
            ButtonSize::Large => (40.0, 18.0, type_scale::BODY_LG, 18.0),
        };
        let icon_only = self.label.is_none();
        let fg = if self.disabled {
            p.text_disabled
        } else {
            colors.fg
        };

        let mut el = div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(space(1.5))
            .h(px(height))
            .when(icon_only, |d| d.w(px(height)))
            .when(!icon_only, |d| d.px(px(pad_x)))
            .rounded(px(radius::MD))
            .bg(colors.bg)
            .text_size(px(text_size))
            .font_weight(FontWeight::MEDIUM)
            .text_color(fg)
            .when_some(colors.border, |d, b| d.border_1().border_color(b))
            .when_some(self.icon, |d, name| d.child(icon(name, icon_size, fg)))
            .when_some(self.label, |d, l| d.child(l));

        if self.disabled {
            el = el.opacity(0.6).cursor_not_allowed();
        } else {
            el = el
                .cursor_pointer()
                .hover(|s| s.bg(colors.hover))
                .active(|s| s.bg(colors.active));
            if let Some(handler) = self.on_click {
                el = el.on_click(move |e, w, cx| handler(e, w, cx));
            }
        }
        el
    }
}

// ---------------------------------------------------------------- chips

/// A small, quiet pill for metadata: loader + version, categories, counts.
pub fn chip(text: impl Into<SharedString>, cx: &App) -> gpui_kit::Div {
    let p = cx.palette();
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(space(1.0))
        .h(px(20.0))
        .px(space(1.75))
        .rounded(px(radius::SM))
        .bg(p.raised)
        .text_size(px(type_scale::CAPTION))
        .font_weight(FontWeight::MEDIUM)
        .text_color(p.text_secondary)
        .child(text.into())
}

/// A chip with a leading icon.
pub fn icon_chip(icon_name: &str, text: impl Into<SharedString>, cx: &App) -> gpui_kit::Div {
    let p = cx.palette();
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(space(1.25))
        .h(px(20.0))
        .px(space(1.75))
        .rounded(px(radius::SM))
        .bg(p.raised)
        .text_size(px(type_scale::CAPTION))
        .font_weight(FontWeight::MEDIUM)
        .text_color(p.text_secondary)
        .child(icon(icon_name, 12.0, p.text_muted))
        .child(text.into())
}

/// A chip with a tiny bar sparkline: the live game monitor's readout.
/// `values` are oldest first; bars scale to `max`.
pub fn sparkline_chip(
    icon_name: &str,
    text: impl Into<SharedString>,
    values: &[f32],
    max: f32,
    cx: &App,
) -> gpui_kit::Div {
    const BARS: usize = 30;
    const HEIGHT: f32 = 12.0;
    let p = cx.palette();
    let max = max.max(f32::EPSILON);
    let start = values.len().saturating_sub(BARS);
    let shown = &values[start..];
    let empty = BARS - shown.len();
    let bars = (0..empty)
        .map(|_| 0.0)
        .chain(shown.iter().copied())
        .map(|v| {
            let h = (v / max).clamp(0.0, 1.0) * HEIGHT;
            div()
                .w(px(2.0))
                .h(px(h.max(1.0)))
                .rounded(px(1.0))
                .bg(if v > 0.0 { p.accent } else { p.border })
        });
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(space(1.5))
        .h(px(20.0))
        .px(space(1.75))
        .rounded(px(radius::SM))
        .bg(p.raised)
        .text_size(px(type_scale::CAPTION))
        .font_weight(FontWeight::MEDIUM)
        .text_color(p.text_secondary)
        .child(icon(icon_name, 12.0, p.text_muted))
        .child(div().min_w(px(44.0)).child(text.into()))
        .child(
            div()
                .flex()
                .items_end()
                .gap(px(1.0))
                .h(px(HEIGHT))
                .children(bars),
        )
}

/// A tinted chip communicating state (running, update available, issue).
pub fn state_chip(text: impl Into<SharedString>, fg: Hsla, bg: Hsla) -> gpui_kit::Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(space(1.5))
        .h(px(20.0))
        .px(space(2.0))
        .rounded(px(radius::SM))
        .bg(bg)
        .text_size(px(type_scale::CAPTION))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(fg)
        .child(div().size(px(6.0)).rounded_full().bg(fg))
        .child(text.into())
}

// ---------------------------------------------------------------- media

/// A square project / instance icon. Falls back to a monogram tile whose
/// tint is derived from the name, so lists stay scannable without art.
pub fn thumb(
    source: Option<ImageSource>,
    name: &str,
    size: f32,
    corner: f32,
    cx: &App,
) -> AnyElement {
    let p = cx.palette();
    match source {
        Some(src) => div()
            .size(px(size))
            .flex_none()
            .rounded(px(corner))
            .overflow_hidden()
            .bg(p.raised)
            .child(
                img(src)
                    .size_full()
                    .object_fit(ObjectFit::Cover)
                    .rounded(px(corner)),
            )
            .into_any_element(),
        None => {
            let hue = name_hue(name);
            let (bg, fg) = if p.is_dark() {
                (
                    Hsla {
                        h: hue,
                        s: 0.22,
                        l: 0.20,
                        a: 1.0,
                    },
                    Hsla {
                        h: hue,
                        s: 0.35,
                        l: 0.72,
                        a: 1.0,
                    },
                )
            } else {
                (
                    Hsla {
                        h: hue,
                        s: 0.30,
                        l: 0.88,
                        a: 1.0,
                    },
                    Hsla {
                        h: hue,
                        s: 0.40,
                        l: 0.32,
                        a: 1.0,
                    },
                )
            };
            let initial: String = name
                .chars()
                .find(|c| c.is_alphanumeric())
                .map(|c| c.to_uppercase().collect())
                .unwrap_or_else(|| "?".into());
            div()
                .size(px(size))
                .flex_none()
                .rounded(px(corner))
                .bg(bg)
                .flex()
                .items_center()
                .justify_center()
                .text_size(px((size * 0.42).round()))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(fg)
                .child(initial)
                .into_any_element()
        }
    }
}

fn name_hue(name: &str) -> f32 {
    // FNV-1a: stable across runs and platforms.
    let mut hash: u32 = 0x811c9dc5;
    for b in name.bytes() {
        hash ^= b as u32;
        hash = hash.wrapping_mul(0x01000193);
    }
    (hash % 360) as f32 / 360.0
}

// ---------------------------------------------------------------- loading

/// A skeleton block shaped like the content it stands in for.
pub fn skeleton(w: impl Into<gpui_kit::Length>, h: Pixels, cx: &App) -> gpui_kit::Div {
    div()
        .w(w.into())
        .h(h)
        .flex_none()
        .rounded(px(radius::SM))
        .bg(cx.palette().skeleton)
}

/// A thin determinate progress bar.
pub fn progress_bar(fraction: f32, cx: &App) -> gpui_kit::Div {
    let p = cx.palette();
    div().w_full().h(px(4.0)).rounded_full().bg(p.raised).child(
        div()
            .h_full()
            .rounded_full()
            .bg(p.accent)
            .w(relative(fraction.clamp(0.0, 1.0))),
    )
}

// ---------------------------------------------------------------- layout

/// A bordered surface used for panels and cards.
pub fn panel(cx: &App) -> gpui_kit::Div {
    let p = cx.palette();
    div()
        .bg(p.surface)
        .border_1()
        .border_color(p.border)
        .rounded(px(radius::LG))
}

/// An empty state that always offers a way forward.
pub fn empty_state(
    icon_name: &str,
    title: impl Into<SharedString>,
    body: impl Into<SharedString>,
    action: Option<AnyElement>,
    cx: &App,
) -> gpui_kit::Div {
    let p = cx.palette();
    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(space(3.0))
        .py(space(16.0))
        .child(
            div()
                .size(px(44.0))
                .rounded(px(radius::LG))
                .bg(p.raised)
                .flex()
                .items_center()
                .justify_center()
                .child(icon(icon_name, 20.0, p.text_muted)),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(space(1.0))
                .child(
                    div()
                        .text_size(px(type_scale::BODY_LG))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(p.text)
                        .child(title.into()),
                )
                .child(
                    div()
                        .max_w(px(360.0))
                        .text_center()
                        .text_size(px(type_scale::BODY))
                        .text_color(p.text_secondary)
                        .child(body.into()),
                ),
        )
        .when_some(action, |d, a| d.child(a))
}

/// Format a large count the way players read it: 1,204 → "1.2k", 3_400_000 → "3.4M".
pub fn compact_number(n: u64) -> String {
    fn one_decimal(v: f64, suffix: &str) -> String {
        let s = format!("{v:.1}");
        let s = s.strip_suffix(".0").unwrap_or(&s);
        format!("{s}{suffix}")
    }
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => one_decimal(n as f64 / 1_000.0, "k"),
        1_000_000..=999_999_999 => one_decimal(n as f64 / 1_000_000.0, "M"),
        _ => one_decimal(n as f64 / 1_000_000_000.0, "B"),
    }
}

/// "3h 12m", "45m", "just now".
pub fn short_duration(seconds: u64) -> String {
    let h = seconds / 3600;
    let m = (seconds % 3600) / 60;
    match (h, m) {
        (0, 0) => "<1m".into(),
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

/// "just now", "12 minutes ago", "yesterday", "3 days ago", "Mar 4".
pub fn relative_time(when: chrono::DateTime<chrono::Utc>) -> String {
    relative_time_from(when, chrono::Utc::now())
}

fn relative_time_from(
    when: chrono::DateTime<chrono::Utc>,
    now: chrono::DateTime<chrono::Utc>,
) -> String {
    let secs = (now - when).num_seconds().max(0);
    let mins = secs / 60;
    let hours = mins / 60;
    let days = hours / 24;
    match (mins, hours, days) {
        (0, _, _) => "just now".into(),
        (1, _, _) => "a minute ago".into(),
        (m, 0, _) => format!("{m} minutes ago"),
        (_, 1, _) => "an hour ago".into(),
        (_, h, 0) => format!("{h} hours ago"),
        (_, _, 1) => "yesterday".into(),
        (_, _, d) if d < 7 => format!("{d} days ago"),
        (_, _, d) if d < 14 => "last week".into(),
        (_, _, d) if d < 30 => format!("{} weeks ago", d / 7),
        _ => when
            .with_timezone(&chrono::Local)
            .format("%b %-d, %Y")
            .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_numbers_read_naturally() {
        assert_eq!(compact_number(0), "0");
        assert_eq!(compact_number(999), "999");
        assert_eq!(compact_number(1_000), "1k");
        assert_eq!(compact_number(1_204), "1.2k");
        assert_eq!(compact_number(3_400_000), "3.4M");
        assert_eq!(compact_number(12_000_000_000), "12B");
    }

    #[test]
    fn durations_are_short() {
        assert_eq!(short_duration(30), "<1m");
        assert_eq!(short_duration(45 * 60), "45m");
        assert_eq!(short_duration(3 * 3600), "3h");
        assert_eq!(short_duration(3 * 3600 + 12 * 60), "3h 12m");
    }

    #[test]
    fn relative_times_read_naturally() {
        use chrono::{Duration, TimeZone, Utc};
        let now = Utc.with_ymd_and_hms(2026, 11, 15, 12, 0, 0).unwrap();
        assert_eq!(relative_time_from(now, now), "just now");
        assert_eq!(
            relative_time_from(now - Duration::minutes(12), now),
            "12 minutes ago"
        );
        assert_eq!(
            relative_time_from(now - Duration::hours(3), now),
            "3 hours ago"
        );
        assert_eq!(
            relative_time_from(now - Duration::hours(30), now),
            "yesterday"
        );
        assert_eq!(
            relative_time_from(now - Duration::days(4), now),
            "4 days ago"
        );
        assert_eq!(
            relative_time_from(now - Duration::days(9), now),
            "last week"
        );
        assert_eq!(
            relative_time_from(now - Duration::days(20), now),
            "2 weeks ago"
        );
    }

    #[test]
    fn monogram_hue_is_stable() {
        assert_eq!(
            name_hue("Fabulously Optimized"),
            name_hue("Fabulously Optimized")
        );
        assert_ne!(name_hue("Create"), name_hue("Better PvP"));
    }
}
