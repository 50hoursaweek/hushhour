//! Inkpad: an iA Writer style Markdown editor for Linux, built with iced.

mod editor;
mod highlight;
mod history;
mod palette;
mod stats;

use std::path::PathBuf;
use std::sync::Arc;

use iced::advanced::text::editor::{Cursor, Edit, Motion};
use iced::advanced::text::{LineHeight, Wrapping};
use iced::font::{Family, Stretch, Style as FontStyle, Weight};
use iced::keyboard::{self, Key, Modifiers};
use iced::widget::{column, container, row, space, text};

use editor::widget::{self as text_editor, TextEditor};
use history::{History, position_at};
use iced::{Background, Border, Color, Element, Font, Length, Padding, Subscription, Task, Theme};

use highlight::{FocusMode, MarkdownHighlighter, Settings as HlSettings, sentence_at};
use iced::advanced::text::editor::Position;
use palette::{ThemeKind, mix};
use stats::Stats;

const APP_NAME: &str = "Inkpad";
const MIN_SIZE: f32 = 12.0;
const MAX_SIZE: f32 = 40.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum FontKind {
    #[default]
    Duo,
    Mono,
    Quattro,
}

impl FontKind {
    fn family(self) -> &'static str {
        match self {
            Self::Duo => "iA Writer Duo S",
            Self::Mono => "iA Writer Mono S",
            Self::Quattro => "iA Writer Quattro S",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Duo => Self::Mono,
            Self::Mono => Self::Quattro,
            Self::Quattro => Self::Duo,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Duo => "Duo",
            Self::Mono => "Mono",
            Self::Quattro => "Quattro",
        }
    }

    fn font(self) -> Font {
        Font {
            family: Family::Name(self.family()),
            weight: Weight::Normal,
            stretch: Stretch::Normal,
            style: FontStyle::Normal,
        }
    }
}

struct Inkpad {
    content: text_editor::Content,
    /// Cached copy of the document, kept in sync after every edit.
    text: String,
    history: History,
    path: Option<PathBuf>,
    dirty: bool,
    focus: FocusMode,
    typewriter: bool,
    theme: ThemeKind,
    font: FontKind,
    size: f32,
    highlight: HlSettings,
    stats: Stats,
    notice: Option<String>,
}

#[derive(Debug, Clone)]
enum Message {
    Edit(text_editor::Action),
    New,
    Open,
    Opened(Option<(PathBuf, Arc<String>)>),
    Save,
    SaveAs,
    Saved(Result<PathBuf, String>),
    CycleFocus,
    ToggleTypewriter,
    CycleTheme,
    CycleFont,
    ZoomIn,
    ZoomOut,
    Undo,
    Redo,
    KeyPressed(Key, Modifiers),
    SelfTest,
    CursorToEnd,
}

impl Inkpad {
    fn new() -> (Self, Task<Message>) {
        let app = Self {
            content: text_editor::Content::new(),
            text: String::new(),
            history: History::default(),
            path: None,
            dirty: false,
            focus: FocusMode::Off,
            typewriter: false,
            theme: ThemeKind::Light,
            font: FontKind::Duo,
            size: 18.0,
            highlight: HlSettings::default(),
            stats: Stats::default(),
            notice: None,
        };
        let mut app = app;
        let mut tasks = vec![iced::widget::operation::focus_next()];
        for arg in std::env::args().skip(1) {
            match arg.as_str() {
                "--typewriter" => app.typewriter = true,
                "--end" => tasks.push(Task::done(Message::CursorToEnd)),
                "--selftest" => tasks.push(Task::done(Message::SelfTest)),
                path => tasks.push(Task::perform(read_file(PathBuf::from(path)), Message::Opened)),
            }
        }
        (app, Task::batch(tasks))
    }

