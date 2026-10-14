//! The iced side of highlighting: spotlight dimming (everything but the current
//! paragraph or sentence is faded) and fonts and colours for the spans that the
//! shared scanner in `hushhour_core::markdown` produces.

use std::ops::Range;

use iced::advanced::text::highlighter::{Format, Highlighter};
use iced::font::{Family, Stretch, Style as FontStyle, Weight};
use iced::{Font, Theme};

use crate::palette::mix;
pub use hushhour_core::markdown::{Kind, sentence_at};
use hushhour_core::markdown::{Scanner, split_at};

pub const MONO_FAMILY: &str = "IBM Plex Mono";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FocusMode {
    #[default]
    Off,
    Paragraph,
    Sentence,
}

impl FocusMode {
    pub const ALL: [Self; 3] = [Self::Off, Self::Paragraph, Self::Sentence];

    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::Paragraph,
            Self::Paragraph => Self::Sentence,
            Self::Sentence => Self::Off,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Paragraph => "Paragraph",
            Self::Sentence => "Sentence",
        }
    }
}

impl std::fmt::Display for FocusMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// Everything the highlighter needs to know about the editor state.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub family: &'static str,
    pub focus: FocusMode,
    /// Lines belonging to the paragraph under the cursor.
    pub paragraph: Range<usize>,
    pub cursor_line: usize,
    /// Byte range of the sentence under the cursor, within `cursor_line`.
    pub sentence: Range<usize>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            family: "IBM Plex Mono",
            focus: FocusMode::Off,
            paragraph: 0..1,
            cursor_line: 0,
            sentence: 0..0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Highlight {
    pub kind: Kind,
    pub dim: bool,
    pub family: &'static str,
}

/// iced's view of the shared scanner: spans from `hushhour_core::markdown`, dimmed
/// according to the spotlight settings.
pub struct MarkdownHighlighter {
    settings: Settings,
    scanner: Scanner,
}

impl Highlighter for MarkdownHighlighter {
    type Settings = Settings;
    type Highlight = Highlight;
    type Iterator<'a> = std::vec::IntoIter<(Range<usize>, Highlight)>;

    fn new(settings: &Self::Settings) -> Self {
        Self { settings: settings.clone(), scanner: Scanner::new() }
    }

    fn update(&mut self, new_settings: &Self::Settings) {
        self.settings = new_settings.clone();
        self.change_line(0);
    }

    fn change_line(&mut self, line: usize) {
        self.scanner.change_line(line);
    }

    fn highlight_line(&mut self, line: &str) -> Self::Iterator<'_> {
        let index = self.scanner.current_line();
        let mut spans = self.scanner.scan(line);

        let dim_whole = match self.settings.focus {
            FocusMode::Off => false,
            FocusMode::Paragraph => !self.settings.paragraph.contains(&index),
            FocusMode::Sentence => index != self.settings.cursor_line,
        };

        if self.settings.focus == FocusMode::Sentence && index == self.settings.cursor_line {
            let s = within_line(&self.settings.sentence, line);
            spans = split_at(spans, &[s.start, s.end]);
            let family = self.settings.family;
            return spans
                .into_iter()
                .map(|(r, kind)| {
                    let inside = r.start >= s.start && r.end <= s.end;
                    (r, Highlight { kind, dim: !inside, family })
                })
                .collect::<Vec<_>>()
                .into_iter();
        }

        let family = self.settings.family;
        spans
            .into_iter()
            .map(|(r, kind)| (r, Highlight { kind, dim: dim_whole, family }))
            .collect::<Vec<_>>()
            .into_iter()
    }

    fn current_line(&self) -> usize {
        self.scanner.current_line()
    }
}

/// Map a highlight to colour and font. Passed to `TextEditor::highlight_with`.
pub fn to_format(h: &Highlight, theme: &Theme) -> Format<Font> {
    let p = theme.palette();
    let (bg, fg) = (p.background, p.text);

    let base = match h.kind {
        Kind::Marker | Kind::ListMarker => mix(fg, bg, 0.55),
        Kind::Url => mix(fg, bg, 0.45),
        Kind::Link => crate::palette::link(&p),
        _ => fg,
    };
    let color = if h.dim { mix(base, bg, 0.72) } else { base };

    let family = match h.kind {
        Kind::Code | Kind::CodeBlock => Family::Name(MONO_FAMILY),
        _ => Family::Name(h.family),
    };
    let weight = match h.kind {
        Kind::Heading | Kind::Strong | Kind::StrongEmphasis => Weight::Bold,
        _ => Weight::Normal,
    };
    let style = match h.kind {
        Kind::Emphasis | Kind::StrongEmphasis => FontStyle::Italic,
        _ => FontStyle::Normal,
    };

    Format {
        color: Some(color),
        font: Some(Font { family, weight, stretch: Stretch::Normal, style }),
    }
}


/// The sentence range made safe for this line: inside it, in order, and on character
/// boundaries. Every edit refreshes the range before the next frame, so it should already
/// be all three; a stale one (the iPad crash of build 72) must not reach the renderer,
/// where a cut inside a multibyte character panics.
fn within_line(range: &Range<usize>, line: &str) -> Range<usize> {
    let floor = |i: usize| {
        let mut i = i.min(line.len());
        while !line.is_char_boundary(i) {
            i -= 1;
        }
        i
    };
    let (a, b) = (floor(range.start), floor(range.end));
    a.min(b)..a.max(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sentence spotlight: only the sentence under the cursor stays undimmed, and the
    /// spans are cut at its edges.
    #[test]
    fn sentence_focus_dims_outside_the_sentence() {
        let line = "First one. Second here! Third?";
        let settings = Settings { focus: FocusMode::Sentence, cursor_line: 0, sentence: sentence_at(line, 12), ..Settings::default() };
        let mut h = MarkdownHighlighter::new(&settings);
        let spans: Vec<_> = h.highlight_line(line).collect();
        let lit: Vec<_> = spans.iter().filter(|(_, hl)| !hl.dim).map(|(r, _)| r.clone()).collect();
        // The lit spans cover exactly the sentence (possibly in several pieces: "!" can start a link).
        assert_eq!((lit.first().map(|r| r.start), lit.last().map(|r| r.end)), (Some(11), Some(23)));
        assert!(lit.windows(2).all(|w| w[0].end == w[1].start));
        assert!(spans.iter().all(|(r, hl)| hl.dim || (r.start >= 11 && r.end <= 23)));
    }

    /// A stale sentence range (cut inside "’", reversed, past the end) never yields a span
    /// that splits a character or leaves the line.
    #[test]
    fn stale_sentence_range_stays_on_char_boundaries() {
        let line = "I don’t know."; // "’" is bytes 5..8
        let reversed = Range { start: 7, end: 4 };
        for sentence in [6..9, reversed.clone(), 0..99, 7..7] {
            let settings = Settings { focus: FocusMode::Sentence, cursor_line: 0, sentence, ..Settings::default() };
            let mut h = MarkdownHighlighter::new(&settings);
            for (r, _) in h.highlight_line(line) {
                assert!(r.start <= r.end && r.end <= line.len(), "{r:?}");
                assert!(line.is_char_boundary(r.start) && line.is_char_boundary(r.end), "{r:?} splits a character");
            }
        }
        assert_eq!(within_line(&reversed, line), 4..5, "floored and ordered");
        assert_eq!(within_line(&(6..99), line), 5..line.len(), "floored and clamped");
    }
}
