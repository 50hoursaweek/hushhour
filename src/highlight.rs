//! Line-based Markdown highlighter with spotlight dimming (everything but the
//! current paragraph or sentence is faded).
//!
//! iced feeds the highlighter one logical line at a time, in order, starting
//! from the first changed line. We keep just enough state (fenced code blocks)
//! to style multi-line constructs.

use std::ops::Range;

use iced::advanced::text::highlighter::{Format, Highlighter};
use iced::font::{Family, Stretch, Style as FontStyle, Weight};
use iced::{Font, Theme};

use crate::palette::mix;

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
pub enum Kind {
    Text,
    Marker,
    Heading,
    Emphasis,
    Strong,
    StrongEmphasis,
    Code,
    CodeBlock,
    Link,
    Url,
    ListMarker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Highlight {
    pub kind: Kind,
    pub dim: bool,
    pub family: &'static str,
}

/// An open fenced code block: its delimiter character and length, so only a
/// matching fence closes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fence {
    ch: u8,
    len: usize,
}

pub struct MarkdownHighlighter {
    settings: Settings,
    current_line: usize,
    in_fence: Option<Fence>,
    /// The fence state at the start of each line seen so far, so highlighting can
    /// resume from any line with the right state.
    line_states: Vec<Option<Fence>>,
}

impl Highlighter for MarkdownHighlighter {
    type Settings = Settings;
    type Highlight = Highlight;
    type Iterator<'a> = std::vec::IntoIter<(Range<usize>, Highlight)>;

    fn new(settings: &Self::Settings) -> Self {
        Self { settings: settings.clone(), current_line: 0, in_fence: None, line_states: Vec::new() }
    }

    fn update(&mut self, new_settings: &Self::Settings) {
        self.settings = new_settings.clone();
        self.change_line(0);
    }

    fn change_line(&mut self, line: usize) {
        // Resume with the state that was in force at the start of `line`.
        self.current_line = line;
        self.line_states.truncate(line);
        self.in_fence = self.line_states.last().copied().flatten();
        if line == 0 {
            self.in_fence = None;
        }
    }

    fn highlight_line(&mut self, line: &str) -> Self::Iterator<'_> {
        let index = self.current_line;
        self.current_line += 1;

        // `line_states[n]` is the state before line n; the state after is pushed below.
        if self.line_states.len() < index {
            self.line_states.resize(index, None);
        }
        let mut spans = tokenize(line, &mut self.in_fence);
        self.line_states.truncate(index);
        self.line_states.push(self.in_fence);

        let dim_whole = match self.settings.focus {
            FocusMode::Off => false,
            FocusMode::Paragraph => !self.settings.paragraph.contains(&index),
            FocusMode::Sentence => index != self.settings.cursor_line,
        };

        if self.settings.focus == FocusMode::Sentence && index == self.settings.cursor_line {
            let s = self.settings.sentence.clone();
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
        self.current_line
    }
}

