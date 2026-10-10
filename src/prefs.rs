//! User preferences, persisted as `key = value` lines under the XDG config directory.

use std::fmt;
use std::path::PathBuf;

use iced::Font;
use iced::font::{Family, Stretch, Style as FontStyle, Weight};

use crate::highlight::FocusMode;
use crate::music::Music;
use crate::palette::ThemeKind;

pub const MIN_SIZE: f32 = 12.0;
pub const MAX_SIZE: f32 = 40.0;
/// Default body line height, relative to the font size.
pub const LINE_HEIGHT: f32 = 1.55;
pub const MIN_LINE_HEIGHT: f32 = 1.2;
/// The page never goes fully invisible.
pub const MAX_TRANSPARENCY: f32 = 0.9;
pub const MAX_LINE_HEIGHT: f32 = 2.2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FontKind {
    #[default]
    Duo,
    Mono,
    Quattro,
}

impl FontKind {
    pub const ALL: [Self; 3] = [Self::Duo, Self::Mono, Self::Quattro];

    pub fn family(self) -> &'static str {
        match self {
            Self::Duo => "iA Writer Duo S",
            Self::Mono => "iA Writer Mono S",
            Self::Quattro => "iA Writer Quattro S",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Duo => Self::Mono,
            Self::Mono => Self::Quattro,
            Self::Quattro => Self::Duo,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Duo => "Duo",
            Self::Mono => "Mono",
            Self::Quattro => "Quattro",
        }
    }

    pub fn font(self) -> Font {
        Font {
            family: Family::Name(self.family()),
            weight: Weight::Normal,
            stretch: Stretch::Normal,
            style: FontStyle::Normal,
        }
    }
}

impl fmt::Display for FontKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Which set of key samples to play.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SoundSet {
    #[default]
    Soft,
    Keyboard,
}

impl SoundSet {
    pub const ALL: [Self; 2] = [Self::Soft, Self::Keyboard];

    pub fn label(self) -> &'static str {
        match self {
            Self::Soft => "Soft",
            Self::Keyboard => "Keyboard",
        }
    }
}

impl fmt::Display for SoundSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Everything the user can change from the menu.
#[derive(Debug, Clone, PartialEq)]
pub struct Preferences {
    pub theme: ThemeKind,
    pub font: FontKind,
    pub size: f32,
    pub line_height: f32,
    pub focus: FocusMode,
    pub typewriter: bool,
    pub typing_sound: bool,
    pub sound_set: SoundSet,
    /// 0..=1.
    pub sound_volume: f32,
    /// Show the welcome dialog at launch until the user opts out.
    pub show_welcome: bool,
    pub frost: bool,
    /// 0..=0.9: how see-through the page is when frosted.
    pub frost_transparency: f32,
    /// 0..=1: how heavily the backdrop is frosted, where the compositor can.
    pub frost_thickness: f32,
    /// Not saved: the app always starts silent.
    pub music: Music,
    /// 0..=1.
    pub music_volume: f32,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            theme: ThemeKind::Light,
            font: FontKind::Duo,
            size: 18.0,
            line_height: LINE_HEIGHT,
            focus: FocusMode::Off,
            typewriter: false,
            typing_sound: true,
            sound_set: SoundSet::Soft,
            sound_volume: 0.6,
            show_welcome: true,
            frost: false,
            frost_transparency: 0.35,
            frost_thickness: 0.6,
            music: Music::Off,
            music_volume: 0.5,
        }
    }
}