    fn title(&self) -> String {
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_owned());
        let flag = if self.dirty { "•" } else { "" };
        format!("{flag}{name} — {APP_NAME}")
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Edit(action) => {
                let is_edit = action.is_edit();
                let before = self.content.cursor().position;
                self.content.perform(action);
                if is_edit {
                    let new_text = self.content.text();
                    let after = self.content.cursor().position;
                    if self.history.record(&self.text, &new_text, before, after) {
                        self.dirty = true;
                        self.notice = None;
                    }
                    self.stats = stats::compute(&new_text);
                    self.text = new_text;
                }
                self.refresh_focus();
                Task::none()
            }
            Message::Undo => {
                if let Some(d) = self.history.undo() {
                    self.apply(d.offset, d.inserted.len(), &d.removed, d.cursor_before);
                }
                Task::none()
            }
            Message::Redo => {
                if let Some(d) = self.history.redo() {
                    self.apply(d.offset, d.removed.len(), &d.inserted, d.cursor_after);
                }
                Task::none()
            }
            Message::New => {
                self.load(String::new());
                self.path = None;
                Task::none()
            }
            Message::Open => Task::perform(open_dialog(), Message::Opened),
            Message::Opened(Some((path, text))) => {
                self.load(text.replace("\r\n", "\n"));
                self.path = Some(path);
                Task::none()
            }
            Message::Opened(None) => Task::none(),
            Message::Save => match self.path.clone() {
                Some(path) => Task::perform(write_file(path, self.content.text()), Message::Saved),
                None => self.update(Message::SaveAs),
            },
            Message::SaveAs => Task::perform(save_dialog(self.content.text()), Message::Saved),
            Message::Saved(Ok(path)) => {
                self.path = Some(path);
                self.dirty = false;
                self.notice = Some("Saved".to_owned());
                Task::none()
            }
            Message::Saved(Err(e)) => {
                self.notice = Some(format!("Save failed: {e}"));
                Task::none()
            }
            Message::CycleFocus => {
                self.focus = self.focus.next();
                self.refresh_focus();
                Task::none()
            }
            Message::ToggleTypewriter => {
                self.typewriter = !self.typewriter;
                Task::none()
            }
            Message::CycleTheme => {
                self.theme = self.theme.next();
                Task::none()
            }
            Message::CycleFont => {
                self.font = self.font.next();
                self.refresh_focus();
                Task::none()
            }
            Message::ZoomIn => {
                self.size = (self.size + 1.0).min(MAX_SIZE);
                Task::none()
            }
            Message::ZoomOut => {
                self.size = (self.size - 1.0).max(MIN_SIZE);
                Task::none()
            }
            Message::CursorToEnd => {
                self.content.perform(text_editor::Action::Move(Motion::DocumentEnd));
                self.refresh_focus();
                Task::none()
            }
            Message::SelfTest => match self.selftest() {
                Ok(()) => {
                    println!("SELFTEST PASS");
                    iced::exit()
                }
                Err(e) => {
                    println!("SELFTEST FAIL: {e}");
                    iced::exit()
                }
            },
            Message::KeyPressed(key, modifiers) => match shortcut(&key, modifiers) {
                Some(m) => self.update(m),
                None => Task::none(),
            },
        }
    }

    /// Replace the document, resetting history and statistics.
    fn load(&mut self, text: String) {
        self.content = text_editor::Content::with_text(&text);
        self.stats = stats::compute(&text);
        self.text = text;
        self.history.clear();
        self.dirty = false;
        self.notice = None;
        self.refresh_focus();
    }

    /// Replace `remove_len` bytes at `offset` with `insert`, then place the cursor.
    /// Used to apply undo and redo steps through the editor's own actions.
    fn apply(&mut self, offset: usize, remove_len: usize, insert: &str, cursor: Position) {
        let start = position_at(&self.text, offset);
        // Collapse any selection first; a stale one would be replaced by the paste.
        self.content.perform(text_editor::Action::Move(Motion::Left));
        if remove_len > 0 {
            let end = position_at(&self.text, offset + remove_len);
            self.content.move_to(Cursor { position: start, selection: Some(end) });
            self.content.perform(text_editor::Action::Edit(Edit::Delete));
        } else {
            self.content.move_to(Cursor { position: start, selection: None });
        }
        if !insert.is_empty() {
            self.content
                .perform(text_editor::Action::Edit(Edit::Paste(Arc::new(insert.to_owned()))));
        }
        self.content.move_to(Cursor { position: cursor, selection: None });
        self.text = self.content.text();
        self.stats = stats::compute(&self.text);
        self.dirty = true;
        self.refresh_focus();
    }

    /// Scripted end-to-end check of typing, selection replacement, undo and redo
    /// through the real editor. Run with `--selftest`.
    fn selftest(&mut self) -> Result<(), String> {
        use text_editor::Action;
        let step = |app: &mut Self, msg: Message, want: &str, label: &str| -> Result<(), String> {
            let _ = app.update(msg);
            let got = app.content.text();
            if got == want { Ok(()) } else { Err(format!("{label}: got {got:?}, want {want:?}")) }
        };
        let edit = |e: Edit| Message::Edit(Action::Edit(e));

        self.load("Hello world.\nSecond line.".to_owned());
        let _ = self.update(Message::CursorToEnd);
        for c in " More".chars() {
            let _ = self.update(edit(Edit::Insert(c)));
        }
        step(self, Message::Edit(Action::Move(Motion::End)), "Hello world.\nSecond line. More", "typed")?;
        step(self, edit(Edit::Enter), "Hello world.\nSecond line. More\n", "enter")?;
        for c in "New".chars() {
            let _ = self.update(edit(Edit::Insert(c)));
        }
        step(self, Message::Edit(Action::Move(Motion::End)), "Hello world.\nSecond line. More\nNew", "typed 2")?;

        step(self, Message::Undo, "Hello world.\nSecond line. More\n", "undo word")?;
        step(self, Message::Undo, "Hello world.\nSecond line. More", "undo enter")?;
        step(self, Message::Undo, "Hello world.\nSecond line.", "undo ' More'")?;
        step(self, Message::Undo, "Hello world.\nSecond line.", "undo at bottom is a no-op")?;
        step(self, Message::Redo, "Hello world.\nSecond line. More", "redo 1")?;
        step(self, Message::Redo, "Hello world.\nSecond line. More\n", "redo 2")?;
        step(self, Message::Redo, "Hello world.\nSecond line. More\nNew", "redo 3")?;
        let c = self.content.cursor().position;
        if (c.line, c.column) != (2, 3) {
            return Err(format!("cursor after redo: {:?}", (c.line, c.column)));
        }

        // Replace a selection, then undo it.
        self.content.move_to(Cursor { position: Position { line: 0, column: 6 }, selection: Some(Position { line: 0, column: 11 }) });
        self.refresh_focus();
        step(self, edit(Edit::Insert('X')), "Hello X.\nSecond line. More\nNew", "replace selection")?;
        step(self, Message::Undo, "Hello world.\nSecond line. More\nNew", "undo replace")?;
        step(self, Message::Redo, "Hello X.\nSecond line. More\nNew", "redo replace")?;

        // Backspace coalescing and multi-byte text.
        let _ = self.update(Message::CursorToEnd);
        for c in "è!".chars() {
            let _ = self.update(edit(Edit::Insert(c)));
        }
        step(self, Message::Edit(Action::Move(Motion::End)), "Hello X.\nSecond line. More\nNewè!", "unicode typed")?;
        step(self, edit(Edit::Backspace), "Hello X.\nSecond line. More\nNewè", "bs 1")?;
        step(self, edit(Edit::Backspace), "Hello X.\nSecond line. More\nNew", "bs 2")?;
        step(self, Message::Undo, "Hello X.\nSecond line. More\nNewè!", "undo both backspaces")?;
        Ok(())
    }

    /// Recompute the paragraph and sentence under the cursor for focus mode.
    fn refresh_focus(&mut self) {
        let cursor = self.content.cursor();
        let line = cursor.position.line;
        let col = cursor.position.column;

        let blank: Vec<bool> = self.content.lines().map(|l| l.text.trim().is_empty()).collect();
        let mut start = line;
        while start > 0 && !blank[start - 1] {
            start -= 1;
        }
        let mut end = line + 1;
        while end < blank.len() && !blank[end] {
            end += 1;
        }

        let sentence = self
            .content
            .line(line)
            .map(|l| sentence_at(&l.text, col))
            .unwrap_or(0..0);

        self.highlight = HlSettings {
            family: self.font.family(),
            focus: self.focus,
            paragraph: start..end,
            cursor_line: line,
            sentence,
        };
    }

    fn subscription(&self) -> Subscription<Message> {
        keyboard::listen().filter_map(|event| match event {
            keyboard::Event::KeyPressed { key, modifiers, .. } => Some(Message::KeyPressed(key, modifiers)),
            _ => None,
        })
    }

    fn view(&self) -> Element<'_, Message> {
        let font = self.font.font();

        let editor = TextEditor::new(&self.content)
            .placeholder("Start writing…")
            .typewriter(self.typewriter)
            .font(font)
            .size(self.size)
            .line_height(LineHeight::Relative(1.65))
            .padding(Padding::new(64.0).left(32.0).right(32.0))
            .wrapping(Wrapping::Word)
            .height(Length::Fill)
            .highlight_with::<MarkdownHighlighter>(self.highlight.clone(), highlight::to_format)
            .key_binding(|press| {
                use text_editor::Binding;
                if let Some(m) = shortcut(&press.key, press.modifiers) {
                    return Some(Binding::Custom(m));
                }
                Binding::from_key_press(press)
            })
            .on_action(Message::Edit)
            .style(|theme: &Theme, _status| {
                let p = theme.palette();
                text_editor::Style {
                    background: Background::Color(p.background),
                    border: Border::default(),
                    placeholder: mix(p.text, p.background, 0.6),
                    value: p.text,
                    selection: Color { a: 0.35, ..p.primary },
                }
            });

        let page = container(editor)
            .max_width(760.0 + 64.0)
            .center_x(Length::Fill)
            .height(Length::Fill);

        let muted = |s: String| text(s).size(12.0).style(|theme: &Theme| {
            let p = theme.palette();
            text::Style { color: Some(mix(p.text, p.background, 0.45)) }
        });

        let status = row![
            muted(format!("{} words", self.stats.words)),
            muted("·".into()),
            muted(format!("{} characters", self.stats.characters)),
            muted("·".into()),
            muted(self.stats.reading_time()),
            space().width(Length::Fill),
            muted(self.notice.clone().unwrap_or_default()),
            space().width(16),
            muted(format!("Focus: {}", self.focus.label())),
            muted("·".into()),
            muted(format!("Typewriter: {}", if self.typewriter { "On" } else { "Off" })),
            muted("·".into()),
            muted(format!("{} · {}", self.font.label(), self.theme.label())),
        ]
        .spacing(8)
        .padding(Padding::new(6.0).left(16.0).right(16.0));

        let layout = column![page, status].height(Length::Fill);

        container(layout)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(theme.palette().background)),
                ..container::Style::default()
            })
            .into()
    }
}

