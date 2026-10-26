//! A title for a new document, from its first line (Francesco, 2026-10-23: "If HH creates a
//! new file and the user closes it, the title should be the first 5 words from the first
//! line"). The same rules as the iPad's `AutoTitle.title(for:)`: the first five words of the
//! first line that has a letter, a digit or an emoji once Markdown is taken off, without
//! characters awkward in a file name, at most 60 characters and 200 bytes.
//!
//! The iPad renames a new document's file when it closes; the Linux app has no file before
//! the first save, so it offers the title as the file name in the Save dialog instead.
//! Lengths here count Unicode scalar values where the iPad counts grapheme clusters, so a
//! title of long emoji sequences may be cut a little earlier.

/// Words taken from the line.
pub const WORD_LIMIT: usize = 5;
/// Characters in a title.
pub const LENGTH_LIMIT: usize = 60;
/// Bytes in a title, so " 99999.md" still fits a file system's 255.
pub const BYTE_LIMIT: usize = 200;
/// Lines with text looked at; a document whose first lines are all Markdown punctuation
/// gets no title.
pub const LINE_LIMIT: usize = 64;
/// Characters of a line looked at.
pub const LINE_LENGTH: usize = 4096;

/// The title for `text`, or `None` when none of its first lines has one.
pub fn suggest(text: &str) -> Option<String> {
    let mut rest = text;
    for _ in 0..LINE_LIMIT {
        rest = rest.trim_start(); // newlines are whitespace too
        if rest.is_empty() {
            return None;
        }
        let end = rest.find(is_newline).unwrap_or(rest.len());
        let line: String = rest[..end].chars().take(LINE_LENGTH).collect();
        if let Some(title) = title_of_line(&line) {
            return Some(title);
        }
        rest = &rest[end..];
    }
    None
}

/// Whether `name` is the name New would give: "Untitled.md" or "Untitled N.md".
pub fn is_untitled(name: &str) -> bool {
    let Some(base) = name.strip_suffix(".md") else { return false };
    match base.strip_prefix("Untitled") {
        Some("") => true,
        Some(n) => n
            .strip_prefix(' ')
            .is_some_and(|d| !d.is_empty() && !d.starts_with('0') && d.bytes().all(|b| b.is_ascii_digit())),
        None => false,
    }
}

