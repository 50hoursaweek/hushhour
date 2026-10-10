# Inkpad

An iA Writer style Markdown editor for Linux, written in Rust with [iced](https://iced.rs) 0.14.
Working name; rename at will.

## Build and run

Requires a Rust toolchain (`rustup`), a C linker, and a Wayland or X11 session with Vulkan.

```sh
cargo run -- examples/sample.md
```

To build the AppImage, put `appimagetool` on `PATH` (or point `APPIMAGETOOL` at it)
and run `packaging/build-appimage.sh`; the result lands in `dist/`.

## Shortcuts

| Keys | Action |
|---|---|
| Ctrl+, | Open the menu; Esc or a click outside closes it or any dialog |
| Ctrl+O / Ctrl+S / Ctrl+Shift+S / Ctrl+N | Open, save, save as, new |
| Ctrl+D | Cycle focus mode: off, paragraph, sentence |
| Ctrl+T | Toggle typewriter mode (caret line stays vertically centred) |
| Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y | Undo, redo |
| Ctrl+Shift+T | Cycle theme: light, sepia, dark |
| Ctrl+Shift+F | Cycle font: Duo, Mono, Quattro |
| Ctrl+= / Ctrl+- | Zoom |
| F11 | Toggle fullscreen |
| Ctrl+Shift+D | Toggle window decorations (off by default) |
| Ctrl+Q / Ctrl+W | Quit |

The window has no title bar. Drag the top strip or the status bar to move it,
double-click the top strip to maximise, and use your compositor's resize
gesture (Meta + right-drag on KDE) or Ctrl+Shift+D to bring decorations back.
The ☰ button in the top-left corner opens the menu: file actions, then Appearance and
Sound and music, which open dialogs, then the Writing and Window sections, then About.

Preferences (theme, font, size, line height, focus mode, typewriter mode) are
saved to `~/.config/inkpad/preferences` whenever they change.

A key sound plays on every edit; the Sound and music dialog has a switch, a choice of four
sound sets (soft keys, keyboard, typewriter, mechanical) and a volume slider for it.
Sound needs `libasound` (ALSA) at run time, which PipeWire and PulseAudio desktops provide;
without it the editor is simply silent.

Background music for focus lives in the Sound and music dialog: five generated
soundscapes that never repeat (spring rain with an occasional gong, a night in
Menorca with crickets, a train ride, curtains swaying in a breeze) and two binaural
beats (10 Hz calm, 15 Hz focus; headphones needed), four bundled tracks
(`assets/music`, shipped in the AppImage under `usr/share/inkpad/music`), plus any
tracks you put in `~/.local/share/inkpad/music` (mp3, ogg, flac or wav; they loop).
Switching cross-fades over 1.5 s, and the music has its own volume.

The text column is 66 characters wide at most and centred, with at least 1.75 em of
padding on each side, both measured at the window's base size (18 px, stepping to 20
and 22 px at 900 and 1400 px of window width) so the column fills more of a large
screen. The size preference and Ctrl+= / Ctrl+- zoom the text inside that column, so
the margins stay put and the text reflows.

The menu's About section shows the version and a build number. `build.rs` increments
`build-number` on every build that follows a source change, so a running copy can be
matched to a build; the file is committed so numbers keep counting across machines.

Debug flags: `--typewriter` starts with typewriter mode on, `--menu` starts with the menu open, `--dialog=appearance|sound|about` with that dialog open, `--soundtest` plays a few key sounds and exits, `--end` moves the caret
to the document end, `--selftest` runs a scripted undo/redo check and exits.

## Layout

- `src/main.rs` – application state, messages, view, shortcuts, file I/O
- `src/editor/widget.rs` – iced's `text_editor` widget, vendored and extended with typewriter mode
- `src/editor/mod.rs` – cosmic-text buffer access and caret geometry
- `src/history.rs` – undo/redo as coalesced text deltas
- `src/highlight.rs` – line-based Markdown highlighter plus focus-mode dimming
- `src/palette.rs` – light, sepia and dark colour schemes
- `src/prefs.rs` – user preferences and their on-disk format
- `src/sound.rs` – mixer thread for key sounds and music, played through ALSA (loaded at runtime)
- `src/music.rs` – generated soundscapes, binaural beats and looped playback of the user's own tracks
- `assets/sounds/` – key samples: a CC0 set by unicaegames plus two cut from supplied recordings (see `LICENSE-sounds.md`)
- `src/stats.rs` – word, character, sentence counts and reading time
- `assets/fonts/` – iA Writer Duo, Mono and Quattro (SIL OFL, see `LICENSE-iA-Fonts.md`)

## Roadmap

Library sidebar, preview, export (HTML/PDF/DOCX), spell check, syntax highlight
by part of speech, style check, wikilinks, hashtags, content blocks.