impl Preferences {
    /// `$XDG_CONFIG_HOME/inkpad/preferences`.
    pub fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("inkpad").join("preferences"))
    }

    /// Read the preferences file, falling back to defaults for anything missing or malformed.
    pub fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|text| Self::parse(&text))
            .unwrap_or_default()
    }

    fn parse(text: &str) -> Self {
        let mut prefs = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            let value = value.trim();
            match key.trim() {
                "theme" => prefs.theme = pick(&[ThemeKind::Light, ThemeKind::Sepia, ThemeKind::Dark], value).unwrap_or(prefs.theme),
                "font" => prefs.font = pick(&FontKind::ALL, value).unwrap_or(prefs.font),
                "size" => prefs.size = parse_clamped(value, MIN_SIZE, MAX_SIZE).unwrap_or(prefs.size),
                "line_height" => {
                    prefs.line_height = parse_clamped(value, MIN_LINE_HEIGHT, MAX_LINE_HEIGHT).unwrap_or(prefs.line_height)
                }
                "focus" => {
                    prefs.focus = pick(&[FocusMode::Off, FocusMode::Paragraph, FocusMode::Sentence], value)
                        .unwrap_or(prefs.focus)
                }
                "typewriter" => prefs.typewriter = value == "true",
                "typing_sound" => prefs.typing_sound = value == "true",
                "sound_set" => prefs.sound_set = pick(&SoundSet::ALL, value).unwrap_or(prefs.sound_set),
                "sound_volume" => prefs.sound_volume = parse_clamped(value, 0.0, 1.0).unwrap_or(prefs.sound_volume),
                "music_volume" => prefs.music_volume = parse_clamped(value, 0.0, 1.0).unwrap_or(prefs.music_volume),
                "show_welcome" => prefs.show_welcome = value != "false",
                "frost" => prefs.frost = value == "true",
                "frost_transparency" => prefs.frost_transparency = parse_clamped(value, 0.0, MAX_TRANSPARENCY).unwrap_or(prefs.frost_transparency),
                "frost_thickness" => prefs.frost_thickness = parse_clamped(value, 0.0, 1.0).unwrap_or(prefs.frost_thickness),
                _ => {}
            }
        }
        prefs
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::path().ok_or("no config directory")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, self.serialize()).map_err(|e| e.to_string())
    }

    fn serialize(&self) -> String {
        format!(
            "theme = {}\nfont = {}\nsize = {}\nline_height = {:.2}\nfocus = {}\ntypewriter = {}\ntyping_sound = {}\nsound_set = {}\nsound_volume = {:.2}\nmusic_volume = {:.2}\nshow_welcome = {}\nfrost = {}\nfrost_transparency = {:.2}\nfrost_thickness = {:.2}\n",
            self.theme.label(),
            self.font.label(),
            self.size,
            self.line_height,
            self.focus.label(),
            self.typewriter,
            self.typing_sound,
            self.sound_set.label(),
            self.sound_volume,
            self.music_volume,
            self.show_welcome,
            self.frost,
            self.frost_transparency,
            self.frost_thickness,
        )
    }
}

/// Match `value` against the labels of `options`, case-insensitively.
fn pick<T: Copy + fmt::Display>(options: &[T], value: &str) -> Option<T> {
    options.iter().copied().find(|o| o.to_string().eq_ignore_ascii_case(value))
}

fn parse_clamped(value: &str, min: f32, max: f32) -> Option<f32> {
    let v: f32 = value.parse().ok()?;
    v.is_finite().then(|| v.clamp(min, max))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let prefs = Preferences {
            theme: ThemeKind::Dark,
            font: FontKind::Quattro,
            size: 21.0,
            line_height: 1.8,
            focus: FocusMode::Sentence,
            typewriter: true,
            typing_sound: false,
            sound_set: SoundSet::Keyboard,
            sound_volume: 0.25,
            show_welcome: false,
            frost: true,
            frost_transparency: 0.5,
            frost_thickness: 0.25,
            music: Music::Off,
            music_volume: 0.35,
        };
        assert_eq!(Preferences::parse(&prefs.serialize()), prefs);
    }

    #[test]
    fn malformed_values_fall_back_to_defaults() {
        let text = "theme = neon\nsize = huge\nline_height = 9\nfocus=paragraph\nbogus = 1\n";
        let prefs = Preferences::parse(text);
        assert_eq!(prefs.theme, ThemeKind::Light);
        assert_eq!(prefs.size, 18.0);
        assert_eq!(prefs.line_height, MAX_LINE_HEIGHT);
        assert_eq!(prefs.focus, FocusMode::Paragraph);
    }
}
