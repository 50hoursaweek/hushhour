//! Inkpad: an iA Writer style Markdown editor for Linux, built with iced.

mod editor;
mod highlight;
mod history;
mod music;
mod palette;
mod prefs;
mod sound;
mod stats;

use std::path::PathBuf;
use std::sync::Arc;

use iced::advanced::text::editor::{Cursor, Edit, Motion};
use iced::advanced::text::{LineHeight, Wrapping};
use iced::keyboard::{self, Key, Modifiers};
use iced::keyboard::key::Named;
use iced::widget::{
    button, column, container, scrollable, mouse_area, opaque, pick_list, row, rule, slider, space, stack, text, toggler,
};
use iced::window;

use editor::widget::{self as text_editor, TextEditor};
use history::{History, position_at};
use iced::{Background, Border, Color, Element, Length, Padding, Shadow, Subscription, Task, Theme, Vector};

use highlight::{FocusMode, MarkdownHighlighter, Settings as HlSettings, sentence_at};
use iced::advanced::text::editor::Position;
use palette::{ThemeKind, mix};
use music::Music;
use prefs::{FontKind, MAX_LINE_HEIGHT, MAX_SIZE, MIN_LINE_HEIGHT, MIN_SIZE, Preferences, SoundSet};
use stats::Stats;

const APP_NAME: &str = "Inkpad";
/// Incremented by build.rs on every build; see the About section of the menu.
const BUILD: &str = env!("INKPAD_BUILD");
const DRAG_STRIP: f32 = 28.0;
// Text column layout. The measure is set in characters so it scales with the font;
// effective width = min(window width - 2 * padding, MEASURE_CH ch), centred.
/// Maximum line length in characters (`ch`, the width of the `0` glyph).
const MEASURE_CH: f32 = 66.0;
/// Width of one `ch` in em for the bundled iA Writer fonts.
const CH_EM: f32 = 0.6;
/// Minimum horizontal padding on each side of the text, in em of the body size.
const SIDE_PADDING_EM: f32 = 1.75;
/// Body size steps with window width: points added to the preferred size at or above each width.
/// With the default 18 pt this gives 18 px below 900, 20 px from 900 and 22 px from 1400.
const SIZE_STEPS: [(f32, f32); 2] = [(900.0, 2.0), (1400.0, 4.0)];
const EDGE: f32 = 6.0;
/// Corner radius of the undecorated window, matching typical desktop windows.
const CORNER_RADIUS: f32 = 12.0;
const WINDOW_SIZE: (f32, f32) = (1100.0, 780.0);

struct Inkpad {
    content: text_editor::Content,
    /// Cached copy of the document, kept in sync after every edit.
    text: String,
    history: History,
    path: Option<PathBuf>,
    dirty: bool,
    prefs: Preferences,
    sound: sound::Sound,
    music_dir: PathBuf,
    /// Menu options for the music picker: the built-ins, then the files found in `music_dir`.
    music_options: Vec<Music>,
    menu_open: bool,
    fullscreen: bool,
    maximized: bool,
    window_width: f32,
    window_height: f32,
    top_strip_hovered: bool,
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
    SoundTest,
    CursorToEnd,
    DragWindow,
    ResizeWindow(window::Direction),
    TopStripHovered(bool),
    ToggleMaximize,
    Minimize,
    ToggleFullscreen,
    WindowResized(iced::Size),
    Maximized(bool),
    ToggleDecorations,
    ToggleMenu,
    CloseMenu,
    SetTheme(ThemeKind),
    SetFont(FontKind),
    SetSize(f32),
    SetLineHeight(f32),
    SetFocus(FocusMode),
    SetTypewriter(bool),
    SetTypingSound(bool),
    SetSoundSet(SoundSet),
    SetSoundVolume(f32),
    SetMusic(Music),
    SetMusicVolume(f32),
    Quit,
}

