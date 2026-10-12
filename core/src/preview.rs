//! The model a Markdown preview draws from: the document as a tree of blocks with
//! styled inline runs, built with a CommonMark parser (plus task lists and
//! strikethrough). Pure: a platform renders the tree with its own widgets and maps
//! each block's source line back to the editor for scrolling in step.

use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// A parsed document: the top-level blocks in order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Document {
    pub blocks: Vec<Block>,
}

/// One block. `line` is the zero-based source line where it starts.
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Heading { level: u8, inlines: Vec<Inline>, line: usize },
    Paragraph { inlines: Vec<Inline>, line: usize },
    /// Fenced or indented code. `text` keeps its line breaks, without the final one.
    Code { language: Option<String>, text: String, line: usize },
    Quote { blocks: Vec<Block>, line: usize },
    /// `start` is the first number of an ordered list, `None` for a bulleted one.
    List { start: Option<u64>, items: Vec<Item>, line: usize },
    Rule { line: usize },
}

impl Block {
    pub fn line(&self) -> usize {
        match self {
            Self::Heading { line, .. }
            | Self::Paragraph { line, .. }
            | Self::Code { line, .. }
            | Self::Quote { line, .. }
            | Self::List { line, .. }
            | Self::Rule { line } => *line,
        }
    }
}

/// A list item: its own blocks, and its checkbox state for a task list.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Item {
    pub checked: Option<bool>,
    pub blocks: Vec<Block>,
}

/// Inline content: runs of uniformly styled text and explicit line breaks.
#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    Text { text: String, style: Style, link: Option<String> },
    /// A hard break (two trailing spaces or a backslash). Soft breaks become spaces.
    Break,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Style {
    pub strong: bool,
    pub emphasis: bool,
    pub strike: bool,
    pub code: bool,
    /// The text is an image's alternative text; `link` holds the image address.
    pub image: bool,
}

/// Parse `text` into a [`Document`]. Linear in the input; safe on any text.
pub fn parse(text: &str) -> Document {
    let options = Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH;
    let mut b = Builder { starts: line_starts(text), ..Builder::default() };
    for (event, range) in Parser::new_ext(text, options).into_offset_iter() {
        b.event(event, range);
    }
    b.flush();
    while !b.frames.is_empty() {
        b.close_frame();
    }
    Document { blocks: b.blocks }
}

fn line_starts(text: &str) -> Vec<usize> {
    std::iter::once(0).chain(text.match_indices('\n').map(|(i, _)| i + 1)).collect()
}

/// A container being filled: blocks go into the innermost one.
#[derive(Debug)]
enum Frame {
    Quote { blocks: Vec<Block>, line: usize },
    List { start: Option<u64>, items: Vec<Item>, line: usize },
    Item(Item),
}

/// The inline block being filled, if any.
#[derive(Debug, Clone, Copy)]
enum Target {
    Heading(u8),
    Paragraph,
}

#[derive(Debug, Default)]
struct Builder {
    starts: Vec<usize>,
    blocks: Vec<Block>,
    frames: Vec<Frame>,
    target: Option<(Target, usize)>,
    inlines: Vec<Inline>,
    style: Style,
    /// Nesting depths, so `**a *b* c**` unwinds correctly.
    strong: u32,
    emphasis: u32,
    strike: u32,
    links: Vec<String>,
    image: u32,
    code: Option<(Option<String>, String, usize)>,
}

impl Builder {
    fn line(&self, offset: usize) -> usize {
        self.starts.partition_point(|&s| s <= offset).saturating_sub(1)
    }

    fn event(&mut self, event: Event<'_>, range: Range<usize>) {
        let line = self.line(range.start);
        if let Some((_, code, _)) = &mut self.code {
            match event {
                Event::Text(t) | Event::Html(t) => code.push_str(&t),
                Event::End(TagEnd::CodeBlock) => {
                    let (language, mut text, line) = self.code.take().unwrap_or_default();
                    if text.ends_with('\n') {
                        text.pop();
                    }
                    self.push_block(Block::Code { language, text, line });
                }
                _ => {}
            }
            return;
        }
        match event {
            Event::Start(tag) => self.start(tag, line),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => self.text(&t, line),
            Event::Code(t) => {
                let saved = self.style;
                self.style.code = true;
                self.text(&t, line);
                self.style = saved;
            }
            Event::InlineMath(t) | Event::DisplayMath(t) => self.text(&t, line),
            // Raw HTML is shown as written: the preview never interprets markup.
            Event::Html(t) | Event::InlineHtml(t) => self.text(t.trim_end_matches('\n'), line),
            Event::FootnoteReference(t) => self.text(&format!("[^{t}]"), line),
            Event::SoftBreak => self.text(" ", line),
            Event::HardBreak => {
                self.open(line);
                self.inlines.push(Inline::Break);
            }
            Event::Rule => {
                self.flush();
                self.push_block(Block::Rule { line });
            }
            Event::TaskListMarker(checked) => {
                if let Some(Frame::Item(item)) = self.frames.last_mut() {
                    item.checked = Some(checked);
                }
            }
        }
    }

