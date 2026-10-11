//! Inkpad: a quiet Markdown editor for Linux, built with iced.

mod dsp;
mod editor;
mod frost;
mod highlight;
mod history;
mod music;
mod palette;
mod prefs;
mod recent;
mod sound;
mod stats;

use std::path::PathBuf;
use std::sync::Arc;

use iced::advanced::text::editor::{Cursor, Edit, Motion};
use iced::advanced::text::{LineHeight, Wrapping};
use iced::keyboard::{self, Key, Modifiers};
use iced::keyboard::key::Named;
use iced::widget::{
    button, center, column, container, scrollable, mouse_area, opaque, pick_list, row, rule, slider, space, stack, text, toggler,
};
use iced::window;

use editor::widget::{self as text_editor, TextEditor};
use history::{History, position_at};
use iced::{Background, Border, Color, Element, Font, Length, Padding, Shadow, Subscription, Task, Theme, Vector};

use highlight::{FocusMode, MarkdownHighlighter, Settings as HlSettings, sentence_at};
use iced::advanced::text::editor::Position;
use palette::{ThemeKind, mix};
use music::{Music, MusicDirs};
use prefs::{FontKind, MAX_LINE_HEIGHT, MAX_SIZE, MAX_TRANSPARENCY, MIN_LINE_HEIGHT, MIN_SIZE, Preferences, SoundSet};
use stats::Stats;

const APP_NAME: &str = "Inkpad";
/// Incremented by build.rs on every build; see the About section of the menu.
const BUILD: &str = env!("INKPAD_BUILD");
const APP_ICON: &[u8] = include_bytes!("../assets/icon-256.png");
const REPOSITORY: &str = "https://github.com/50hoursaweek/inkpad";
/// The menu and dialogs use Lato Light; the editor uses Plex Mono, Plex Sans and Literata.
const UI_FONT: Font = Font {
    family: iced::font::Family::Name("Lato"),
    weight: iced::font::Weight::Light,
    stretch: iced::font::Stretch::Normal,
    style: iced::font::Style::Normal,
};
const DRAG_STRIP: f32 = 28.0;
// Text column layout. The measure is set in characters of the window's base size
// (not the zoomed size), so zooming reflows text inside a column whose margins
// stay put; effective width = min(window width - 2 * padding, MEASURE_CH ch), centred.
/// Maximum line length in characters of running text at the base size.
const MEASURE_CH: f32 = 66.0;
/// Minimum horizontal padding on each side of the text, in em of the body size.
const SIDE_PADDING_EM: f32 = 1.75;
/// Body size steps with window width: points added to the preferred size at or above each width.
/// With the default 18 pt this gives 18 px below 900, 20 px from 900 and 22 px from 1400.
const SIZE_STEPS: [(f32, f32); 2] = [(900.0, 2.0), (1400.0, 4.0)];
/// The size the column is measured against before the user's zoom is applied.
const BASE_SIZE: f32 = 18.0;
const EDGE: f32 = 6.0;
/// Corner radius of the undecorated window, matching typical desktop windows.
const CORNER_RADIUS: f32 = 12.0;
const WINDOW_SIZE: (f32, f32) = (1100.0, 780.0);

/// The dialogs reachable from the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dialog {
    Welcome,
    Credits,
    /// Asks what to do with unsaved changes before `Pending` goes ahead.
    Unsaved(Pending),
    Appearance,
    Sound,
    About,
}

/// An action that would lose unsaved changes and therefore waits for the user's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    Quit,
    New,
    Open,
    /// Open the file held in `Inkpad::pending_path`.
    OpenRecent,
    /// Replace the document with the already loaded file in `Inkpad::pending_open`.
    Replace,
}

struct Inkpad {
    content: text_editor::Content,
    /// Cached copy of the document, kept in sync after every edit.
    text: String,
    /// The text as last loaded or saved; the document is dirty when `text` differs from it.
    saved_text: Arc<String>,
    history: History,
    path: Option<PathBuf>,
    dirty: bool,
    /// Bumped whenever the document is replaced; results of older requests are ignored.
    generation: u64,
    /// Bumped on every edit; lets a "Don't save" answer authorise one specific replacement.
    edits: u64,
    /// Set by "Don't save" for an Open: the (generation, edits) state that may be replaced.
    replace_authorised: Option<(u64, u64)>,
    /// A file that finished loading while the document had new changes; waits for an answer.
    pending_open: Option<(PathBuf, Arc<String>)>,
    /// True while a write is in flight; a second Save waits for it.
    saving: bool,
    prefs: Preferences,
    sound: sound::Sound,
    music_dirs: MusicDirs,
    /// Menu options for the music picker: the built-ins, then the files found in `music_dir`.
    music_options: Vec<Music>,
    menu_open: bool,
    dialog: Option<Dialog>,
    /// Set while a save is in flight on behalf of a pending action; it runs once the save succeeds.
    after_save: Option<Pending>,
    /// The recent file waiting behind the unsaved-changes dialog.
    pending_path: Option<PathBuf>,
    recent: Vec<PathBuf>,
    /// Compositor effects for the window; `None` until attached or when not on Wayland.
    frost: Option<frost::Frost>,
    frost_attach_started: bool,
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
    /// A file finished loading (or failed) for the document generation it was requested in.
    Opened { generation: u64, result: Result<(PathBuf, Arc<String>), String> },
    OpenRecent(PathBuf),
    Save,
    SaveAs,
    /// A write finished; carries the generation and the exact text that was written.
    Saved { generation: u64, text: Arc<String>, result: Result<PathBuf, String> },
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
    OpenDialog(Dialog),
    /// Closes the menu and any dialog.
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
    SetShowWelcome(bool),
    OpenRepository,
    /// Carry out a pending action without saving.
    Discard(Pending),
    /// Save first, then carry out the pending action.
    SaveThen(Pending),
    FrostAttached(Option<frost::Frost>),
    /// Persist the preferences (sliders call this on release rather than on every tick).
    SavePrefs,
    OpenLicences,
    SetFrost(bool),
    SetFrostTransparency(f32),
    SetFrostThickness(f32),
    Quit,
}