fn is_newline(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

fn title_of_line(line: &str) -> Option<String> {
    let mut text = line.trim();
    // Block markers at the start, nested ones too ("> - [ ] item").
    while let Some(rest) = strip_marker(text) {
        text = rest;
    }
    // Images and links keep their text; emphasis and code marks go.
    let text: String = keep_link_text(text)
        .chars()
        .map(|c| match c {
            '*' | '_' | '~' | '`' => ' ',
            '/' | ':' | '\\' | '?' | '"' | '<' | '>' | '|' => ' ',
            // Control characters, and the bidi overrides that can make a name read as
            // another; joiners stay (they hold emoji sequences together).
            c if c.is_control() || ('\u{202A}'..='\u{202E}').contains(&c) || ('\u{2066}'..='\u{2069}').contains(&c) => ' ',
            c => c,
        })
        .collect();
    let mut title = text.split_whitespace().take(WORD_LIMIT).collect::<Vec<_>>().join(" ");
    while let Some(rest) = title.strip_prefix('.') {
        title = rest.trim().to_owned();
    }
    if title.chars().count() > LENGTH_LIMIT {
        // Cut at a word if there is one in the second half, else at the limit.
        let cut: String = title.chars().take(LENGTH_LIMIT).collect();
        title = match cut.rfind(' ') {
            Some(space) if cut[..space].chars().count() > LENGTH_LIMIT / 2 => cut[..space].to_owned(),
            _ => cut,
        };
    }
    // Few characters can still be many bytes (long emoji sequences, stacked marks).
    while title.len() > BYTE_LIMIT {
        title.pop();
    }
    let title = title.trim().to_owned();
    title.chars().any(|c| c.is_alphabetic() || c.is_numeric() || is_emoji(c)).then_some(title)
}

/// One block marker at the start of `text`, if any: `#`–`######` followed by a space or the
/// end, `>`, `-`/`*`/`+` followed by a space, `1.` or `1)` followed by a space, `[ ]`/`[x]`.
fn strip_marker(text: &str) -> Option<&str> {
    let b = text.as_bytes();
    let spaces = |from: usize| text[from..].len() - text[from..].trim_start().len();
    let hashes = b.iter().take_while(|&&c| c == b'#').count();
    if (1..=6).contains(&hashes) {
        let n = spaces(hashes);
        if n > 0 || hashes == text.len() {
            return Some(&text[hashes + n..]);
        }
    }
    if b.first() == Some(&b'>') {
        return Some(&text[1 + spaces(1)..]);
    }
    if matches!(b.first(), Some(b'-' | b'*' | b'+')) && spaces(1) > 0 {
        return Some(&text[1 + spaces(1)..]);
    }
    let digits = b.iter().take_while(|c| c.is_ascii_digit()).count();
    if (1..=9).contains(&digits) && matches!(b.get(digits), Some(b'.' | b')')) && spaces(digits + 1) > 0 {
        return Some(&text[digits + 1 + spaces(digits + 1)..]);
    }
    if b.len() >= 3 && b[0] == b'[' && matches!(b[1], b' ' | b'x' | b'X') && b[2] == b']' {
        return Some(&text[3 + spaces(3)..]);
    }
    None
}

/// `[text](url)` and `![text](url)` become `text`.
fn keep_link_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let link = after.find(']').and_then(|close| {
            let label = &after[..close];
            let tail = after[close + 1..].strip_prefix('(')?;
            let end = tail.find(')')?;
            Some((label, &tail[end + 1..]))
        });
        match link {
            Some((label, next)) => {
                let before = &rest[..open];
                out.push_str(before.strip_suffix('!').unwrap_or(before));
                out.push_str(label);
                rest = next;
            }
            None => {
                out.push_str(&rest[..=open]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Emoji outside Latin-1 (the iPad's test: the Emoji property above U+00FF), by block.
fn is_emoji(c: char) -> bool {
    matches!(c as u32, 0x1F000..=0x1FAFF | 0x2300..=0x23FF | 0x2600..=0x27BF | 0x2B00..=0x2BFF | 0x3030 | 0x303D | 0x3297 | 0x3299)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(text: &str) -> Option<String> {
        suggest(text)
    }

    // The iPad's AutoTitleTests, case for case.
    #[test]
    fn first_five_words_of_the_first_line() {
        assert_eq!(t("The quick brown fox jumps over the dog\nsecond line").as_deref(), Some("The quick brown fox jumps"));
        assert_eq!(t("Two words").as_deref(), Some("Two words"));
        assert_eq!(t("One").as_deref(), Some("One"));
        assert_eq!(t("  spaced    out\twords  ").as_deref(), Some("spaced out words"));
    }

    #[test]
    fn markdown_is_taken_off() {
        for (text, title) in [
            ("# Chapter one", "Chapter one"),
            ("### Deep heading here", "Deep heading here"),
            ("- a list item", "a list item"),
            ("* starred item", "starred item"),
            ("+ plus item", "plus item"),
            ("> quoted words", "quoted words"),
            ("1. numbered item", "numbered item"),
            ("12) other numbering", "other numbering"),
            ("- [ ] buy milk", "buy milk"),
            ("- [x] done thing", "done thing"),
            ("> - nested quote list", "nested quote list"),
            ("**Bold** and _italic_ and ~~gone~~ `code`", "Bold and italic and gone"),
            ("See [the docs](https://example.com) now", "See the docs now"),
            ("![A picture](p.png) caption", "A picture caption"),
            ("#hashtag stays", "#hashtag stays"),
        ] {
            assert_eq!(t(text).as_deref(), Some(title), "{text}");
        }
    }

    #[test]
    fn awkward_characters_become_spaces() {
        assert_eq!(t("and/or: a \"quote\" <tag> a|b").as_deref(), Some("and or a quote tag"));
        assert_eq!(t("why? back\\slash star*").as_deref(), Some("why back slash star"));
        assert_eq!(t("tab\u{0007}bell").as_deref(), Some("tab bell"));
        assert_eq!(t("...hidden file").as_deref(), Some("hidden file"));
        assert_eq!(t(". . dots").as_deref(), Some("dots"));
        assert_eq!(t("a\u{202E}gnp.exe").as_deref(), Some("a gnp.exe"), "bidi override");
    }

    #[test]
    fn capped_at_sixty_characters_at_a_word() {
        let long = "Supercalifragilistic ".repeat(5);
        assert_eq!(t(&long).as_deref(), Some("Supercalifragilistic Supercalifragilistic"));
        assert_eq!(t(&"a".repeat(200)), Some("a".repeat(60)));
        assert_eq!(t("✍️ 🎉").as_deref(), Some("✍️ 🎉"));
        assert_eq!(t("--- === !!! ()"), None);
        let emoji = "👩‍👩‍👧‍👦".repeat(20);
        assert!(t(&emoji).is_some_and(|s| s.len() <= BYTE_LIMIT));
    }

    #[test]
    fn blank_and_marker_only_lines_are_skipped() {
        assert_eq!(t("\n\n   \n#\n---\n***\n> \nReal start here").as_deref(), Some("Real start here"));
        assert_eq!(t(""), None);
        assert_eq!(t("   \n\n\t\n"), None);
        assert_eq!(t("# \n- \n***"), None);
        let many = "---\n".repeat(LINE_LIMIT) + "Too late";
        assert_eq!(t(&many), None, "only the first lines are looked at");
    }

    #[test]
    fn untitled_names() {
        for name in ["Untitled.md", "Untitled 2.md", "Untitled 21.md"] {
            assert!(is_untitled(name), "{name}");
        }
        for name in ["Untitled 0.md", "Untitled2.md", "untitled.md", "Untitled draft.md", "Untitled.txt", "Chapter.md"] {
            assert!(!is_untitled(name), "{name}");
        }
    }
}
