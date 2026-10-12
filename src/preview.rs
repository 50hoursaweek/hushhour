//! Draws the preview model from `hushhour_core::preview` with iced widgets, in the
//! current theme and font: headings scale up, code uses the mono face on a tinted
//! ground, quotes get a rule at their left, links are underlined and open in the browser.

use iced::font::{Family, Style as FontStyle, Weight};
use iced::widget::text::Span;
use iced::widget::{column, container, rich_text, row, rule, scrollable, span, text};
use iced::{Background, Border, Color, Element, Font, Length, Padding, Theme};

use hushhour_core::preview::{Block, Document, Inline, Item, Style};

use crate::Message;
use crate::highlight::MONO_FAMILY;
use crate::palette::mix;

/// The preview's scrollable, so it can be kept in step with the editor's caret.
pub const SCROLL_ID: &str = "preview";

/// How the preview is set: the editor's font and size, and the page colours.
#[derive(Clone, Copy)]
pub struct Look {
    pub family: &'static str,
    pub size: f32,
    pub line_height: f32,
    pub text: Color,
    pub background: Color,
    pub accent: Color,
}

impl Look {
    pub fn new(theme: &Theme, family: &'static str, size: f32, line_height: f32) -> Self {
        let p = theme.palette();
        Self { family, size, line_height, text: p.text, background: p.background, accent: p.primary }
    }

    fn muted(&self, t: f32) -> Color {
        mix(self.text, self.background, t)
    }

    fn font(&self, style: Style) -> Font {
        let family = if style.code { MONO_FAMILY } else { self.family };
        Font {
            family: Family::Name(family),
            weight: if style.strong { Weight::Bold } else { Weight::Normal },
            style: if style.emphasis || style.image { FontStyle::Italic } else { FontStyle::Normal },
            ..Font::DEFAULT
        }
    }
}

/// Only web and mail links are opened; anything else (`file:`, custom schemes) is not.
pub fn openable(url: &str) -> bool {
    let lower = url.trim_start().to_ascii_lowercase();
    ["https://", "http://", "mailto:"].iter().any(|scheme| lower.starts_with(scheme))
}

/// The whole preview: a centred column of the document's blocks in a scrollable.
pub fn view<'a>(doc: &'a Document, look: Look, padding: Padding, max_width: f32) -> Element<'a, Message> {
    let body: Element<'a, Message> = if doc.blocks.is_empty() {
        text("Nothing to preview yet.").size(look.size).color(look.muted(0.55)).font(look.font(Style::default())).into()
    } else {
        blocks(&doc.blocks, look)
    };
    let page = container(body).padding(padding).width(Length::Fill).max_width(max_width);
    scrollable(container(page).center_x(Length::Fill))
        .direction(scrollable::Direction::Vertical(scrollable::Scrollbar::new().width(4).scroller_width(4).margin(2)))
        .id(iced::widget::Id::new(SCROLL_ID))
        .height(Length::Fill)
        .into()
}

fn blocks<'a>(list: &'a [Block], look: Look) -> Element<'a, Message> {
    column(list.iter().map(|b| block(b, look))).spacing(look.size * 0.8).width(Length::Fill).into()
}