/// Keyboard shortcuts, shared by the editor key binding and the global listener.
fn shortcut(key: &Key, modifiers: Modifiers) -> Option<Message> {
    if !modifiers.command() {
        return None;
    }
    let shift = modifiers.shift();
    match key.as_ref() {
        Key::Character("s") if shift => Some(Message::SaveAs),
        Key::Character("s") => Some(Message::Save),
        Key::Character("S") => Some(Message::SaveAs),
        Key::Character("o") => Some(Message::Open),
        Key::Character("n") => Some(Message::New),
        Key::Character("d") => Some(Message::CycleFocus),
        Key::Character("t") if shift => Some(Message::CycleTheme),
        Key::Character("T") => Some(Message::CycleTheme),
        Key::Character("t") => Some(Message::ToggleTypewriter),
        Key::Character("f") if shift => Some(Message::CycleFont),
        Key::Character("F") => Some(Message::CycleFont),
        Key::Character("=") | Key::Character("+") => Some(Message::ZoomIn),
        Key::Character("-") => Some(Message::ZoomOut),
        Key::Character("z") if shift => Some(Message::Redo),
        Key::Character("Z") => Some(Message::Redo),
        Key::Character("z") => Some(Message::Undo),
        Key::Character("y") => Some(Message::Redo),
        _ => None,
    }
}