/// Map a highlight to colour and font. Passed to `TextEditor::highlight_with`.
pub fn to_format(h: &Highlight, theme: &Theme) -> Format<Font> {
    let p = theme.palette();
    let (bg, fg) = (p.background, p.text);

    let base = match h.kind {
        Kind::Marker | Kind::ListMarker => mix(fg, bg, 0.55),
        Kind::Url => mix(fg, bg, 0.45),
        Kind::Link => p.primary,
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

/// Split spans so that no span straddles any of `cuts`.
fn split_at(spans: Vec<(Range<usize>, Kind)>, cuts: &[usize]) -> Vec<(Range<usize>, Kind)> {
    let mut out = Vec::with_capacity(spans.len() + cuts.len());
    for (r, kind) in spans {
        let mut start = r.start;
        for &c in cuts {
            if c > start && c < r.end {
                out.push((start..c, kind));
                start = c;
            }
        }
        out.push((start..r.end, kind));
    }
    out
}

fn tokenize(line: &str, in_fence: &mut Option<Fence>) -> Vec<(Range<usize>, Kind)> {
    let len = line.len();
    let trimmed = line.trim_start();
    let indent = len - trimmed.len();

    // A fence is three or more backticks or tildes; a block closes only on the same
    // character, at least as long as the opener.
    let fence = trimmed.bytes().next().filter(|c| *c == b'`' || *c == b'~').map(|ch| Fence {
        ch,
        len: trimmed.bytes().take_while(|c| *c == ch).count(),
    });
    if let Some(fence) = fence.filter(|f| f.len >= 3) {
        match *in_fence {
            None => {
                *in_fence = Some(fence);
                return vec![(0..len, Kind::Marker)];
            }
            Some(open) if open.ch == fence.ch && fence.len >= open.len => {
                *in_fence = None;
                return vec![(0..len, Kind::Marker)];
            }
            Some(_) => {}
        }
    }
    if in_fence.is_some() {
        return vec![(0..len, Kind::CodeBlock)];
    }
    if len == 0 {
        return Vec::new();
    }

    // Headings: `# `, `## ` ... `###### `
    let hashes = trimmed.bytes().take_while(|b| *b == b'#').count();
    if (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ') {
        let marker_end = indent + hashes + 1;
        let mut spans = vec![(0..marker_end, Kind::Marker)];
        spans.push((marker_end..len, Kind::Heading));
        return spans;
    }

    // Horizontal rules
    if is_rule(trimmed) {
        return vec![(0..len, Kind::Marker)];
    }

    let mut spans = Vec::new();
    // `pos` is always a byte offset into the original line; `rest` is the text from `pos`.
    let mut pos = indent;
    if indent > 0 {
        spans.push((0..indent, Kind::Text));
    }

    // Block quote marker(s)
    let mut rest = trimmed;
    let mut offset = indent;
    while let Some(after) = rest.strip_prefix('>') {
        let after_ws = after.trim_start();
        let consumed = 1 + (after.len() - after_ws.len());
        spans.push((offset..offset + consumed, Kind::Marker));
        offset += consumed;
        rest = after_ws;
        pos = offset;
    }

    // List markers: `- `, `* `, `+ `, `1. `, `1) ` and task boxes
    if let Some(n) = list_marker_len(rest) {
        spans.push((pos..pos + n, Kind::ListMarker));
        pos += n;
        rest = &rest[n..];
        if let Some(n) = task_box_len(rest) {
            spans.push((pos..pos + n, Kind::Marker));
            pos += n;
        }
    }

    inline(line, pos, &mut spans);
    spans
}

fn is_rule(s: &str) -> bool {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    s.len() >= 3 && (s.bytes().all(|b| b == b'-') || s.bytes().all(|b| b == b'*') || s.bytes().all(|b| b == b'_'))
}

fn list_marker_len(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    if b.len() >= 2 && matches!(b[0], b'-' | b'*' | b'+') && b[1] == b' ' {
        return Some(2);
    }
    let digits = b.iter().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 && digits <= 9 && b.len() > digits + 1 && matches!(b[digits], b'.' | b')') && b[digits + 1] == b' ' {
        return Some(digits + 2);
    }
    None
}

fn task_box_len(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    if b.len() >= 4 && b[0] == b'[' && matches!(b[1], b' ' | b'x' | b'X') && b[2] == b']' && b[3] == b' ' {
        Some(4)
    } else {
        None
    }
}

/// Inline scan for code spans, emphasis, strong and links. All delimiters are
/// ASCII so byte indices never land inside a multi-byte character.
fn inline(line: &str, start: usize, spans: &mut Vec<(Range<usize>, Kind)>) {
    let b = line.as_bytes();
    let len = b.len();
    let mut i = start;
    let mut strong = false;
    let mut em = false;

    let text_kind = |strong: bool, em: bool| match (strong, em) {
        (true, true) => Kind::StrongEmphasis,
        (true, false) => Kind::Strong,
        (false, true) => Kind::Emphasis,
        (false, false) => Kind::Text,
    };

    // Once a closing delimiter is known to be absent after some position, later
    // searches for it are skipped: this keeps a line of repeated `[` or `` ` `` linear.
    let mut no_backtick_from = usize::MAX;
    let mut no_rbracket_from = usize::MAX;
    let mut no_rparen_from = usize::MAX;
    let find = |needle: u8, from: usize, none_from: &mut usize| -> Option<usize> {
        if from >= *none_from {
            return None;
        }
        let found = find_byte(b, needle, from);
        if found.is_none() {
            *none_from = from;
        }
        found
    };

    while i < len {
        let c = b[i];

        // Code span
        if c == b'`' {
            if let Some(j) = find(b'`', i + 1, &mut no_backtick_from) {
                spans.push((i..i + 1, Kind::Marker));
                spans.push((i + 1..j, Kind::Code));
                spans.push((j..j + 1, Kind::Marker));
                i = j + 1;
                continue;
            }
        }

        // Strong: ** or __
        if (c == b'*' || c == b'_') && i + 1 < len && b[i + 1] == c {
            spans.push((i..i + 2, Kind::Marker));
            strong = !strong;
            i += 2;
            continue;
        }

        // Emphasis: * or _ (underscore only at a word boundary)
        if c == b'*' || (c == b'_' && at_word_boundary(b, i)) {
            spans.push((i..i + 1, Kind::Marker));
            em = !em;
            i += 1;
            continue;
        }

        // Links and images: [text](url) / ![alt](url)
        let link_start = if c == b'!' && i + 1 < len && b[i + 1] == b'[' { Some(i + 1) } else if c == b'[' { Some(i) } else { None };
        if let Some(lb) = link_start {
            if let Some(rb) = find(b']', lb + 1, &mut no_rbracket_from) {
                if rb + 1 < len && b[rb + 1] == b'(' {
                    if let Some(rp) = find(b')', rb + 2, &mut no_rparen_from) {
                        spans.push((i..lb + 1, Kind::Marker));
                        spans.push((lb + 1..rb, Kind::Link));
                        spans.push((rb..rb + 2, Kind::Marker));
                        spans.push((rb + 2..rp, Kind::Url));
                        spans.push((rp..rp + 1, Kind::Marker));
                        i = rp + 1;
                        continue;
                    }
                }
            }
        }

        // Plain run until the next potentially special byte.
        let mut j = i + 1;
        while j < len && !matches!(b[j], b'`' | b'*' | b'_' | b'[' | b'!') {
            j += 1;
        }
        spans.push((i..j, text_kind(strong, em)));
        i = j;
    }
}

fn find_byte(b: &[u8], needle: u8, from: usize) -> Option<usize> {
    b[from.min(b.len())..].iter().position(|&x| x == needle).map(|p| p + from)
}

fn at_word_boundary(b: &[u8], i: usize) -> bool {
    let before = i == 0 || !b[i - 1].is_ascii_alphanumeric();
    let after = i + 1 >= b.len() || !b[i + 1].is_ascii_alphanumeric();
    before || after
}

/// Byte range of the sentence containing `col` in `line`.
pub fn sentence_at(line: &str, col: usize) -> Range<usize> {
    let col = col.min(line.len());
    let ends: Vec<usize> = line
        .char_indices()
        .filter(|(i, c)| {
            matches!(c, '.' | '!' | '?')
                && line[i + c.len_utf8()..].chars().next().is_none_or(|n| n.is_whitespace())
        })
        .map(|(i, c)| i + c.len_utf8())
        .collect();

    let start = ends.iter().rev().find(|&&e| e <= col).copied().unwrap_or(0);
    let end = ends.iter().find(|&&e| e > col).copied().unwrap_or(line.len());

    // Skip the whitespace that follows the previous terminator.
    let start = start + line[start..].len() - line[start..].trim_start().len();
    start..end
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(line: &str) -> Vec<(Range<usize>, Kind)> {
        let mut fence = None;
        tokenize(line, &mut fence)
    }

    /// Every span must be in order, inside the line and on UTF-8 boundaries.
    fn assert_spans_valid(line: &str, spans: &[(Range<usize>, Kind)]) {
        let mut last = 0;
        for (r, _) in spans {
            assert!(r.start >= last && r.end >= r.start && r.end <= line.len(), "bad span {r:?} in {line:?}");
            assert!(line.is_char_boundary(r.start) && line.is_char_boundary(r.end), "span {r:?} splits a char in {line:?}");
            last = r.end;
        }
    }

    #[test]
    fn indented_list_offsets_are_line_relative() {
        let line = "  - [x] done";
        let spans = kinds(line);
        assert_spans_valid(line, &spans);
        assert_eq!(spans[0], (0..2, Kind::Text));
        assert_eq!(spans[1], (2..4, Kind::ListMarker));
        assert_eq!(spans[2], (4..8, Kind::Marker));
        // Unicode indentation (em space, 3 bytes) must not be split.
        let line = "\u{2003}- item";
        let spans = kinds(line);
        assert_spans_valid(line, &spans);
        assert_eq!(spans[0], (0..3, Kind::Text));
    }

    #[test]
    fn fence_state_resumes_per_line() {
        let settings = Settings::default();
        let mut h = MarkdownHighlighter::new(&settings);
        let lines = ["```rust", "let x = 1;", "```", "after", "~~~", "inside tilde", "```", "still inside"];
        let kinds_of = |h: &mut MarkdownHighlighter, line: &str| h.highlight_line(line).map(|(_, hl)| hl.kind).next();
        let first: Vec<_> = lines.iter().map(|l| kinds_of(&mut h, l)).collect();
        assert_eq!(first[1], Some(Kind::CodeBlock));
        assert_eq!(first[3], Some(Kind::Text));
        // A backtick fence does not close a tilde block.
        assert_eq!(first[7], Some(Kind::CodeBlock));
        // Resume after the block closed: plain text.
        h.change_line(3);
        assert_eq!(kinds_of(&mut h, lines[3]), Some(Kind::Text));
        // Resume from the interior line of the first block: still code, and the lines
        // that follow (re-highlighted in order, as iced does) come out right too.
        h.change_line(1);
        assert_eq!(kinds_of(&mut h, lines[1]), Some(Kind::CodeBlock));
        assert_eq!(kinds_of(&mut h, lines[2]), Some(Kind::Marker));
        assert_eq!(kinds_of(&mut h, lines[3]), Some(Kind::Text));
    }

    #[test]
    fn repeated_brackets_stay_linear() {
        let line = "[".repeat(20_000);
        let start = std::time::Instant::now();
        let spans = kinds(&line);
        assert_spans_valid(&line, &spans);
        assert!(start.elapsed().as_millis() < 200, "took {:?}", start.elapsed());
    }

    #[test]
    fn heading() {
        assert_eq!(kinds("## Title"), vec![(0..3, Kind::Marker), (3..8, Kind::Heading)]);
    }

    #[test]
    fn strong_and_em() {
        assert_eq!(
            kinds("a **b** _c_"),
            vec![
                (0..2, Kind::Text),
                (2..4, Kind::Marker),
                (4..5, Kind::Strong),
                (5..7, Kind::Marker),
                (7..8, Kind::Text),
                (8..9, Kind::Marker),
                (9..10, Kind::Emphasis),
                (10..11, Kind::Marker),
            ]
        );
    }

    #[test]
    fn link() {
        assert_eq!(
            kinds("[iA](https://ia.net)"),
            vec![
                (0..1, Kind::Marker),
                (1..3, Kind::Link),
                (3..5, Kind::Marker),
                (5..19, Kind::Url),
                (19..20, Kind::Marker),
            ]
        );
    }

    #[test]
    fn list_with_task() {
        assert_eq!(kinds("- [x] done")[..2], [(0..2, Kind::ListMarker), (2..6, Kind::Marker)]);
    }

    #[test]
    fn unicode_is_safe() {
        let spans = kinds("Città *è* bella — ok");
        assert!(spans.iter().all(|(r, _)| r.end <= "Città *è* bella — ok".len()));
    }

    #[test]
    fn sentences() {
        let l = "First one. Second here! Third?";
        assert_eq!(sentence_at(l, 0), 0..10);
        assert_eq!(sentence_at(l, 12), 11..23);
        assert_eq!(sentence_at(l, 28), 24..30);
    }
}