    fn start(&mut self, tag: Tag<'_>, line: usize) {
        match tag {
            Tag::Paragraph => {
                self.flush();
                self.target = Some((Target::Paragraph, line));
            }
            Tag::Heading { level, .. } => {
                self.flush();
                self.target = Some((Target::Heading(heading_level(level)), line));
            }
            Tag::BlockQuote(_) => {
                self.flush();
                self.frames.push(Frame::Quote { blocks: Vec::new(), line });
            }
            Tag::CodeBlock(kind) => {
                self.flush();
                let language = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().filter(|l| !l.is_empty()).map(str::to_owned)
                    }
                    CodeBlockKind::Indented => None,
                };
                self.code = Some((language, String::new(), line));
            }
            Tag::List(start) => {
                self.flush();
                self.frames.push(Frame::List { start, items: Vec::new(), line });
            }
            Tag::Item => {
                self.flush();
                self.frames.push(Frame::Item(Item::default()));
            }
            Tag::Emphasis => {
                self.emphasis += 1;
                self.style.emphasis = true;
            }
            Tag::Strong => {
                self.strong += 1;
                self.style.strong = true;
            }
            Tag::Strikethrough => {
                self.strike += 1;
                self.style.strike = true;
            }
            Tag::Link { dest_url, .. } => self.links.push(dest_url.into_string()),
            Tag::Image { dest_url, .. } => {
                self.image += 1;
                self.style.image = true;
                self.links.push(dest_url.into_string());
            }
            // Anything else (HTML blocks, tables, footnote definitions) contributes its
            // text as ordinary paragraphs.
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::HtmlBlock => self.flush(),
            TagEnd::BlockQuote(_) | TagEnd::List(_) | TagEnd::Item => {
                self.flush();
                self.close_frame();
            }
            TagEnd::Emphasis => {
                self.emphasis = self.emphasis.saturating_sub(1);
                self.style.emphasis = self.emphasis > 0;
            }
            TagEnd::Strong => {
                self.strong = self.strong.saturating_sub(1);
                self.style.strong = self.strong > 0;
            }
            TagEnd::Strikethrough => {
                self.strike = self.strike.saturating_sub(1);
                self.style.strike = self.strike > 0;
            }
            TagEnd::Link => {
                self.links.pop();
            }
            TagEnd::Image => {
                self.links.pop();
                self.image = self.image.saturating_sub(1);
                self.style.image = self.image > 0;
            }
            _ => {}
        }
    }

    /// Start an implicit paragraph if text arrives outside one (tight list items).
    fn open(&mut self, line: usize) {
        if self.target.is_none() {
            self.target = Some((Target::Paragraph, line));
        }
    }

    fn text(&mut self, text: &str, line: usize) {
        if text.is_empty() {
            return;
        }
        self.open(line);
        let link = self.links.last().cloned();
        if let Some(Inline::Text { text: last, style, link: last_link }) = self.inlines.last_mut()
            && *style == self.style
            && *last_link == link
        {
            last.push_str(text);
            return;
        }
        self.inlines.push(Inline::Text { text: text.to_owned(), style: self.style, link });
    }

    /// Finish the inline block in progress, if any.
    fn flush(&mut self) {
        let Some((target, line)) = self.target.take() else { return };
        let inlines = std::mem::take(&mut self.inlines);
        self.push_block(match target {
            Target::Heading(level) => Block::Heading { level, inlines, line },
            Target::Paragraph => Block::Paragraph { inlines, line },
        });
    }

    fn push_block(&mut self, block: Block) {
        match self.frames.last_mut() {
            Some(Frame::Quote { blocks, .. }) | Some(Frame::Item(Item { blocks, .. })) => blocks.push(block),
            // Not produced by the parser (items always wrap list content); kept safe anyway.
            Some(Frame::List { items, .. }) => items.push(Item { checked: None, blocks: vec![block] }),
            None => self.blocks.push(block),
        }
    }

    fn close_frame(&mut self) {
        match self.frames.pop() {
            Some(Frame::Quote { blocks, line }) => self.push_block(Block::Quote { blocks, line }),
            Some(Frame::List { start, items, line }) => self.push_block(Block::List { start, items, line }),
            Some(Frame::Item(item)) => match self.frames.last_mut() {
                Some(Frame::List { items, .. }) => items.push(item),
                _ => {
                    for block in item.blocks {
                        self.push_block(block);
                    }
                }
            },
            None => {}
        }
    }
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// Plain text of some inlines, for tests and accessibility.
pub fn plain(inlines: &[Inline]) -> String {
    inlines
        .iter()
        .map(|i| match i {
            Inline::Text { text, .. } => text.as_str(),
            Inline::Break => "\n",
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, style: Style, link: Option<&str>) -> Inline {
        Inline::Text { text: text.into(), style, link: link.map(str::to_owned) }
    }

    #[test]
    fn empty_and_blank_documents() {
        assert_eq!(parse(""), Document::default());
        assert_eq!(parse("\n\n   \n"), Document::default());
    }

    #[test]
    fn headings_paragraphs_and_lines() {
        let doc = parse("# Title\n\nFirst line\nsecond line.\n\n### Small\n");
        assert_eq!(doc.blocks.len(), 3);
        assert!(matches!(&doc.blocks[0], Block::Heading { level: 1, line: 0, inlines } if plain(inlines) == "Title"));
        // A soft break becomes a space; the paragraph starts on line 2.
        assert!(
            matches!(&doc.blocks[1], Block::Paragraph { line: 2, inlines } if plain(inlines) == "First line second line.")
        );
        assert!(matches!(&doc.blocks[2], Block::Heading { level: 3, line: 5, .. }));
        assert_eq!(doc.blocks.iter().map(Block::line).collect::<Vec<_>>(), vec![0, 2, 5]);
    }

    #[test]
    fn inline_styles_merge_and_nest() {
        let doc = parse("a **b *c* d** `e` ~~f~~ [g](https://x.y) ![h](i.png)\\\nj");
        let Block::Paragraph { inlines, .. } = &doc.blocks[0] else { panic!() };
        let strong = Style { strong: true, ..Style::default() };
        let both = Style { strong: true, emphasis: true, ..Style::default() };
        let code = Style { code: true, ..Style::default() };
        let strike = Style { strike: true, ..Style::default() };
        let image = Style { image: true, ..Style::default() };
        let plain_style = Style::default();
        assert_eq!(
            inlines,
            &vec![
                run("a ", plain_style, None),
                run("b ", strong, None),
                run("c", both, None),
                run(" d", strong, None),
                run(" ", plain_style, None),
                run("e", code, None),
                run(" ", plain_style, None),
                run("f", strike, None),
                run(" ", plain_style, None),
                run("g", plain_style, Some("https://x.y")),
                run(" ", plain_style, None),
                run("h", image, Some("i.png")),
                Inline::Break,
                run("j", plain_style, None),
            ]
        );
    }

    #[test]
    fn lists_tight_loose_nested_ordered_and_tasks() {
        let doc = parse("- one\n- two\n  - inner\n\n3. three\n4. four\n\n- [x] done\n- [ ] open\n");
        assert_eq!(doc.blocks.len(), 3);
        let Block::List { start: None, items, line: 0 } = &doc.blocks[0] else { panic!("{:?}", doc.blocks[0]) };
        assert_eq!(items.len(), 2);
        assert!(matches!(&items[0].blocks[..], [Block::Paragraph { inlines, .. }] if plain(inlines) == "one"));
        // The nested list belongs to the second item, after its own text.
        assert!(matches!(&items[1].blocks[1], Block::List { start: None, items, line: 2 } if items.len() == 1));
        assert!(matches!(&doc.blocks[1], Block::List { start: Some(3), items, line: 4 } if items.len() == 2));
        let Block::List { items, .. } = &doc.blocks[2] else { panic!() };
        assert_eq!(items.iter().map(|i| i.checked).collect::<Vec<_>>(), vec![Some(true), Some(false)]);
        assert!(matches!(&items[0].blocks[..], [Block::Paragraph { inlines, .. }] if plain(inlines) == "done"));
    }

    #[test]
    fn quotes_code_rules_and_html() {
        let doc = parse("> quoted\n> - item\n\n```rust title\nfn a() {}\n\nb\n```\n\n---\n\n    indented\n\n<b>raw</b>\n");
        let Block::Quote { blocks, line: 0 } = &doc.blocks[0] else { panic!("{:?}", doc.blocks[0]) };
        assert!(matches!(&blocks[0], Block::Paragraph { inlines, .. } if plain(inlines) == "quoted"));
        assert!(matches!(&blocks[1], Block::List { .. }));
        assert_eq!(
            doc.blocks[1],
            Block::Code { language: Some("rust".into()), text: "fn a() {}\n\nb".into(), line: 3 }
        );
        assert_eq!(doc.blocks[2], Block::Rule { line: 9 });
        assert_eq!(doc.blocks[3], Block::Code { language: None, text: "indented".into(), line: 11 });
        assert!(matches!(&doc.blocks[4], Block::Paragraph { inlines, line: 13 } if plain(inlines) == "<b>raw</b>"));
    }

    #[test]
    fn unclosed_constructs_and_odd_input_are_safe() {
        for text in ["```\nnever closed", "> > > deep", "- a\n  - b\n    - c\n      - d", "**open", "[x](", "\u{0}\r\n\r\n#", "é ü ✓"]
        {
            let doc = parse(text);
            assert!(!doc.blocks.is_empty(), "{text:?}");
        }
        assert!(matches!(&parse("```\nnever closed").blocks[0], Block::Code { text, .. } if text == "never closed"));
    }

    /// Live preview re-parses on every keystroke, so a large document must stay quick.
    #[test]
    fn large_documents_parse_quickly() {
        let unit = "## Heading\n\nSome *text* with `code` and a [link](https://example.com).\n\n- item\n- [x] task\n\n> quote\n\n";
        let text = unit.repeat(1_000_000 / unit.len());
        let start = std::time::Instant::now();
        let doc = parse(&text);
        assert!(doc.blocks.len() > 10_000);
        assert!(start.elapsed().as_millis() < 2_000, "1 MB took {:?}", start.elapsed());
    }
}
