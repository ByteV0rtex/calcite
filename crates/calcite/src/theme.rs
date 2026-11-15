//! Calcite's design tokens.
//!
//! One neutral palette per appearance, one accent, and a handful of state
//! colors. Everything the UI paints comes from here; components never invent
//! colors of their own. The same palette is also projected onto
//! gpui-component's theme so stock controls (inputs, menus, dialogs) match.

use std::rc::Rc;

use gpui_kit::component::{Theme, ThemeConfig, ThemeMode};
use gpui_kit::{App, Global, Hsla, Pixels, Rgba, px, rgb, rgba};
use serde_json::json;

/// Light, dark, or true-black.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Appearance {
    Light,
    #[default]
    Dark,
    Oled,
}

impl Appearance {
    pub fn is_dark(self) -> bool {
        !matches!(self, Appearance::Light)
    }
}

/// The curated accent set. Honey is Calcite's own; the rest are for people
/// who want their launcher to feel like theirs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Accent {
    #[default]
    Honey,
    Jade,
    Lapis,
    Amethyst,
    Coral,
    Rose,
    Teal,
    Slate,
}

impl Accent {
    pub const ALL: [Accent; 8] = [
        Accent::Honey,
        Accent::Jade,
        Accent::Lapis,
        Accent::Amethyst,
        Accent::Coral,
        Accent::Rose,
        Accent::Teal,
        Accent::Slate,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Accent::Honey => "Honey",
            Accent::Jade => "Jade",
            Accent::Lapis => "Lapis",
            Accent::Amethyst => "Amethyst",
            Accent::Coral => "Coral",
            Accent::Rose => "Rose",
            Accent::Teal => "Teal",
            Accent::Slate => "Slate",
        }
    }

    /// (on dark backgrounds, on light backgrounds)
    fn hex(self) -> (u32, u32) {
        match self {
            Accent::Honey => (0xE6A23C, 0xB4711A),
            Accent::Jade => (0x3FBF8F, 0x1E8A62),
            Accent::Lapis => (0x6B9AF2, 0x2F62C9),
            Accent::Amethyst => (0xA98AF0, 0x7350C9),
            Accent::Coral => (0xEF7A66, 0xC24A36),
            Accent::Rose => (0xE874AC, 0xB83E7A),
            Accent::Teal => (0x3DBBC8, 0x1A8592),
            Accent::Slate => (0xC2C6CE, 0x4F5560),
        }
    }
}

/// Every color the app paints, resolved for the current appearance + accent.
#[derive(Clone, Debug)]
pub struct Palette {
    pub appearance: Appearance,
    pub accent_kind: Accent,

    // Surfaces, from the back of the window to the front.
    pub rail: Hsla,
    pub app: Hsla,
    pub surface: Hsla,
    pub raised: Hsla,
    pub hover: Hsla,
    pub pressed: Hsla,
    pub overlay: Hsla,
    pub scrim: Hsla,

    // Lines.
    pub border: Hsla,
    pub border_strong: Hsla,

    // Text.
    pub text: Hsla,
    pub text_secondary: Hsla,
    pub text_muted: Hsla,
    pub text_disabled: Hsla,

    // Accent.
    pub accent: Hsla,
    pub accent_hover: Hsla,
    pub accent_pressed: Hsla,
    pub accent_soft: Hsla,
    pub accent_text: Hsla,
    pub on_accent: Hsla,

    // State. Used only to communicate state, never for decoration.
    pub running: Hsla,
    pub running_soft: Hsla,
    pub danger: Hsla,
    pub danger_soft: Hsla,
    pub warning: Hsla,
    pub warning_soft: Hsla,
    pub info: Hsla,
    pub info_soft: Hsla,

    pub skeleton: Hsla,
    pub focus_ring: Hsla,
}

impl Global for Palette {}

fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

fn ca(hex_rgba: u32) -> Hsla {
    rgba(hex_rgba).into()
}

fn with_alpha(color: Hsla, alpha: f32) -> Hsla {
    Hsla { a: alpha, ..color }
}

fn hex_string(color: Hsla) -> String {
    let rgba: Rgba = color.into();
    let to = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}{:02x}",
        to(rgba.r),
        to(rgba.g),
        to(rgba.b),
        to(rgba.a)
    )
}

/// Lighten (positive) or darken (negative) a color in HSL space.
fn shift(color: Hsla, amount: f32) -> Hsla {
    Hsla {
        l: (color.l + amount).clamp(0.0, 1.0),
        ..color
    }
}