impl Inkpad {
    fn new() -> (Self, Task<Message>) {
        let music_dir = music::music_dir().unwrap_or_else(|| PathBuf::from("music"));
        // Create the folder so it is easy to find and drop tracks into.
        let _ = std::fs::create_dir_all(&music_dir);
        let app = Self {
            content: text_editor::Content::new(),
            text: String::new(),
            history: History::default(),
            path: None,
            dirty: false,
            prefs: Preferences::load(),
            sound: sound::Sound::start(music_dir.clone()),
            music_dir,
            music_options: Vec::new(),
            menu_open: false,
            fullscreen: false,
            maximized: false,
            window_width: WINDOW_SIZE.0,
            window_height: WINDOW_SIZE.1,
            top_strip_hovered: false,
            highlight: HlSettings::default(),
            stats: Stats::default(),
            notice: None,
        };
        let mut app = app;
        app.refresh_music_options();
        app.sound.set_music_volume(app.prefs.music_volume);
        app.sound.set_music(app.prefs.music.clone());
        let mut tasks = vec![iced::widget::operation::focus_next()];
        for arg in std::env::args().skip(1) {
            match arg.as_str() {
                "--typewriter" => app.prefs.typewriter = true,
                "--menu" => app.menu_open = true,
                "--soundtest" => tasks.push(Task::done(Message::SoundTest)),
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
                let key = match &action {
                    text_editor::Action::Edit(Edit::Enter) => Some(sound::Key::Enter),
                    text_editor::Action::Edit(Edit::Backspace | Edit::Delete) => Some(sound::Key::Backspace),
                    text_editor::Action::Edit(_) => Some(sound::Key::Character),
                    _ => None,
                };
                let before = self.content.cursor().position;
                self.content.perform(action);
                if let Some(key) = key
                    && self.prefs.typing_sound
                {
                    self.sound.play(key, self.prefs.sound_set, self.prefs.sound_volume);
                }
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
            Message::CycleFocus => self.update(Message::SetFocus(self.prefs.focus.next())),
            Message::ToggleTypewriter => self.update(Message::SetTypewriter(!self.prefs.typewriter)),
            Message::CycleTheme => self.update(Message::SetTheme(self.prefs.theme.next())),
            Message::CycleFont => self.update(Message::SetFont(self.prefs.font.next())),
            Message::ZoomIn => self.update(Message::SetSize(self.prefs.size + 1.0)),
            Message::ZoomOut => self.update(Message::SetSize(self.prefs.size - 1.0)),
            Message::SetFocus(focus) => {
                self.prefs.focus = focus;
                self.refresh_focus();
                self.save_prefs()
            }
            Message::SetTypewriter(on) => {
                self.prefs.typewriter = on;
                self.save_prefs()
            }
            Message::SetTypingSound(on) => {
                self.prefs.typing_sound = on;
                self.save_prefs()
            }
            Message::SetSoundSet(set) => {
                self.prefs.sound_set = set;
                self.sound.play(sound::Key::Character, set, self.prefs.sound_volume);
                self.save_prefs()
            }
            Message::SetSoundVolume(v) => {
                self.prefs.sound_volume = ((v * 20.0).round() / 20.0).clamp(0.0, 1.0);
                // Preview the new level right away.
                self.sound.play(sound::Key::Character, self.prefs.sound_set, self.prefs.sound_volume);
                self.save_prefs()
            }
            Message::SetTheme(theme) => {
                self.prefs.theme = theme;
                self.save_prefs()
            }
            Message::SetFont(font) => {
                self.prefs.font = font;
                self.refresh_focus();
                self.save_prefs()
            }
            Message::SetSize(size) => {
                self.prefs.size = size.round().clamp(MIN_SIZE, MAX_SIZE);
                self.save_prefs()
            }
            Message::SetLineHeight(lh) => {
                self.prefs.line_height = ((lh * 20.0).round() / 20.0).clamp(MIN_LINE_HEIGHT, MAX_LINE_HEIGHT);
                self.save_prefs()
            }
            Message::ToggleMenu => {
                self.menu_open = !self.menu_open;
                if self.menu_open {
                    self.refresh_music_options();
                }
                Task::none()
            }
            Message::SetMusic(music) => {
                self.prefs.music = music.clone();
                self.sound.set_music(music);
                self.save_prefs()
            }
            Message::SetMusicVolume(v) => {
                self.prefs.music_volume = ((v * 20.0).round() / 20.0).clamp(0.0, 1.0);
                self.sound.set_music_volume(self.prefs.music_volume);
                self.save_prefs()
            }
            Message::CloseMenu => {
                self.menu_open = false;
                Task::none()
            }
            Message::DragWindow => window::latest().and_then(window::drag),
            Message::ResizeWindow(direction) => {
                window::latest().and_then(move |id| window::drag_resize(id, direction))
            }
            Message::TopStripHovered(hovered) => {
                self.top_strip_hovered = hovered;
                Task::none()
            }
            Message::ToggleMaximize => window::latest().and_then(window::toggle_maximize),
            Message::Minimize => window::latest().and_then(|id| window::minimize(id, true)),
            Message::ToggleDecorations => window::latest().and_then(window::toggle_decorations),
            // The compositor does not announce maximisation, so ask after every resize;
            // the corners are square while maximised or fullscreen.
            Message::WindowResized(size) => {
                self.window_width = size.width;
                self.window_height = size.height;
                window::latest().and_then(window::is_maximized).map(Message::Maximized)
            }
            Message::Maximized(maximized) => {
                self.maximized = maximized;
                Task::none()
            }
            Message::ToggleFullscreen => {
                self.fullscreen = !self.fullscreen;
                let mode = if self.fullscreen { window::Mode::Fullscreen } else { window::Mode::Windowed };
                window::latest().and_then(move |id| window::set_mode(id, mode))
            }
            Message::Quit => iced::exit(),
            Message::CursorToEnd => {
                self.content.perform(text_editor::Action::Move(Motion::DocumentEnd));
                self.refresh_focus();
                Task::none()
            }
            // Play a few keys at full volume and exit; used to check audio output.
            Message::SoundTest => {
                for key in [sound::Key::Character, sound::Key::Character, sound::Key::Enter, sound::Key::Backspace] {
                    self.sound.play(key, self.prefs.sound_set, 1.0);
                    std::thread::sleep(std::time::Duration::from_millis(180));
                }
                std::thread::sleep(std::time::Duration::from_millis(700));
                println!("SOUNDTEST DONE");
                iced::exit()
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
            Message::KeyPressed(key, modifiers) => {
                if self.menu_open && matches!(key, Key::Named(Named::Escape)) {
                    return self.update(Message::CloseMenu);
                }
                match shortcut(&key, modifiers) {
                    Some(m) => self.update(m),
                    None => Task::none(),
                }
            }
        }
    }

    /// Rescan the music folder for the picker.
    fn refresh_music_options(&mut self) {
        self.music_options = Music::BUILT_IN.to_vec();
        self.music_options.extend(music::scan_music_dir(&self.music_dir).into_iter().map(Music::File));
    }

    /// Persist the preferences; a failure is reported in the status bar rather than fatal.
    fn save_prefs(&mut self) -> Task<Message> {
        if let Err(e) = self.prefs.save() {
            self.notice = Some(format!("Could not save preferences: {e}"));
        }
        Task::none()
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

    /// The preferred size plus the step for the current window width, so the
    /// column fills more of a large screen.
    fn body_size(&self) -> f32 {
        let step = SIZE_STEPS
            .iter()
            .filter(|(min_width, _)| self.window_width >= *min_width)
            .map(|(_, step)| *step)
            .last()
            .unwrap_or(0.0);
        (self.prefs.size + step).min(MAX_SIZE)
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
            family: self.prefs.font.family(),
            focus: self.prefs.focus,
            paragraph: start..end,
            cursor_line: line,
            sentence,
        };
    }

    /// The menu: file actions and preferences in a card over the editor.
    /// Clicking outside the card or pressing Escape closes it.
    fn menu(&self) -> Element<'_, Message> {
        let muted = |s: String| {
            text(s).size(12.0).style(|theme: &Theme| {
                let p = theme.palette();
                text::Style { color: Some(mix(p.text, p.background, 0.45)) }
            })
        };
        let heading = |s: &'static str| {
            text(s).size(11.0).style(|theme: &Theme| {
                let p = theme.palette();
                text::Style { color: Some(mix(p.text, p.background, 0.55)) }
            })
        };
        let action = |label: &'static str, hint: &'static str, msg: Message| {
            button(
                row![text(label).size(13.0), space().width(Length::Fill), muted(hint.to_owned())]
                    .align_y(iced::Alignment::Center),
            )
            .width(Length::Fill)
            .padding(Padding::new(5.0).left(8.0).right(8.0))
            .on_press(msg)
            .style(|theme: &Theme, status| {
                let p = theme.palette();
                let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: hovered.then(|| Background::Color(mix(p.text, p.background, 0.9))),
                    text_color: p.text,
                    border: Border::default().rounded(4.0),
                    ..button::Style::default()
                }
            })
        };
        let divider = || rule::horizontal(1).style(|theme: &Theme| {
            let p = theme.palette();
            rule::Style {
                color: mix(p.text, p.background, 0.85),
                radius: 0.0.into(),
                fill_mode: rule::FillMode::Full,
                snap: false,
            }
        });