impl Inkpad {
    fn new() -> (Self, Task<Message>) {
        let music_dirs = MusicDirs::locate();
        // Create the user's folder so it is easy to find and drop tracks into.
        let _ = std::fs::create_dir_all(&music_dirs.user);
        let app = Self {
            content: text_editor::Content::new(),
            text: String::new(),
            saved_text: Arc::new(String::new()),
            history: History::default(),
            path: None,
            dirty: false,
            generation: 0,
            edits: 0,
            replace_authorised: None,
            pending_open: None,
            saving: false,
            prefs: Preferences::load(),
            sound: sound::Sound::start(music_dirs.clone()),
            music_dirs,
            music_options: Vec::new(),
            menu_open: false,
            dialog: None,
            after_save: None,
            pending_path: None,
            recent: recent::load(),
            frost: None,
            frost_attach_started: false,
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
        if app.prefs.show_welcome {
            app.dialog = Some(Dialog::Welcome);
        }
        app.refresh_music_options();
        // Music is never started automatically; only its volume is remembered.
        app.sound.set_music_volume(app.prefs.music_volume);
        let mut tasks = vec![iced::widget::operation::focus_next()];
        // Options come before `--`; everything after it, and anything not starting with
        // `--`, is a path. Paths stay as OS strings: Linux file names need not be UTF-8.
        let mut options_done = false;
        let mut file: Option<PathBuf> = None;
        for arg in std::env::args_os().skip(1) {
            let option = (!options_done).then(|| arg.to_str().filter(|a| a.starts_with("--"))).flatten();
            match option {
                Some("--") => options_done = true,
                Some("--anchor") => app.prefs.typewriter = true,
                Some("--menu") => app.menu_open = true,
                Some("--dialog=appearance") => app.dialog = Some(Dialog::Appearance),
                Some("--dialog=sound") => app.dialog = Some(Dialog::Sound),
                Some("--dialog=about") => app.dialog = Some(Dialog::About),
                Some("--dialog=welcome") => app.dialog = Some(Dialog::Welcome),
                Some("--dialog=unsaved") => app.dialog = Some(Dialog::Unsaved(Pending::Quit)),
                Some("--soundtest") => tasks.push(Task::done(Message::SoundTest)),
                Some("--end") => tasks.push(Task::done(Message::CursorToEnd)),
                Some("--selftest") => tasks.push(Task::done(Message::SelfTest)),
                Some(other) => {
                    eprintln!("inkpad: unknown option {other}");
                    std::process::exit(2);
                }
                None => {
                    // Only the first file opens; a second one would race the first.
                    if file.is_none() {
                        file = Some(PathBuf::from(arg));
                    } else {
                        eprintln!("inkpad: ignoring extra path {}", PathBuf::from(arg).display());
                    }
                }
            }
        }
        if let Some(path) = file {
            tasks.push(app.read(path));
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
        // Actions chosen from the menu close it; the result should be visible, not hidden behind the card.
        if matches!(
            message,
            Message::New
                | Message::Open
                | Message::OpenRecent(_)
                | Message::Save
                | Message::SaveAs
                | Message::ToggleFullscreen
                | Message::ToggleDecorations
        ) {
            self.menu_open = false;
        }
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
                        self.edits += 1;
                        self.notice = None;
                    }
                    self.stats = stats::compute(&new_text);
                    self.text = new_text;
                    self.update_dirty();
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
                if self.dirty {
                    return self.ask_unsaved(Pending::New);
                }
                self.proceed(Pending::New)
            }
            Message::Open => {
                if self.dirty {
                    return self.ask_unsaved(Pending::Open);
                }
                self.proceed(Pending::Open)
            }
            Message::OpenRecent(path) => {
                if self.dirty {
                    self.pending_path = Some(path);
                    return self.ask_unsaved(Pending::OpenRecent);
                }
                self.read(path)
            }
            Message::Discard(pending) => {
                self.dialog = None;
                // The document is not marked clean here: if the replacement never happens
                // (cancelled chooser, unreadable file) its unsaved changes stay protected.
                // Instead this exact state is authorised to be replaced once.
                self.replace_authorised = Some((self.generation, self.edits));
                self.proceed(pending)
            }
            Message::SaveThen(pending) => {
                self.dialog = None;
                self.after_save = Some(pending);
                self.update(Message::Save)
            }
            Message::Opened { generation, result } => {
                if generation != self.generation {
                    // The document was replaced while this file was loading; drop the result.
                    return Task::none();
                }
                let (path, text) = match result {
                    Ok(loaded) => loaded,
                    Err(e) => {
                        if !e.is_empty() {
                            self.notice = Some(e);
                        }
                        return Task::none();
                    }
                };
                let authorised = self.replace_authorised.take() == Some((self.generation, self.edits));
                if self.dirty && !authorised {
                    // Edited since the request was made: ask again rather than replace silently.
                    self.pending_open = Some((path, text));
                    return self.ask_unsaved(Pending::Replace);
                }
                self.replace_with(path, &text);
                Task::none()
            }
            Message::Save => {
                if self.saving {
                    self.notice = Some("Still saving…".to_owned());
                    return Task::none();
                }
                match self.path.clone() {
                    Some(path) => self.write(path),
                    None => self.update(Message::SaveAs),
                }
            }
            Message::SaveAs => {
                if self.saving {
                    self.notice = Some("Still saving…".to_owned());
                    return Task::none();
                }
                self.saving = true;
                let generation = self.generation;
                let text = Arc::new(self.content.text());
                let written = text.clone();
                Task::perform(save_dialog(text), move |result| Message::Saved { generation, text: written.clone(), result })
            }
            Message::Saved { generation, text, result } => {
                self.saving = false;
                if generation != self.generation {
                    // Belongs to a document that has since been replaced: ignore it.
                    self.after_save = None;
                    return Task::none();
                }
                match result {
                    Ok(path) => {
                        recent::push(&mut self.recent, &path);
                        self.path = Some(path);
                        self.saved_text = text;
                        self.history.end_group();
                        self.update_dirty();
                        self.notice = Some(if self.dirty { "Saved (edited since)".to_owned() } else { "Saved".to_owned() });
                        match self.after_save.take() {
                            // Typed while the chooser was open: the new text is not saved, ask again.
                            Some(pending) if self.dirty => self.ask_unsaved(pending),
                            Some(pending) => self.proceed(pending),
                            None => Task::none(),
                        }
                    }
                    Err(e) => {
                        // A cancelled Save As also lands here: the pending action is dropped.
                        self.after_save = None;
                        self.notice = Some(format!("Save failed: {e}"));
                        Task::none()
                    }
                }
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
                Task::none()
            }
            Message::SetTheme(theme) => {
                self.prefs.theme = theme;
                self.apply_frost();
                self.save_prefs()
            }
            Message::SetFont(font) => {
                self.prefs.font = font;
                self.refresh_focus();
                self.save_prefs()
            }
            Message::SetSize(size) => {
                self.prefs.size = size.round().clamp(MIN_SIZE, MAX_SIZE);
                Task::none()
            }
            Message::SavePrefs => self.save_prefs(),
            Message::SetLineHeight(lh) => {
                self.prefs.line_height = ((lh * 20.0).round() / 20.0).clamp(MIN_LINE_HEIGHT, MAX_LINE_HEIGHT);
                Task::none()
            }
            Message::ToggleMenu => {
                self.dialog = None;
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
            Message::OpenRepository => {
                // Hand the URL to the desktop's default browser.
                let _ = std::process::Command::new("xdg-open").arg(REPOSITORY).spawn();
                Task::none()
            }
            Message::SetShowWelcome(show) => {
                self.prefs.show_welcome = show;
                self.save_prefs()
            }
            Message::SetMusicVolume(v) => {
                self.prefs.music_volume = ((v * 20.0).round() / 20.0).clamp(0.0, 1.0);
                self.sound.set_music_volume(self.prefs.music_volume);
                Task::none()
            }
            Message::OpenDialog(dialog) => {
                self.menu_open = false;
                self.dialog = Some(dialog);
                if dialog == Dialog::Sound {
                    self.refresh_music_options();
                }
                Task::none()
            }
            Message::CloseMenu => {
                self.menu_open = false;
                self.dialog = None;
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
                let maximized = window::latest().and_then(window::is_maximized).map(Message::Maximized);
                if self.frost_attach_started || !self.prefs.frost {
                    return maximized;
                }
                // Frosted glass is on and the window exists: bind the compositor effects, once.
                Task::batch([maximized, self.attach_frost()])
            }
            Message::FrostAttached(frost) => {
                self.frost = frost;
                self.apply_frost();
                Task::none()
            }
            Message::SetFrost(on) => {
                self.prefs.frost = on;
                self.apply_frost();
                let save = self.save_prefs();
                if on && !self.frost_attach_started {
                    // First use: only now does the compositor get asked for the effects.
                    return Task::batch([save, self.attach_frost()]);
                }
                save
            }
            Message::OpenLicences => {
                if let Some(dir) = licences_dir() {
                    let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
                }
                Task::none()
            }
            Message::SetFrostTransparency(v) => {
                self.prefs.frost_transparency = ((v * 20.0).round() / 20.0).clamp(0.0, MAX_TRANSPARENCY);
                self.apply_frost();
                Task::none()
            }
            Message::SetFrostThickness(v) => {
                self.prefs.frost_thickness = ((v * 20.0).round() / 20.0).clamp(0.0, 1.0);
                self.apply_frost();
                Task::none()
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
            Message::Quit => {
                if self.dirty {
                    return self.ask_unsaved(Pending::Quit);
                }
                iced::exit()
            }
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
                    std::process::exit(1)
                }
            },
            Message::KeyPressed(key, modifiers) => {
                if (self.menu_open || self.dialog.is_some()) && matches!(key, Key::Named(Named::Escape)) {
                    return self.update(Message::CloseMenu);
                }
                match shortcut(&key, modifiers) {
                    Some(m) => self.update(m),
                    None => Task::none(),
                }
            }
        }
    }

    /// Show the unsaved-changes dialog for `pending`, closing the menu.
    fn ask_unsaved(&mut self, pending: Pending) -> Task<Message> {
        self.menu_open = false;
        self.dialog = Some(Dialog::Unsaved(pending));
        Task::none()
    }

    /// Carry out an action that was waiting on the unsaved-changes dialog (or needed none).
    fn proceed(&mut self, pending: Pending) -> Task<Message> {
        match pending {
            Pending::Quit => {
                // Release compositor objects and the settings before the window goes away.
                if let Some(frost) = self.frost.take() {
                    frost.release();
                }
                let _ = self.prefs.save();
                iced::exit()
            }
            Pending::New => {
                self.load(String::new());
                self.path = None;
                Task::none()
            }
            Pending::Open => {
                let generation = self.generation;
                Task::perform(open_dialog(), move |result| Message::Opened { generation, result })
            }
            Pending::OpenRecent => match self.pending_path.take() {
                Some(path) => self.read(path),
                None => Task::none(),
            },
            Pending::Replace => match self.pending_open.take() {
                Some((path, text)) => {
                    self.replace_with(path, &text);
                    Task::none()
                }
                None => Task::none(),
            },
        }
    }

    /// Start reading `path` for the current document generation.
    fn read(&self, path: PathBuf) -> Task<Message> {
        let generation = self.generation;
        Task::perform(read_file(path), move |result| Message::Opened { generation, result })
    }

    /// Start writing the current text to `path`.
    fn write(&mut self, path: PathBuf) -> Task<Message> {
        self.saving = true;
        let generation = self.generation;
        let text = Arc::new(self.content.text());
        let written = text.clone();
        Task::perform(write_file(path, text), move |result| Message::Saved { generation, text: written.clone(), result })
    }

    /// Replace the document with a loaded file.
    fn replace_with(&mut self, path: PathBuf, text: &str) {
        self.load(text.replace("\r\n", "\n"));
        recent::push(&mut self.recent, &path);
        self.path = Some(path);
    }

    /// Dirty means the text differs from what was last loaded or saved, so undoing back
    /// to the saved revision makes the document clean again.
    fn update_dirty(&mut self) {
        self.dirty = self.text != *self.saved_text;
    }

    /// Bind the compositor effects to the window (a short synchronous roundtrip).
    fn attach_frost(&mut self) -> Task<Message> {
        self.frost_attach_started = true;
        window::latest().and_then(|id| window::run(id, |w| frost::Frost::attach(w))).map(Message::FrostAttached)
    }

    /// Push the current frost settings to the compositor, if attached.
    fn apply_frost(&self) {
        let Some(frost) = &self.frost else { return };
        let bg = self.prefs.theme.theme().palette().background;
        frost.apply(frost::Settings {
            enabled: self.prefs.frost,
            transparency: self.prefs.frost_transparency,
            thickness: self.prefs.frost_thickness,
            tint: [(bg.r * 255.0) as u8, (bg.g * 255.0) as u8, (bg.b * 255.0) as u8],
        });
    }

    /// Rescan the music folder for the picker.
    fn refresh_music_options(&mut self) {
        // Off, then the bundled tracks, then the generated soundscapes, then the user's own files.
        let (bundled, own): (Vec<Music>, Vec<Music>) =
            self.music_dirs.choices().into_iter().partition(|m| matches!(m, Music::Bundled(_)));
        self.music_options = vec![Music::Off];
        self.music_options.extend(bundled);
        self.music_options.extend(Music::BUILT_IN.iter().filter(|m| **m != Music::Off).cloned());
        self.music_options.extend(own);
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
        self.saved_text = Arc::new(text.clone());
        self.text = text;
        self.history.clear();
        self.dirty = false;
        self.generation += 1;
        self.edits = 0;
        self.replace_authorised = None;
        self.pending_open = None;
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
        self.edits += 1;
        self.update_dirty();
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

        // Quitting, New and Open with unsaved changes must ask instead of acting.
        for (msg, pending) in [(Message::Quit, Pending::Quit), (Message::New, Pending::New), (Message::Open, Pending::Open)] {
            self.dialog = None;
            let _ = self.update(msg);
            if self.dialog != Some(Dialog::Unsaved(pending)) {
                return Err(format!("{pending:?} with unsaved changes did not ask"));
            }
            if self.content.text().is_empty() {
                return Err("document was lost".to_owned());
            }
        }
        self.dialog = None;

        // "Don't save" then a cancelled chooser must leave the changes protected.
        let _ = self.update(Message::Discard(Pending::Open));
        let _ = self.update(Message::Opened { generation: self.generation, result: Err(String::new()) });
        if !self.dirty {
            return Err("cancelled Open after Discard marked the document clean".to_owned());
        }
        // A stale save result (older generation) must be ignored.
        let stale = Message::Saved { generation: self.generation + 100, text: Arc::new(String::new()), result: Ok(PathBuf::from("/nonexistent")) };
        let _ = self.update(stale);
        if !self.dirty || self.path.is_some() {
            return Err("stale save result was applied".to_owned());
        }
        // A save of older text must not mark newer text clean.
        let older = Arc::new("older".to_owned());
        let _ = self.update(Message::Saved { generation: self.generation, text: older, result: Ok(PathBuf::from("/nonexistent/x.md")) });
        if !self.dirty {
            return Err("save of older text marked newer text clean".to_owned());
        }
        // A save of the current text makes it clean; undo past it makes it dirty again;
        // redo back to it makes it clean.
        let current = Arc::new(self.content.text());
        let _ = self.update(Message::Saved { generation: self.generation, text: current, result: Ok(PathBuf::from("/nonexistent/x.md")) });
        if self.dirty {
            return Err("save of current text left the document dirty".to_owned());
        }
        step(self, Message::Undo, "Hello X.\nSecond line. More\nNew", "undo past save")?;
        if !self.dirty {
            return Err("undo past the saved revision is not dirty".to_owned());
        }
        step(self, Message::Redo, "Hello X.\nSecond line. More\nNewè!", "redo to saved")?;
        if self.dirty {
            return Err("redo back to the saved revision is still dirty".to_owned());
        }
        // A file that finished loading while the document is dirty must ask, not replace.
        let _ = self.update(edit(Edit::Insert('!')));
        let loaded = Message::Opened { generation: self.generation, result: Ok((PathBuf::from("/nonexistent/y.md"), Arc::new("other".to_owned()))) };
        let _ = self.update(loaded);
        if self.dialog != Some(Dialog::Unsaved(Pending::Replace)) || self.content.text() == "other" {
            return Err("late Open result replaced an edited document".to_owned());
        }
        self.dialog = None;
        self.pending_open = None;
        self.path = None;
        Ok(())
    }

    /// Points added to sizes for the current window width, so text fills more of a large screen.
    fn size_step(&self) -> f32 {
        SIZE_STEPS
            .iter()
            .filter(|(min_width, _)| self.window_width >= *min_width)
            .map(|(_, step)| *step)
            .last()
            .unwrap_or(0.0)
    }

    /// The size the column width and padding are derived from: depends on the window only.
    fn base_size(&self) -> f32 {
        BASE_SIZE + self.size_step()
    }

    /// The size text is drawn at: the preferred size plus the window step.
    fn body_size(&self) -> f32 {
        (self.prefs.size + self.size_step()).min(MAX_SIZE)
    }

    /// Recompute the paragraph and sentence under the cursor for the spotlight.
    fn refresh_focus(&mut self) {
        let cursor = self.content.cursor();
        let line = cursor.position.line;
        let col = cursor.position.column;

        if self.prefs.focus == FocusMode::Off {
            // Nothing is dimmed: skip the per-line scan.
            self.highlight = HlSettings {
                family: self.prefs.font.family(),
                focus: FocusMode::Off,
                paragraph: 0..0,
                cursor_line: line,
                sentence: 0..0,
            };
            return;
        }

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

    /// The menu: a drop-down under the ☰ button with file actions, entries that open
    /// the settings dialogs, the writing and window sections, and About.
    fn menu(&self) -> Element<'_, Message> {
        let mut recent_items = column![].spacing(4);
        if !self.recent.is_empty() {
            recent_items = recent_items.push(divider()).push(heading("RECENT"));
            for path in &self.recent {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                recent_items =
                    recent_items.push(action(name, recent::folder_label(path), Message::OpenRecent(path.clone())));
            }
        }
        let card = column![
            heading("FILE"),
            action("New", "Ctrl+N", Message::New),
            action("Open…", "Ctrl+O", Message::Open),
            action("Save", "Ctrl+S", Message::Save),
            action("Save As…", "Ctrl+Shift+S", Message::SaveAs),
            recent_items,
            divider(),
            heading("SETTINGS"),
            action("Appearance…", "", Message::OpenDialog(Dialog::Appearance)),
            action("Sound and music…", "", Message::OpenDialog(Dialog::Sound)),
            divider(),
            heading("WRITING"),
            setting("Spotlight", pick(&FocusMode::ALL, self.prefs.focus, Message::SetFocus)),
            setting(
                "Anchor",
                toggler(self.prefs.typewriter).on_toggle(Message::SetTypewriter).size(18.0),
            ),
            divider(),
            heading("WINDOW"),
            action("Fullscreen", "F11", Message::ToggleFullscreen),
            action("Toggle decorations", "Ctrl+Shift+D", Message::ToggleDecorations),
            action("Quit", "Ctrl+Q", Message::Quit),
            divider(),
            action("About Inkpad…", "", Message::OpenDialog(Dialog::About)),
        ]
        .spacing(4)
        .width(300.0);

        // Scroll when the card is taller than the window, so nothing is clipped.
        let card = container(scrollable(container(card).padding(Padding::new(0.0).right(8.0))))
            .max_height((self.window_height - DRAG_STRIP - 24.0).max(200.0))
            .padding(Padding::new(16.0).right(8.0))
            .style(card_style);

        // The card hangs below the ☰ button; the backdrop swallows clicks so they
        // close the menu instead of reaching the editor.
        let anchored = container(opaque(card))
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(Padding::new(0.0).top(DRAG_STRIP + 2.0).left(8.0));
        opaque(mouse_area(anchored).on_press(Message::CloseMenu))
    }

    /// A settings or about dialog, centred over the editor.
    fn dialog(&self, dialog: Dialog) -> Element<'_, Message> {
        let size = self.prefs.size;
        let line_height = self.prefs.line_height;
        let volume = self.prefs.sound_volume;
        let music_volume = self.prefs.music_volume;
        let transparency = self.prefs.frost_transparency;
        let thickness = self.prefs.frost_thickness;

        let paragraph = |s: &'static str| text(s).size(14.0).font(UI_FONT).width(Length::Fill);
        let (title, body): (&str, Element<'_, Message>) = match dialog {
            Dialog::Welcome => (
                "Welcome to Inkpad",
                column![
                    paragraph("A quiet place to write Markdown. The page is the whole window: there is no toolbar, and the window has no title bar."),
                    paragraph("The ☰ button in the top-left corner (or Ctrl+,) opens the menu, with file actions, Appearance, and Sound and music."),
                    paragraph("Sound and music is where you pick a typing sound and a background: bundled music loops, generated soundscapes (rain, a night of crickets, a train, a breeze, a drone), or your own tracks."),
                    muted(format!("Your own tracks (mp3, ogg, flac, wav) go in\n{}", self.music_dirs.user.display())),
                    paragraph("Handy keys: Ctrl+D cycles the spotlight (dims everything but the paragraph or sentence you are in), Ctrl+T anchors the caret line mid-screen, Ctrl+S saves, F11 goes fullscreen."),
                    iced::widget::checkbox(!self.prefs.show_welcome)
                        .label("Don't show this again")
                        .on_toggle(|checked| Message::SetShowWelcome(!checked))
                        .size(16.0)
                        .text_size(13.0)
                        .font(UI_FONT),
                ]
                .spacing(10)
                .into(),
            ),
            Dialog::Appearance => (
                "Appearance",
                column![
                    setting("Theme", pick(&ThemeKind::ALL, self.prefs.theme, Message::SetTheme)),
                    setting("Font", pick(&FontKind::ALL, self.prefs.font, Message::SetFont)),
                    setting(
                        "Size",
                        slider_row(
                            slider(MIN_SIZE..=MAX_SIZE, size, Message::SetSize).step(1.0_f32).on_release(Message::SavePrefs),
                            format!("{size:.0} pt"),
                        ),
                    ),
                    setting(
                        "Line height",
                        slider_row(
                            slider(MIN_LINE_HEIGHT..=MAX_LINE_HEIGHT, line_height, Message::SetLineHeight)
                                .step(0.05_f32)
                                .on_release(Message::SavePrefs),
                            format!("{line_height:.2}"),
                        ),
                    ),
                    divider(),
                    heading("FROSTED GLASS"),
                    setting("Frosted glass", toggler(self.prefs.frost).on_toggle(Message::SetFrost).size(18.0)),
                    setting(
                        "Transparency",
                        slider_row(
                            slider(0.0..=MAX_TRANSPARENCY, transparency, Message::SetFrostTransparency).step(0.05_f32).on_release(Message::SavePrefs),
                            format!("{:.0}%", transparency * 100.0),
                        ),
                    ),
                    setting(
                        "Thickness",
                        slider_row(
                            slider(0.0..=1.0, thickness, Message::SetFrostThickness).step(0.05_f32).on_release(Message::SavePrefs),
                            format!("{:.0}%", thickness * 100.0),
                        ),
                    ),
                    muted(match self.frost.as_ref().map(frost::Frost::blur_supported) {
                        Some(true) => "The desktop behind the window is blurred by the compositor.".to_owned(),
                        Some(false) => "This desktop cannot blur behind windows (KDE Plasma can), so only the transparency applies.".to_owned(),
                        None if self.prefs.frost => "Checking what this desktop supports…".to_owned(),
                        None => "Blur behind the window depends on the desktop: KDE Plasma can, COSMIC and GNOME apply the transparency only.".to_owned(),
                    }),
                ]
                .spacing(8)
                .into(),
            ),
            Dialog::Sound => (
                "Sound and music",
                column![
                    heading("TYPING SOUND"),
                    setting(
                        "Enabled",
                        toggler(self.prefs.typing_sound).on_toggle(Message::SetTypingSound).size(18.0),
                    ),
                    setting("Sound", pick(&SoundSet::ALL, self.prefs.sound_set, Message::SetSoundSet)),
                    setting(
                        "Volume",
                        slider_row(
                            slider(0.0..=1.0, volume, Message::SetSoundVolume).step(0.05_f32).on_release(Message::SavePrefs),
                            format!("{:.0}%", volume * 100.0),
                        ),
                    ),
                    divider(),
                    heading("MUSIC"),
                    setting("Music", pick(&self.music_options, self.prefs.music.clone(), Message::SetMusic)),
                    setting(
                        "Volume",
                        slider_row(
                            slider(0.0..=1.0, music_volume, Message::SetMusicVolume).step(0.05_f32).on_release(Message::SavePrefs),
                            format!("{:.0}%", music_volume * 100.0),
                        ),
                    ),
                    muted(format!("Add your own tracks (mp3, ogg, flac, wav) in\n{}", self.music_dirs.user.display())),
                ]
                .spacing(8)
                .into(),
            ),
            Dialog::Unsaved(_) => (
                "Unsaved changes",
                column![
                    text(match self.path.as_ref().and_then(|p| p.file_name()) {
                        Some(name) => format!("Save the changes to {}?", name.to_string_lossy()),
                        None => "This document has not been saved yet. Save it?".to_owned(),
                    })
                    .size(14.0)
                    .font(UI_FONT)
                    .width(Length::Fill)
                ]
                .into(),
            ),
            Dialog::Credits => (
                "Credits and licences",
                column![
                    paragraph("Inkpad is released under the MIT License."),
                    paragraph("Fonts: IBM Plex Mono and IBM Plex Sans (IBM), Literata (TypeTogether for Google Fonts) and Lato (Łukasz Dziedzic), all under the SIL Open Font License 1.1."),
                    paragraph("Typing sounds: cut from Keyboard Soundpack #1 by unicaegames (OpenGameArt), CC0 1.0."),
                    paragraph("Music loops: generated by the author with Google Lyria 3 and released under CC0 1.0. Soundscapes are synthesised in code."),
                    paragraph("Built with iced (MIT); the text editor widget is iced's, extended. Full licence texts ship with the app."),
                ]
                .spacing(10)
                .into(),
            ),
            Dialog::About => (
                "",
                column![
                    iced::widget::image(iced::widget::image::Handle::from_bytes(APP_ICON)).width(96.0).height(96.0),
                    text(APP_NAME).size(20.0).font(UI_FONT),
                    muted(format!("Version {} · build {BUILD}", env!("CARGO_PKG_VERSION"))),
                    muted("© 2026 Grunt".to_owned()),
                    link_button(REPOSITORY.trim_start_matches("https://"), Message::OpenRepository),
                    row![
                        link_button("Credits and licences", Message::OpenDialog(Dialog::Credits)),
                        link_button("Open licence folder", Message::OpenLicences),
                    ]
                    .spacing(4),
                ]
                .spacing(6)
                .align_x(iced::Alignment::Center)
                .width(Length::Fill)
                .into(),
            ),
        };

        let done = dialog_button("Done", Message::CloseMenu);
        let width = match dialog {
            Dialog::About => 300.0,
            Dialog::Unsaved(_) => 360.0,
            Dialog::Welcome | Dialog::Credits => 440.0,
            _ => 380.0,
        };
        let mut card = column![].spacing(14).width(width);
        if !title.is_empty() {
            card = card.push(text(title).size(17.0).font(UI_FONT));
        }
        let buttons: Element<'_, Message> = if let Dialog::Unsaved(pending) = dialog {
            row![
                dialog_button("Don't save", Message::Discard(pending)),
                space().width(Length::Fill),
                dialog_button("Cancel", Message::CloseMenu),
                dialog_button("Save", Message::SaveThen(pending)),
            ]
            .spacing(8)
            .width(Length::Fill)
            .into()
        } else if dialog == Dialog::Welcome {
            row![
                space().width(Length::Fill),
                dialog_button("Sound and music…", Message::OpenDialog(Dialog::Sound)),
                dialog_button("Start writing", Message::CloseMenu),
            ]
            .spacing(8)
            .width(Length::Fill)
            .into()
        } else {
            row![space().width(Length::Fill), done].width(Length::Fill).into()
        };
        let card = card.push(body).push(buttons);
        let card = container(card).padding(20).style(card_style);
        // Clicking the backdrop closes the dialog, like the menu.
        opaque(mouse_area(center(opaque(card))).on_press(Message::CloseMenu))
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
        // The window's close button goes through the unsaved-changes guard too.
        let closes = window::close_requests().map(|_| Message::Quit);
        Subscription::batch([keys, resizes, closes])
    }

    fn view(&self) -> Element<'_, Message> {
        let font = self.prefs.font.font();
        let menu_open = self.menu_open || self.dialog.is_some();
        let size = self.body_size();
        let base = self.base_size();
        let side_padding = base * SIDE_PADDING_EM;
        let measure = base * self.prefs.font.char_em() * MEASURE_CH;

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
                    // The page container paints the (possibly translucent) background.
                    background: Background::Color(Color::TRANSPARENT),
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
                format!("Spotlight: {}", self.prefs.focus.label()),
                self.prefs.focus != FocusMode::Off,
                Message::CycleFocus,
            ),
            item(
                format!("Anchor: {}", if self.prefs.typewriter { "On" } else { "Off" }),
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
        if let Some(dialog) = self.dialog {
            layout = layout.push(self.dialog(dialog));
        }

        // The window itself is transparent; this container paints the rounded page,
        // translucent when the frosted glass is on.
        let radius = if self.maximized || self.fullscreen { 0.0 } else { CORNER_RADIUS };
        let page_alpha = if self.prefs.frost { 1.0 - self.prefs.frost_transparency } else { 1.0 };
        container(layout)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |theme: &Theme| container::Style {
                background: Some(Background::Color(Color { a: page_alpha, ..theme.palette().background })),
                border: Border::default().rounded(radius),
                ..container::Style::default()
            })
            .into()
    }
}

