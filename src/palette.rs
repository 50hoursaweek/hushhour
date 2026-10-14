//! Light, sepia and dark colour schemes for the page.

use iced::theme::Palette;
use iced::{Color, Theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeKind {
    #[default]
    Light,
    Sepia,
    Dark,
}

impl ThemeKind {
    pub const ALL: [Self; 3] = [Self::Light, Self::Sepia, Self::Dark];

    pub fn next(self) -> Self {
        match self {
            Self::Light => Self::Sepia,
            Self::Sepia => Self::Dark,
            Self::Dark => Self::Light,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Light => "Light",
            Self::Sepia => "Sepia",
            Self::Dark => "Dark",
        }
    }

    pub fn theme(self) -> Theme {
        let (name, palette) = match self {
            Self::Light => (
                "HushHour Light",
                Palette {
                    background: rgb(0xF7, 0xF7, 0xF7),
                    text: rgb(0x1A, 0x1A, 0x1A),
                    primary: rgb(0x1D, 0x99, 0xF3),
                    success: rgb(0x2E, 0x7D, 0x32),
                    warning: rgb(0xED, 0x6C, 0x02),
                    danger: rgb(0xD3, 0x2F, 0x2F),
                },
            ),
            Self::Sepia => (
                "HushHour Sepia",
                Palette {
                    background: rgb(0xF4, 0xEC, 0xD8),
                    text: rgb(0x3B, 0x30, 0x24),
                    primary: rgb(0xB0, 0x6B, 0x2A),
                    success: rgb(0x5B, 0x7A, 0x3A),
                    warning: rgb(0xB8, 0x6E, 0x1E),
                    danger: rgb(0xA8, 0x3A, 0x2E),
                },
            ),
            Self::Dark => (
                "HushHour Dark",
                Palette {
                    background: rgb(0x1B, 0x1B, 0x1B),
                    text: rgb(0xCC, 0xCC, 0xCC),
                    primary: rgb(0x1D, 0x99, 0xF3),
                    success: rgb(0x66, 0xBB, 0x6A),
                    warning: rgb(0xFF, 0xA7, 0x26),
                    danger: rgb(0xEF, 0x53, 0x50),
                },
            ),
        };
        Theme::custom(name, palette)
    }
}

impl std::fmt::Display for ThemeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb8(r, g, b)
}

/// WCAG contrast ratio between two opaque colours (1 to 21).
pub fn contrast(a: Color, b: Color) -> f32 {
    fn luminance(c: Color) -> f32 {
        let lin = |v: f32| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
        0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
    }
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// The colour for links: the theme's accent, darkened in steps of 5% only as far as it
/// takes to read at 4.5:1 on the page (light and sepia need it; dark does not).
pub fn link(p: &iced::theme::Palette) -> Color {
    let black = Color::BLACK;
    (0..=20)
        .map(|step| mix(p.primary, black, step as f32 * 0.05))
        .find(|c| contrast(*c, p.background) >= 4.5)
        .unwrap_or(p.text)
}

/// Blend `fg` towards `bg` by `t` (0 = fg, 1 = bg). Used for dimmed text and markers.
pub fn mix(fg: Color, bg: Color, t: f32) -> Color {
    Color {
        r: fg.r + (bg.r - fg.r) * t,
        g: fg.g + (bg.g - fg.g) * t,
        b: fg.b + (bg.b - fg.b) * t,
        a: 1.0,
    }
}
