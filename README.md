# Inkpad

A quiet Markdown editor for Linux, written in Rust with [iced](https://iced.rs) 0.14.
Inspired by the best distraction-free writing apps, with its own identity:
the page is the whole window, a spotlight dims everything but what you are writing,
the caret line can be anchored mid-screen, three fonts (mono, sans, serif) suit different
moods, and typing sounds and soundscapes keep you company.

## Build and run

Requires a Rust toolchain (`rustup`), a C linker, and a Wayland or X11 session with Vulkan.

```sh
cargo run -- examples/sample.md
```

To build the AppImage, put `appimagetool` on `PATH` (or point `APPIMAGETOOL` at it)
and run `packaging/build-appimage.sh`; the result lands in `dist/`.

To build and install the Flatpak (app id `io.github._50hoursaweek.inkpad`, as submitted
to Flathub), with `org.flatpak.Builder` and the Freedesktop 26.08 SDK installed:

```sh
flatpak run org.flatpak.Builder --user --install --force-clean build-dir packaging/io.github._50hoursaweek.inkpad.yml
```

`packaging/cargo-sources.json` lists every crate for the offline build; regenerate it
with flatpak-builder-tools' `flatpak-cargo-generator.py Cargo.lock` after changing
dependencies.

## Shortcuts

| Keys | Action |
|---|---|
| Ctrl+, | Open the menu; Esc or a click outside closes it or any dialog |
| Ctrl+O / Ctrl+S / Ctrl+Shift+S / Ctrl+N | Open, save, save as, new |
| Ctrl+D | Cycle the spotlight: off, paragraph, sentence |
| Ctrl+T | Toggle the anchor (the caret line stays vertically centred) |
| Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y | Undo, redo |
| Ctrl+Shift+T | Cycle theme: light, sepia, dark |
| Ctrl+Shift+F | Cycle font: Mono, Sans, Serif |
| Ctrl+= / Ctrl+- | Zoom |
| F11 | Toggle fullscreen |
| Ctrl+Shift+D | Toggle window decorations (off by default) |
| Ctrl+Q / Ctrl+W | Quit (asks what to do with unsaved changes, as do New, Open and the window's close button) |

The window has no title bar. Drag the top strip or the status bar to move it,
double-click the top strip to maximise, and use your compositor's resize
gesture (Meta + right-drag on KDE) or Ctrl+Shift+D to bring decorations back.
A welcome dialog explains the menu and the sound settings at launch until its
"Don't show this again" box is ticked.
The ☰ button in the top-left corner opens the menu: file actions, then Appearance and
Sound and music, which open dialogs, then the Writing and Window sections, then About.

Preferences (theme, font, size, line height, spotlight, anchor) are
saved to `~/.config/inkpad/preferences` whenever they change.

A key sound plays on every edit; the Sound and music dialog has a switch, a choice of two
sound sets (soft and keyboard, both cut from a CC0 recording pack, with slight pitch and
level variation per press; Enter and Backspace sound different) and a volume slider.
Sound needs `libasound` (ALSA) at run time, which PipeWire and PulseAudio desktops provide;
without it the editor is simply silent.

Background music lives in the Sound and music dialog: six bundled loops (Nylon strings,
Late cafe, Wordless, Low strings, Lofi desk, Slow horizon; see Credits), five generated
soundscapes that never repeat (a Focus drone on a low chord that breathes and drifts,
Rainfall with an occasional gong, Cricket night, Night train, Open window), plus any tracks
you put in `~/.local/share/inkpad/music` (mp3, ogg, flac or wav; they loop). Everything in
`assets/music` ships inside the AppImage.
Switching cross-fades over 1.5 s, and the music has its own volume. The app always
starts silent; the volume is remembered, the track is not.

Frosted glass (Appearance dialog) makes the page translucent, with the transparency
adjustable. Where the compositor can blur behind windows (KDE Plasma, through KWin's
blur and contrast protocols) the thickness slider also controls how heavily the
desktop behind is frosted; on COSMIC and GNOME only the transparency applies.

The text column is 66 characters wide at most and centred, with at least 1.75 em of
padding on each side, both measured at the window's base size (18 px, stepping to 20
and 22 px at 900 and 1400 px of window width) so the column fills more of a large
screen. The size preference and Ctrl+= / Ctrl+- zoom the text inside that column, so
the margins stay put and the text reflows.

The menu's About section shows the version and a build number. `build.rs` increments
`build-number` on every build that follows a source change, so a running copy can be
matched to a build; the file is committed so numbers keep counting across machines.

Debug flags: `--anchor` starts with the anchor on, `--menu` starts with the menu open, `--dialog=welcome|appearance|sound|about` with that dialog open, `--soundtest` plays a few key sounds and exits, `--end` moves the caret
to the document end, `--selftest` runs a scripted undo/redo check and exits.

## Layout

- `src/main.rs` – application state, messages, view, shortcuts, file I/O
- `src/editor/widget.rs` – iced's `text_editor` widget, vendored and extended with the anchor (typewriter scrolling)
- `src/editor/mod.rs` – cosmic-text buffer access and caret geometry
- `src/history.rs` – undo/redo as coalesced text deltas
- `src/highlight.rs` – line-based Markdown highlighter plus spotlight dimming
- `src/palette.rs` – light, sepia and dark colour schemes
- `src/prefs.rs` – user preferences and their on-disk format
- `src/sound.rs` – mixer thread for key sounds and music, played through ALSA (loaded at runtime)
- `src/music.rs` – generated soundscapes and looped playback of the user's own tracks
- `src/dsp.rs` – filters used by the soundscapes
- `assets/sounds/` – key samples from a CC0 pack (see `assets/sounds/README.md`)
- `src/stats.rs` – word, character, sentence counts and reading time
- `assets/fonts/` – IBM Plex Mono, IBM Plex Sans and Literata for the editor, Lato Light for the menu and dialogs (all SIL OFL)

## Licence

Inkpad is released under the MIT License (see `LICENSE`). The bundled music loops are
dedicated to the public domain under CC0 1.0.

## Credits and licensing of bundled assets

- **Fonts**: IBM Plex Mono and IBM Plex Sans (IBM), Literata (TypeTogether for Google
  Fonts) and Lato (Łukasz Dziedzic), all under the SIL Open Font License 1.1; see
  `assets/fonts/README.md`.
- **Typing sounds**: cut from *Keyboard Soundpack #1* by unicaegames, OpenGameArt, CC0 1.0
  public domain dedication; see `assets/sounds/README.md`.
- **Bundled music loops**: generated by the project author with Google Lyria 3 through
  OpenRouter (`packaging/gen-tracks.py`), made to loop with `packaging/loopify.py`, and
  released under CC0 1.0.
  They live in `assets/music` and ship inside the AppImage.
- **Soundscapes**: synthesised in code.

## Roadmap

- Library sidebar, preview, export (HTML/PDF/DOCX), spell check, syntax highlight
  by part of speech, style check, wikilinks, hashtags, content blocks.