/// Where the licence texts are installed: `../share/licenses/inkpad` next to the
/// executable (AppImage and Flatpak layouts), else the source tree.
fn licences_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let candidates = [
        exe.parent()?.join("../share/licenses/inkpad"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")),
    ];
    candidates.into_iter().find(|p| p.is_dir())
}

/// Dimmed secondary text.
fn muted<'a>(s: String) -> iced::widget::Text<'a> {
    text(s).size(12.5).font(UI_FONT).style(|theme: &Theme| {
        let p = theme.palette();
        text::Style { color: Some(mix(p.text, p.background, 0.45)) }
    })
}

/// Small caps-style section heading.
fn heading<'a>(s: &'static str) -> iced::widget::Text<'a> {
    text(s).size(11.5).font(UI_FONT).style(|theme: &Theme| {
        let p = theme.palette();
        text::Style { color: Some(mix(p.text, p.background, 0.55)) }
    })
}

/// A full-width flat menu entry with an optional shortcut hint on the right.
fn action<'a>(label: impl Into<String>, hint: impl Into<String>, msg: Message) -> Element<'a, Message> {
    button(
        row![text(label.into()).size(14.0).font(UI_FONT), space().width(Length::Fill), muted(hint.into())]
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
    .into()
}

fn divider<'a>() -> Element<'a, Message> {
    rule::horizontal(1)
        .style(|theme: &Theme| {
            let p = theme.palette();
            rule::Style {
                color: mix(p.text, p.background, 0.85),
                radius: 0.0.into(),
                fill_mode: rule::FillMode::Full,
                snap: false,
            }
        })
        .into()
}

/// The raised card used by the menu and the dialogs.
fn card_style(theme: &Theme) -> container::Style {
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
}

/// A slider with its current value shown on the right.
fn slider_row<'a>(control: iced::widget::Slider<'a, f32, Message>, label: String) -> Element<'a, Message> {
    row![control, muted(label).width(44.0)].spacing(8).align_y(iced::Alignment::Center).into()
}