fn block<'a>(b: &'a Block, look: Look) -> Element<'a, Message> {
    match b {
        Block::Heading { level, inlines, .. } => {
            let scale = match level {
                1 => 1.6,
                2 => 1.35,
                3 => 1.15,
                _ => 1.0,
            };
            let mut look = look;
            look.size *= scale;
            container(paragraph(inlines, look, true)).padding(Padding::new(0.0).top(look.size * 0.3)).into()
        }
        Block::Paragraph { inlines, .. } => paragraph(inlines, look, false),
        Block::Code { text: code, .. } => {
            let ground = look.muted(0.94);
            container(
                text(code.as_str())
                    .font(look.font(Style { code: true, ..Style::default() }))
                    .size(look.size * 0.85)
                    .line_height(1.45)
                    .color(look.text),
            )
            .padding(Padding::new(look.size * 0.7).left(look.size).right(look.size))
            .width(Length::Fill)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(ground)),
                border: Border::default().rounded(6.0),
                ..container::Style::default()
            })
            .into()
        }
        Block::Quote { blocks: inner, .. } => {
            let bar = look.muted(0.75);
            let mut quiet = look;
            quiet.text = look.muted(0.3);
            row![
                rule::vertical(3).style(move |_: &Theme| rule::Style {
                    color: bar,
                    radius: 2.0.into(),
                    fill_mode: rule::FillMode::Full,
                    snap: true,
                }),
                container(blocks(inner, quiet)).padding(Padding::new(0.0).left(look.size * 0.8)).width(Length::Fill),
            ]
            .height(Length::Shrink)
            .into()
        }
        Block::List { start, items, .. } => column(
            items.iter().enumerate().map(|(i, item)| list_item(item, marker(*start, i, item.checked, look), look)),
        )
        .spacing(look.size * 0.3)
        .into(),
        Block::Rule { .. } => {
            let line = look.muted(0.8);
            container(rule::horizontal(1).style(move |_: &Theme| rule::Style {
                color: line,
                radius: 0.0.into(),
                fill_mode: rule::FillMode::Full,
                snap: true,
            }))
            .padding(Padding::new(look.size * 0.5).left(0.0).right(0.0))
            .into()
        }
    }
}

/// The bullet, number or checkbox in front of a list item. Checkboxes are drawn, not
/// typed: the ballot-box glyphs are missing from the bundled fonts.
fn marker<'a>(start: Option<u64>, index: usize, checked: Option<bool>, look: Look) -> Element<'a, Message> {
    let label = |s: String| {
        text(s).font(look.font(Style::default())).size(look.size).line_height(look.line_height).color(look.muted(0.45))
    };
    let width = look.size * 1.6;
    match (checked, start) {
        (Some(done), _) => {
            let side = (look.size * 0.72).round();
            let (edge, fill) = (look.muted(0.45), look.accent);
            let tick: Element<'a, Message> = if done {
                text("✓").size(side * 0.9).line_height(1.0).color(look.background).center().into()
            } else {
                iced::widget::space().into()
            };
            let check = container(tick).center(side).style(move |_: &Theme| container::Style {
                background: done.then_some(Background::Color(fill)),
                border: Border { color: if done { fill } else { edge }, width: 1.5, radius: 3.0.into() },
                ..container::Style::default()
            });
            // Vertically centred on the first line of the item.
            container(check).width(width).height(look.size * look.line_height).center_y(look.size * look.line_height).into()
        }
        (None, Some(n)) => container(label(format!("{}.", n.saturating_add(index as u64)))).width(width).into(),
        (None, None) => container(label("•".to_owned())).width(width).into(),
    }
}

fn list_item<'a>(item: &'a Item, marker: Element<'a, Message>, look: Look) -> Element<'a, Message> {
    let mut content = look;
    if item.checked == Some(true) {
        content.text = look.muted(0.45);
    }
    row![marker, container(blocks(&item.blocks, content)).width(Length::Fill)].into()
}

fn paragraph<'a>(inlines: &'a [Inline], look: Look, heading: bool) -> Element<'a, Message> {
    let spans: Vec<Span<'a, String, Font>> = inlines
        .iter()
        .map(|inline| match inline {
            Inline::Break => span("\n"),
            Inline::Text { text, style, link } => {
                let style = Style { strong: style.strong || heading, ..*style };
                let color = match (link, style.image) {
                    (_, true) => look.muted(0.35),
                    (Some(_), false) => look.accent,
                    (None, false) => look.text,
                };
                let mut s = span(text.as_str())
                    .font(look.font(style))
                    .color(color)
                    .strikethrough(style.strike)
                    .underline(link.is_some() && !style.image)
                    .link_maybe(link.clone().filter(|url| openable(url)));
                if style.code {
                    s = s
                        .size(look.size * 0.88)
                        .background(Background::Color(look.muted(0.92)))
                        .padding(Padding::new(0.0).left(3.0).right(3.0))
                        .border(Border::default().rounded(3.0));
                }
                s
            }
        })
        .collect();
    rich_text(spans)
        .size(look.size)
        .line_height(look.line_height)
        .on_link_click(Message::OpenLink)
        .width(Length::Fill)
        .into()
}