impl Palette {
    pub fn new(appearance: Appearance, accent_kind: Accent) -> Self {
        let (accent_dark, accent_light) = accent_kind.hex();
        match appearance {
            Appearance::Dark | Appearance::Oled => {
                let oled = appearance == Appearance::Oled;
                let accent = c(accent_dark);
                Palette {
                    appearance,
                    accent_kind,
                    rail: if oled { c(0x000000) } else { c(0x0F1013) },
                    app: if oled { c(0x000000) } else { c(0x141518) },
                    surface: if oled { c(0x0B0B0D) } else { c(0x1A1B1F) },
                    raised: if oled { c(0x141416) } else { c(0x212328) },
                    hover: if oled { c(0x18181B) } else { c(0x26282E) },
                    pressed: if oled { c(0x1F1F23) } else { c(0x2C2F35) },
                    overlay: if oled { c(0x101012) } else { c(0x1E2025) },
                    scrim: ca(0x00000099),
                    border: if oled { c(0x1C1C20) } else { c(0x26282D) },
                    border_strong: if oled { c(0x2A2A30) } else { c(0x34373E) },
                    text: c(0xECEAE6),
                    text_secondary: c(0xA9ACB3),
                    text_muted: c(0x74787F),
                    text_disabled: c(0x4E5157),
                    accent,
                    accent_hover: shift(accent, 0.06),
                    accent_pressed: shift(accent, -0.06),
                    accent_soft: with_alpha(accent, 0.14),
                    accent_text: shift(accent, 0.08),
                    on_accent: c(0x17120A),
                    running: c(0x4CC38A),
                    running_soft: ca(0x4CC38A24),
                    danger: c(0xEB5E55),
                    danger_soft: ca(0xEB5E5524),
                    warning: c(0xF08A4B),
                    warning_soft: ca(0xF08A4B24),
                    info: c(0x5B9DFF),
                    info_soft: ca(0x5B9DFF24),
                    skeleton: if oled { c(0x141416) } else { c(0x1F2125) },
                    focus_ring: with_alpha(accent, 0.55),
                }
            }
            Appearance::Light => {
                let accent = c(accent_light);
                Palette {
                    appearance,
                    accent_kind,
                    rail: c(0xECE9E3),
                    app: c(0xF5F3EF),
                    surface: c(0xFFFFFF),
                    raised: c(0xF0EDE8),
                    hover: c(0xE9E5DF),
                    pressed: c(0xE1DCD5),
                    overlay: c(0xFFFFFF),
                    scrim: ca(0x1C1A1766),
                    border: c(0xE2DED7),
                    border_strong: c(0xD0CBC2),
                    text: c(0x1C1A17),
                    text_secondary: c(0x57534C),
                    text_muted: c(0x8A857D),
                    text_disabled: c(0xB5B0A8),
                    accent,
                    accent_hover: shift(accent, -0.05),
                    accent_pressed: shift(accent, -0.10),
                    accent_soft: with_alpha(accent, 0.12),
                    accent_text: shift(accent, -0.06),
                    on_accent: c(0xFFFFFF),
                    running: c(0x1F9A62),
                    running_soft: ca(0x1F9A621F),
                    danger: c(0xC93A31),
                    danger_soft: ca(0xC93A311A),
                    warning: c(0xC8611F),
                    warning_soft: ca(0xC8611F1A),
                    info: c(0x2F6FD1),
                    info_soft: ca(0x2F6FD11A),
                    skeleton: c(0xEAE6E0),
                    focus_ring: with_alpha(accent, 0.45),
                }
            }
        }
    }

    pub fn is_dark(&self) -> bool {
        self.appearance.is_dark()
    }