/// Text that behaves like a hyperlink.
fn link_button<'a>(label: &'static str, msg: Message) -> Element<'a, Message> {
    button(text(label).size(13.0).font(UI_FONT))
        .padding(Padding::new(2.0).left(4.0).right(4.0))
        .on_press(msg)
        .style(|theme: &Theme, status| {
            let p = theme.palette();
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: None,
                text_color: if hovered { p.primary } else { mix(p.primary, p.text, 0.3) },
                border: Border::default(),
                ..button::Style::default()
            }
        })
        .into()
}

/// A raised button for dialogs.
fn dialog_button<'a>(label: &'static str, msg: Message) -> Element<'a, Message> {
    button(text(label).size(14.0).font(UI_FONT))
        .padding(Padding::new(5.0).left(14.0).right(14.0))
        .on_press(msg)
        .style(|theme: &Theme, status| {
            let p = theme.palette();
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(Background::Color(mix(p.text, p.background, if hovered { 0.8 } else { 0.88 }))),
                text_color: p.text,
                border: Border::default().rounded(6.0),
                ..button::Style::default()
            }
        })
        .into()
}

/// A labelled preference row in the menu.
fn setting<'a>(label: &'static str, control: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    row![text(label).size(14.0).font(UI_FONT).width(110.0), control.into()]
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
    pick_list(options, Some(selected), on_select).text_size(14.0).font(UI_FONT).width(Length::Fill).into()
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