        let size = self.prefs.size;
        let line_height = self.prefs.line_height;
        let volume = self.prefs.sound_volume;
        let music_volume = self.prefs.music_volume;
        let card = column![
            heading("FILE"),
            action("New", "Ctrl+N", Message::New),
            action("Open…", "Ctrl+O", Message::Open),
            action("Save", "Ctrl+S", Message::Save),
            action("Save As…", "Ctrl+Shift+S", Message::SaveAs),
            divider(),
            heading("APPEARANCE"),
            setting("Theme", pick(&ThemeKind::ALL, self.prefs.theme, Message::SetTheme)),
            setting("Font", pick(&FontKind::ALL, self.prefs.font, Message::SetFont)),
            setting(
                "Size",
                row![
                    slider(MIN_SIZE..=MAX_SIZE, size, Message::SetSize).step(1.0_f32),
                    muted(format!("{size:.0} pt")).width(44.0),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            ),
            setting(
                "Line height",
                row![
                    slider(MIN_LINE_HEIGHT..=MAX_LINE_HEIGHT, line_height, Message::SetLineHeight).step(0.05_f32),
                    muted(format!("{line_height:.2}")).width(44.0),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            ),
            divider(),
            heading("WRITING"),
            setting("Focus", pick(&FocusMode::ALL, self.prefs.focus, Message::SetFocus)),
            setting(
                "Typewriter",
                toggler(self.prefs.typewriter).on_toggle(Message::SetTypewriter).size(18.0),
            ),
            divider(),
            heading("SOUND"),
            setting(
                "Typing sound",
                toggler(self.prefs.typing_sound).on_toggle(Message::SetTypingSound).size(18.0),
            ),
            setting("Sound", pick(&SoundSet::ALL, self.prefs.sound_set, Message::SetSoundSet)),
            setting(
                "Volume",
                row![
                    slider(0.0..=1.0, volume, Message::SetSoundVolume).step(0.05_f32),
                    muted(format!("{:.0}%", volume * 100.0)).width(44.0),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            ),
            divider(),
            heading("MUSIC"),
            setting("Music", pick(&self.music_options, self.prefs.music.clone(), Message::SetMusic)),
            setting(
                "Volume",
                row![
                    slider(0.0..=1.0, music_volume, Message::SetMusicVolume).step(0.05_f32),
                    muted(format!("{:.0}%", music_volume * 100.0)).width(44.0),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            ),
            muted(format!("Add your own tracks (mp3, ogg, flac, wav) in\n{}", self.music_dir.display())),
            divider(),
            heading("WINDOW"),
            action("Fullscreen", "F11", Message::ToggleFullscreen),
            action("Toggle decorations", "Ctrl+Shift+D", Message::ToggleDecorations),
            action("Quit", "Ctrl+Q", Message::Quit),
            divider(),
            heading("ABOUT"),
            muted(format!("{APP_NAME} {} · build {BUILD}", env!("CARGO_PKG_VERSION"))),
        ]
        .spacing(4)
        .width(340.0);

        // Scroll when the card is taller than the window, so nothing is clipped.
        let card = container(scrollable(container(card).padding(Padding::new(0.0).right(8.0))))
            .max_height((self.window_height - DRAG_STRIP - 24.0).max(200.0))
            .padding(Padding::new(16.0).right(8.0))
            .style(|theme: &Theme| {
                let p = theme.palette();
                container::Style {
                    background: Some(Background::Color(p.background)),
                    border: Border::default().rounded(10.0).width(1.0).color(mix(p.text, p.background, 0.8)),
                    shadow: Shadow {
                        color: Color { a: 0.25, ..Color::BLACK },
                        offset: Vector::new(0.0, 6.0),
                        blur_radius: 24.0,
                    },
                    ..container::Style::default()
                }
            });

        // The card hangs below the ☰ button; the backdrop swallows clicks so they
        // close the menu instead of reaching the editor.
        let anchored = container(opaque(card))
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(Padding::new(0.0).top(DRAG_STRIP + 2.0).left(8.0));
        opaque(mouse_area(anchored).on_press(Message::CloseMenu))
    }

    fn subscription(&self) -> Subscription<Message> {
        let keys = keyboard::listen().filter_map(|event| match event {
            keyboard::Event::KeyPressed { key, modifiers, .. } => Some(Message::KeyPressed(key, modifiers)),
            _ => None,
        });
        let resizes = window::events().filter_map(|(_, event)| match event {
            window::Event::Resized(size) => Some(Message::WindowResized(size)),
            _ => None,
        });
        Subscription::batch([keys, resizes])
    }

    fn view(&self) -> Element<'_, Message> {
        let font = self.prefs.font.font();
        let menu_open = self.menu_open;
        let size = self.body_size();
        let side_padding = size * SIDE_PADDING_EM;
        let measure = size * CH_EM * MEASURE_CH;

        let editor = TextEditor::new(&self.content)
            .placeholder("Start writing…")
            .typewriter(self.prefs.typewriter)
            .font(font)
            .size(size)
            .line_height(LineHeight::Relative(self.prefs.line_height))
            .padding(Padding::new(64.0).top(64.0 - DRAG_STRIP).left(side_padding).right(side_padding))
            .wrapping(Wrapping::Word)
            .height(Length::Fill)
            .highlight_with::<MarkdownHighlighter>(self.highlight.clone(), highlight::to_format)
            .key_binding(move |press| {
                use text_editor::Binding;
                if let Some(m) = shortcut(&press.key, press.modifiers) {
                    return Some(Binding::Custom(m));
                }
                if menu_open {
                    // Keys go to the menu, not the document, while it is open.
                    return matches!(press.key, Key::Named(Named::Escape))
                        .then_some(Binding::Custom(Message::CloseMenu));
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

        // The column is capped at the measure; the outer container centres it, so the
        // margins stay equal and grow together as the window widens.
        let column = container(editor)
            .width(Length::Fill)
            .max_width(measure + 2.0 * side_padding)
            .height(Length::Fill);
        let page = container(column).center_x(Length::Fill).height(Length::Fill);

        // Without window decorations this strip is the handle for moving the
        // window; double-click maximises, like a title bar would.
        let window_button = |label: &'static str, msg: Message| {
            button(text(label).size(14.0).center().width(Length::Fill).height(Length::Fill))
                .width(DRAG_STRIP + 8.0)
                .height(DRAG_STRIP)
                .padding(0)
                .on_press(msg)
                .style(|theme: &Theme, status| {
                    let p = theme.palette();
                    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
                    button::Style {
                        background: hovered.then(|| iced::Background::Color(mix(p.text, p.background, 0.85))),
                        text_color: mix(p.text, p.background, if hovered { 0.0 } else { 0.45 }),
                        border: Border::default().rounded(4.0),
                        ..button::Style::default()
                    }
                })
        };
        // Window controls appear only while the pointer is in the top strip.
        let controls: Element<'_, Message> = if self.top_strip_hovered {
            row![
                window_button("–", Message::Minimize),
                window_button("□", Message::ToggleMaximize),
                window_button("×", Message::Quit),
            ]
            .spacing(2)
            .padding(Padding::new(0.0).right(8.0))
            .into()
        } else {
            space().width(0).into()
        };
        let drag_handle = mouse_area(space().width(Length::Fill).height(DRAG_STRIP))
            .on_press(Message::DragWindow)
            .on_double_click(Message::ToggleMaximize)
            .interaction(iced::mouse::Interaction::Grab);
        // The menu button is always visible, muted until hovered, so the menu can be found.
        let menu_button = container(window_button("☰", Message::ToggleMenu)).padding(Padding::new(0.0).left(8.0));
        let drag_strip = mouse_area(row![menu_button, drag_handle, controls].height(DRAG_STRIP))
            .on_enter(Message::TopStripHovered(true))
            .on_exit(Message::TopStripHovered(false));

        let muted = |s: String| {
            text(s).size(12.0).style(|theme: &Theme| {
                let p = theme.palette();
                text::Style { color: Some(mix(p.text, p.background, 0.45)) }
            })
        };
        // Status bar items that act as flat buttons: muted text, highlighted on hover.
        let item = |label: String, active: bool, msg: Message| {
            button(text(label).size(12.0))
                .padding(Padding::new(2.0).left(6.0).right(6.0))
                .on_press(msg)
                .style(move |theme: &Theme, status| {
                    let p = theme.palette();
                    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
                    let color = if hovered || active {
                        p.text
                    } else {
                        mix(p.text, p.background, 0.45)
                    };
                    button::Style {
                        background: hovered.then(|| iced::Background::Color(mix(p.text, p.background, 0.9))),
                        text_color: color,
                        border: Border::default().rounded(4.0),
                        ..button::Style::default()
                    }
                })
        };

        let status = row![
            muted(format!("{} words", self.stats.words)),
            muted("·".into()),
            muted(format!("{} characters", self.stats.characters)),
            muted("·".into()),
            muted(self.stats.reading_time()),
            space().width(Length::Fill),
            muted(self.notice.clone().unwrap_or_default()),
            space().width(16),
            item(
                format!("Focus: {}", self.prefs.focus.label()),
                self.prefs.focus != FocusMode::Off,
                Message::CycleFocus,
            ),
            item(
                format!("Typewriter: {}", if self.prefs.typewriter { "On" } else { "Off" }),
                self.prefs.typewriter,
                Message::ToggleTypewriter,
            ),
            item(self.prefs.font.label().to_owned(), false, Message::CycleFont),
            item(self.prefs.theme.label().to_owned(), false, Message::CycleTheme),
        ]
        .spacing(4)
        .align_y(iced::Alignment::Center)
        .padding(Padding::new(4.0).left(16.0).right(12.0));

        let status = mouse_area(status).on_press(Message::DragWindow);
        let layout = column![drag_strip, page, status].height(Length::Fill);

        // Invisible resize handles along the edges and corners of the window.
        let handles = {
            use iced::mouse::Interaction as I;
            use window::Direction as D;
            let h = |w: Length, hgt: Length, dir: D, cursor: I| {
                mouse_area(space().width(w).height(hgt))
                    .on_press(Message::ResizeWindow(dir))
                    .interaction(cursor)
            };
            let (edge, fill) = (Length::Fixed(EDGE), Length::Fill);
            column![
                row![
                    h(edge, edge, D::NorthWest, I::ResizingDiagonallyDown),
                    h(fill, edge, D::North, I::ResizingVertically),
                    h(edge, edge, D::NorthEast, I::ResizingDiagonallyUp),
                ],
                row![
                    h(edge, fill, D::West, I::ResizingHorizontally),
                    space().width(fill).height(fill),
                    h(edge, fill, D::East, I::ResizingHorizontally),
                ]
                .height(fill),
                row![
                    h(edge, edge, D::SouthWest, I::ResizingDiagonallyUp),
                    h(fill, edge, D::South, I::ResizingVertically),
                    h(edge, edge, D::SouthEast, I::ResizingDiagonallyDown),
                ],
            ]
            .width(fill)
            .height(fill)
        };
        let mut layout = stack![layout, handles].width(Length::Fill).height(Length::Fill);
        if self.menu_open {
            layout = layout.push(self.menu());
        }

        // The window itself is transparent; this container paints the rounded page.
        let radius = if self.maximized || self.fullscreen { 0.0 } else { CORNER_RADIUS };
        container(layout)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |theme: &Theme| container::Style {
                background: Some(Background::Color(theme.palette().background)),
                border: Border::default().rounded(radius),
                ..container::Style::default()
            })
            .into()
    }
}

/// A labelled preference row in the menu.
fn setting<'a>(label: &'static str, control: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    row![text(label).size(13.0).width(110.0), control.into()]
        .align_y(iced::Alignment::Center)
        .spacing(8)
        .padding(Padding::new(0.0).left(8.0).right(8.0))
        .into()
}

/// A drop-down for one of the enum preferences, sized to match the menu text.
fn pick<'a, T>(options: &'a [T], selected: T, on_select: impl Fn(T) -> Message + 'a) -> Element<'a, Message>
where
    T: ToString + PartialEq + Clone + 'a,
{
    pick_list(options, Some(selected), on_select).text_size(13.0).width(Length::Fill).into()
}

/// Keyboard shortcuts, shared by the editor key binding and the global listener.
fn shortcut(key: &Key, modifiers: Modifiers) -> Option<Message> {
    if let Key::Named(Named::F11) = key {
        return Some(Message::ToggleFullscreen);
    }
    if !modifiers.command() {
        return None;
    }
    let shift = modifiers.shift();
    match key.as_ref() {
        Key::Character(",") => Some(Message::ToggleMenu),
        Key::Character("s") if shift => Some(Message::SaveAs),
        Key::Character("s") => Some(Message::Save),
        Key::Character("S") => Some(Message::SaveAs),
        Key::Character("o") => Some(Message::Open),
        Key::Character("n") => Some(Message::New),
        Key::Character("d") if shift => Some(Message::ToggleDecorations),
        Key::Character("D") => Some(Message::ToggleDecorations),
        Key::Character("d") => Some(Message::CycleFocus),
        Key::Character("q") | Key::Character("w") => Some(Message::Quit),
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
        .theme(|app: &Inkpad| app.prefs.theme.theme())
        .style(|_app: &Inkpad, theme: &Theme| iced::theme::Style {
            background_color: Color::TRANSPARENT,
            text_color: theme.palette().text,
        })
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
        // Set first: the builder methods below extend these settings. The app id must
        // match the desktop file name so Wayland compositors can find the launcher icon.
        .window(window::Settings {
            icon: window::icon::from_file_data(include_bytes!("../assets/icon-256.png"), None).ok(),
            platform_specific: window::settings::PlatformSpecific {
                application_id: "inkpad".to_owned(),
                ..window::settings::PlatformSpecific::default()
            },
            ..window::Settings::default()
        })
        .window_size(WINDOW_SIZE)
        .decorations(false)
        .transparent(true)
        .antialiasing(true)
        .run()
}