async fn open_dialog() -> Option<(PathBuf, Arc<String>)> {
    let handle = rfd::AsyncFileDialog::new()
        .add_filter("Markdown", &["md", "markdown", "txt"])
        .pick_file()
        .await?;
    read_file(handle.path().to_path_buf()).await
}

async fn read_file(path: PathBuf) -> Option<(PathBuf, Arc<String>)> {
    let text = std::fs::read_to_string(&path).ok()?;
    Some((path, Arc::new(text)))
}

async fn save_dialog(contents: String) -> Result<PathBuf, String> {
    let handle = rfd::AsyncFileDialog::new()
        .add_filter("Markdown", &["md", "markdown", "txt"])
        .set_file_name("Untitled.md")
        .save_file()
        .await
        .ok_or_else(|| "cancelled".to_owned())?;
    write_file(handle.path().to_path_buf(), contents).await
}

async fn write_file(path: PathBuf, contents: String) -> Result<PathBuf, String> {
    std::fs::write(&path, contents).map_err(|e| e.to_string())?;
    Ok(path)
}

pub fn main() -> iced::Result {
    iced::application(Inkpad::new, Inkpad::update, Inkpad::view)
        .title(Inkpad::title)
        .theme(|app: &Inkpad| app.theme.theme())
        .subscription(Inkpad::subscription)
        .default_font(FontKind::Duo.font())
        .font(include_bytes!("../assets/fonts/iAWriterDuoS-Regular.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/iAWriterDuoS-Bold.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/iAWriterDuoS-Italic.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/iAWriterDuoS-BoldItalic.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/iAWriterMonoS-Regular.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/iAWriterMonoS-Bold.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/iAWriterMonoS-Italic.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/iAWriterMonoS-BoldItalic.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/iAWriterQuattroS-Regular.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/iAWriterQuattroS-Bold.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/iAWriterQuattroS-Italic.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/iAWriterQuattroS-BoldItalic.ttf").as_slice())
        .window_size((1100.0, 780.0))
        .antialiasing(true)
        .run()
}