/// Documents larger than this are refused rather than loaded into the editor.
const MAX_DOCUMENT_BYTES: u64 = 20 * 1024 * 1024;

/// An empty error means the chooser was cancelled; nothing to report.
async fn open_dialog() -> Result<(PathBuf, Arc<String>), String> {
    let handle = rfd::AsyncFileDialog::new()
        .add_filter("Markdown", &["md", "markdown", "txt"])
        .pick_file()
        .await
        .ok_or_else(String::new)?;
    read_file(handle.path().to_path_buf()).await
}

async fn read_file(path: PathBuf) -> Result<(PathBuf, Arc<String>), String> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let meta = std::fs::metadata(&path).map_err(|e| format!("Cannot open {name}: {e}"))?;
    if !meta.is_file() {
        return Err(format!("Cannot open {name}: not a regular file"));
    }
    if meta.len() > MAX_DOCUMENT_BYTES {
        return Err(format!("Cannot open {name}: larger than {} MB", MAX_DOCUMENT_BYTES / 1024 / 1024));
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("Cannot open {name}: {e}"))?;
    let text = String::from_utf8(bytes).map_err(|_| format!("Cannot open {name}: not UTF-8 text"))?;
    Ok((path, Arc::new(text)))
}

async fn save_dialog(contents: Arc<String>) -> Result<PathBuf, String> {
    let handle = rfd::AsyncFileDialog::new()
        .add_filter("Markdown", &["md", "markdown", "txt"])
        .set_file_name("Untitled.md")
        .save_file()
        .await
        .ok_or_else(|| "cancelled".to_owned())?;
    write_file(handle.path().to_path_buf(), contents).await
}