    /// A gpui-component theme config built from this palette, so stock
    /// controls use exactly the same colors as Calcite's own components.
    fn component_theme(&self) -> ThemeConfig {
        let h = hex_string;
        let mode = if self.is_dark() { "dark" } else { "light" };
        let value = json!({
            "name": format!("Calcite {}", mode),
            "mode": mode,
            "font.family": fonts::UI,
            "font.size": type_scale::BODY,
            "mono_font.family": fonts::MONO,
            "mono_font.size": 12.0,
            "radius": radius::MD as usize,
            "radius.lg": radius::LG as usize,
            "shadow": false,
            "colors": {
                "background": h(self.app),
                "foreground": h(self.text),
                "border": h(self.border),
                "accent.background": h(self.hover),
                "accent.foreground": h(self.text),
                "muted.background": h(self.raised),
                "muted.foreground": h(self.text_muted),
                "primary.background": h(self.accent),
                "primary.hover.background": h(self.accent_hover),
                "primary.active.background": h(self.accent_pressed),
                "primary.foreground": h(self.on_accent),
                "secondary.background": h(self.raised),
                "secondary.hover.background": h(self.hover),
                "secondary.active.background": h(self.pressed),
                "secondary.foreground": h(self.text),
                "danger.background": h(self.danger),
                "danger.foreground": h(self.on_accent),
                "success.background": h(self.running),
                "success.foreground": h(self.on_accent),
                "warning.background": h(self.warning),
                "warning.foreground": h(self.on_accent),
                "info.background": h(self.info),
                "info.foreground": h(self.on_accent),
                "input.border": h(self.border_strong),
                "ring": h(self.focus_ring),
                "caret": h(self.accent),
                "selection.background": h(with_alpha(self.accent, 0.35)),
                "link.foreground": h(self.accent_text),
                "link.hover.foreground": h(self.accent_hover),
                "link.active.foreground": h(self.accent_pressed),
                "popover.background": h(self.overlay),
                "popover.foreground": h(self.text),
                "overlay": h(self.scrim),
                "list.background": h(self.surface),
                "list.hover.background": h(self.hover),
                "list.active.background": h(self.accent_soft),
                "list.active.border": h(self.accent),
                "list.even.background": h(self.surface),
                "list.head.background": h(self.surface),
                "table.background": h(self.surface),
                "table.head.foreground": h(self.text_muted),
                "table.row.border": h(self.border),
                "tab.background": "#00000000",
                "tab.foreground": h(self.text_secondary),
                "tab.active.background": h(self.raised),
                "tab.active.foreground": h(self.text),
                "tab_bar.background": "#00000000",
                "tab_bar.segmented.background": h(self.surface),
                "scrollbar.background": "#00000000",
                "scrollbar.thumb.background": h(with_alpha(self.text_muted, 0.45)),
                "scrollbar.thumb.hover.background": h(with_alpha(self.text_muted, 0.7)),
                "skeleton.background": h(self.skeleton),
                "switch.background": h(self.border_strong),
                "slider.bar.background": h(self.accent),
                "slider.thumb.background": h(self.text),
                "progress_bar.background": h(self.accent),
                "sidebar.background": h(self.rail),
                "sidebar.foreground": h(self.text_secondary),
                "sidebar.border": h(self.border),
                "sidebar.accent.background": h(self.hover),
                "sidebar.accent.foreground": h(self.text),
                "sidebar.primary.background": h(self.accent),
                "sidebar.primary.foreground": h(self.on_accent),
                "title_bar.background": h(self.app),
                "title_bar.border": h(self.border),
                "window.border": h(self.border_strong),
                "group_box.background": h(self.surface),
                "group_box.foreground": h(self.text),
                "accordion.background": h(self.surface),
                "description_list_label.foreground": h(self.text_secondary),
                "drag_border": h(self.accent),
                "drop_target.background": h(self.accent_soft),
            }
        });
        serde_json::from_value(value).expect("Calcite theme config is valid")
    }
}

pub mod fonts {
    pub const UI: &str = "Inter";
    pub const MONO: &str = "JetBrains Mono";
}

/// The only font sizes the app uses.
pub mod type_scale {
    pub const CAPTION: f32 = 11.0;
    pub const SMALL: f32 = 12.0;
    pub const BODY: f32 = 13.0;
    pub const BODY_LG: f32 = 14.0;
    pub const SECTION: f32 = 16.0;
    pub const TITLE: f32 = 20.0;
    pub const HEADLINE: f32 = 24.0;
    pub const DISPLAY: f32 = 32.0;
}

/// Corner radii.
pub mod radius {
    pub const SM: f32 = 6.0;
    pub const MD: f32 = 8.0;
    pub const LG: f32 = 10.0;
    pub const XL: f32 = 14.0;
}

/// 4px spacing grid.
pub fn space(steps: f32) -> Pixels {
    px(steps * 4.0)
}

/// Install the palette and project it onto gpui-component's theme.
pub fn apply(appearance: Appearance, accent: Accent, cx: &mut App) {
    let palette = Palette::new(appearance, accent);
    let config = Rc::new(palette.component_theme());
    let mode = if palette.is_dark() {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    cx.set_global(palette);
    Theme::update(cx, |theme| {
        if mode.is_dark() {
            theme.dark_theme = config.clone();
        } else {
            theme.light_theme = config.clone();
        }
    });
    Theme::change(mode, None, cx);
    cx.refresh_windows();
}

/// Shorthand for reading the palette from any context that derefs to `App`.
pub trait PaletteExt {
    fn palette(&self) -> &Palette;
}

impl PaletteExt for App {
    fn palette(&self) -> &Palette {
        self.global::<Palette>()
    }
}
