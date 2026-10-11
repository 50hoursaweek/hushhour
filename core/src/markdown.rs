//! Line-based Markdown scanner: one logical line at a time, in order, with just enough
//! state (fenced code blocks) to style multi-line constructs, and the sentence finder
//! behind the spotlight. Pure: no platform or UI types.

use std::ops::Range;

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

/// An open fenced code block: its delimiter character and length, so only a
/// matching fence closes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fence {
    ch: u8,
    len: usize,
}

/// Scans lines in order and remembers the fence state after each one, so scanning can
/// resume at any line with the state its predecessor left (editors re-highlight from the
/// first changed line).
#[derive(Debug, Default)]
pub struct Scanner {
    current_line: usize,
    in_fence: Option<Fence>,
    /// `line_states[n]` is the fence state after line n.
    line_states: Vec<Option<Fence>>,
}

impl Scanner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Resume with the state that was in force at the start of `line`.
    pub fn change_line(&mut self, line: usize) {
        self.current_line = line;
        self.line_states.truncate(line);
        self.in_fence = self.line_states.last().copied().flatten();
        if line == 0 {
            self.in_fence = None;
        }
    }

    /// Index of the line the next `scan` will be given.
    pub fn current_line(&self) -> usize {
        self.current_line
    }

    /// Spans of the next line, recording the state it leaves behind.
    pub fn scan(&mut self, line: &str) -> Vec<(Range<usize>, Kind)> {
        let index = self.current_line;
        self.current_line += 1;
        if self.line_states.len() < index {
            self.line_states.resize(index, None);
        }
        let spans = tokenize(line, &mut self.in_fence);
        self.line_states.truncate(index);
        self.line_states.push(self.in_fence);
        spans
    }
}

/// Split spans so that no span straddles any of `cuts`.
pub fn split_at(spans: Vec<(Range<usize>, Kind)>, cuts: &[usize]) -> Vec<(Range<usize>, Kind)> {
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

/// Spans of one line, given (and updating) the fence state its predecessor left.
pub fn tokenize(line: &str, in_fence: &mut Option<Fence>) -> Vec<(Range<usize>, Kind)> {
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

    // Positions of the next closing delimiter from every index, computed once per
    // line by a backward pass, so every lookup is O(1) and the scan is linear whether
    // or not closers exist.
    let next_backtick = next_positions(b, b'`');
    let next_rbracket = next_positions(b, b']');
    let next_rparen = next_positions(b, b')');
    let at = |table: &[u32], from: usize| -> Option<usize> {
        let i = *table.get(from)?;
        (i != u32::MAX).then_some(i as usize)
    };

    while i < len {
        let c = b[i];

        // Code span
        if c == b'`' {
            if let Some(j) = at(&next_backtick, i + 1) {
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
            if let Some(rb) = at(&next_rbracket, lb + 1) {
                if rb + 1 < len && b[rb + 1] == b'(' {
                    if let Some(rp) = at(&next_rparen, rb + 2) {
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

/// `table[i]` is the index of the first `needle` at or after `i`, or `u32::MAX`.
fn next_positions(b: &[u8], needle: u8) -> Vec<u32> {
    let mut table = vec![u32::MAX; b.len() + 1];
    let mut next = u32::MAX;
    for i in (0..b.len()).rev() {
        if b[i] == needle {
            next = i as u32;
        }
        table[i] = next;
    }
    table
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
        let mut s = Scanner::new();
        let lines = ["```rust", "let x = 1;", "```", "after", "~~~", "inside tilde", "```", "still inside"];
        let kinds_of = |s: &mut Scanner, line: &str| s.scan(line).first().map(|(_, k)| *k);
        let first: Vec<_> = lines.iter().map(|l| kinds_of(&mut s, l)).collect();
        assert_eq!(first[1], Some(Kind::CodeBlock));
        assert_eq!(first[3], Some(Kind::Text));
        // A backtick fence does not close a tilde block.
        assert_eq!(first[7], Some(Kind::CodeBlock));
        // Resume after the block closed: plain text.
        s.change_line(3);
        assert_eq!(kinds_of(&mut s, lines[3]), Some(Kind::Text));
        // Resume from the interior line of the first block: still code, and the lines
        // that follow (re-scanned in order, as an editor does) come out right too.
        s.change_line(1);
        assert_eq!(kinds_of(&mut s, lines[1]), Some(Kind::CodeBlock));
        assert_eq!(kinds_of(&mut s, lines[2]), Some(Kind::Marker));
        assert_eq!(kinds_of(&mut s, lines[3]), Some(Kind::Text));
    }

    /// Adversarial lines must scan in linear time: no closer, a present but unusable
    /// closer, nested openers, and `](` with no final `)`.
    #[test]
    fn adversarial_lines_stay_linear() {
        for line in [
            "[".repeat(20_000),
            format!("{}]", "[".repeat(20_000)),
            format!("{}](", "[".repeat(20_000)),
            "`".repeat(20_001),
            format!("{}x", "![".repeat(10_000)),
        ] {
            let start = std::time::Instant::now();
            let spans = kinds(&line);
            assert_spans_valid(&line, &spans);
            // Doubling the input must not quadruple the time; a generous absolute bound
            // catches the quadratic case (which took seconds) without being flaky.
            assert!(start.elapsed().as_millis() < 300, "{:?} took {:?}", &line[..8], start.elapsed());
        }
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