/// Write atomically: the content goes to a temporary sibling that replaces the
/// target only once fully written, so a full disk or a crash leaves the old file
/// intact. Where a sibling cannot be created (some portal paths) it falls back to
/// a direct write.
async fn write_file(path: PathBuf, contents: Arc<String>) -> Result<PathBuf, String> {
    use std::io::Write;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let temp = path.with_file_name(format!(".{name}.inkpad-{}.tmp", std::process::id()));
    let staged = (|| -> std::io::Result<()> {
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
        if let Ok(meta) = std::fs::metadata(&path) {
            let _ = std::fs::set_permissions(&temp, meta.permissions());
        }
        std::fs::rename(&temp, &path)
    })();
    match staged {
        Ok(()) => Ok(path),
        Err(stage_err) => {
            let _ = std::fs::remove_file(&temp);
            // Could not stage next to the target: write in place, as a last resort.
            std::fs::write(&path, contents.as_bytes()).map_err(|e| format!("{e} (atomic write failed: {stage_err})"))?;
            Ok(path)
        }
    }
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
        .default_font(FontKind::Mono.font())
        .font(include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/IBMPlexMono-Bold.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/IBMPlexMono-Italic.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/IBMPlexMono-BoldItalic.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/IBMPlexSans-Regular.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/IBMPlexSans-Bold.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/IBMPlexSans-Italic.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/IBMPlexSans-BoldItalic.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/Literata-Regular.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/Literata-Bold.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/Literata-Italic.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/Literata-BoldItalic.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/Lato-Light.ttf").as_slice())
        // Set first: the builder methods below extend these settings. The app id must
        // match the desktop file name so Wayland compositors can find the launcher icon.
        .window(window::Settings {
            icon: window::icon::from_file_data(include_bytes!("../assets/icon-256.png"), None).ok(),
            platform_specific: window::settings::PlatformSpecific {
                application_id: "io.github._50hoursaweek.inkpad".to_owned(),
                ..window::settings::PlatformSpecific::default()
            },
            ..window::Settings::default()
        })
        .window_size(WINDOW_SIZE)
        .decorations(false)
        .transparent(true)
        .exit_on_close_request(false)
        .antialiasing(true)
        .run()
}
