# HushHour

A quiet Markdown editor for Linux, written in Rust with [iced](https://iced.rs) 0.14.
It was called Inkpad until 0.2.0; settings from that name are carried over.
Inspired by the best distraction-free writing apps, with its own identity:
the page is the whole window, a spotlight dims everything but what you are writing,
the caret line can be anchored mid-screen, three fonts (mono, sans, serif) suit different
moods, and typing sounds and soundscapes keep you company.

## Layout

The repository is a Cargo workspace. `core/` is the `hushhour-core` crate: the platform-
independent part (Markdown scanning, statistics, undo history, soundscapes and file decoding)
that the ports share with this Linux app. The root crate is the Linux app itself: the iced
editor, the ALSA mixer, Wayland frosted glass, preferences and packaging. Run tests with
`cargo test --workspace`. The document-safety rules every port must keep are in
`docs/document-safety.md`.

## Build and run

Requires a Rust toolchain (`rustup`), a C linker, and a Wayland or X11 session with Vulkan.

```sh
cargo run -- examples/sample.md
```

To build the AppImage, put `appimagetool` on `PATH` (or point `APPIMAGETOOL` at it)
and run `packaging/build-appimage.sh`; the result lands in `dist/`. An AppImage needs the
glibc of the machine it was built on or newer; release AppImages are built by the
GitHub Actions workflow on Ubuntu 22.04 (glibc 2.35), x86_64 only, and attached to the
release.

To build and install the Flatpak (app id `io.github._50hoursaweek.hushhour`, as submitted
to Flathub), with `org.flatpak.Builder` and the Freedesktop 26.08 SDK installed:

```sh
flatpak run org.flatpak.Builder --user --install --force-clean build-dir packaging/io.github._50hoursaweek.hushhour.yml
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
| Ctrl+Shift+P | Cycle the view: editor, split (editor beside a live preview), preview |
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
The ☰ button in the top-left corner opens the menu: file actions and the recent files, then
Appearance and Sound and music, which open dialogs, then the Writing and Window sections,
then About.

Preferences (theme, font, size, line height, spotlight, anchor, background, update checks) are
saved to `~/.config/hushhour/preferences` whenever they change; the last eight files opened
or saved are listed in the menu and kept in `~/.config/hushhour/recent`. In the Flatpak these
are `~/.var/app/io.github._50hoursaweek.hushhour/config/hushhour/preferences` and
`.../config/hushhour/recent`.

Typing sounds (two sets) and background music live in the Sound and music dialog: six
bundled loops, five generated soundscapes, or your own tracks dropped in
`~/.local/share/hushhour/music` (for the Flatpak:
`~/.var/app/io.github._50hoursaweek.hushhour/data/hushhour/music`; the dialog shows the
exact folder). The app starts silent. Sound needs the system ALSA library, which PipeWire
and PulseAudio desktops provide.

Frosted glass (Appearance dialog) makes the page translucent; on KDE Plasma the desktop
behind is blurred as well.

Background (Appearance dialog, off by default) puts a slow, faint pattern behind the page:
Waves, Swell or Drift, the text colour mixed a few percent into the page colour. It eases to
a stop while you type, pauses while the window is inactive, and rests while frosted glass is
on. It covers the page only, not the top strip or the status bar. It is drawn by a GPU shader
into a cached texture, redrawn only when it moves; with the software renderer the page stays
plain. Every
launch starts in the editor view; split and preview last for the session.

Updates: the welcome dialog (or, for anyone past it, a one-off dialog) asks whether HushHour
may check GitHub for a new version once a day; nothing is checked until the answer is yes, and
the switch is in About afterwards. The check fetches
`https://github.com/50hoursaweek/hushhour/releases/latest` without following the redirect and
reads the release it points to; nothing about you or your documents is sent. A newer release
shows as "HushHour x.y.z is available" in the status bar, which opens the release page. The
AppImage also carries standard update information, so AppImage update tools (Gear Lever,
AppImageUpdate) can update it in place from each release's `.zsync` file. Inside a Flatpak,
Flatpak handles updates and the question is not asked.

The text column is at most 66 characters wide and centred; zooming reflows the text and
keeps the margins. The About dialog shows the version and build number and links to the
credits and the installed licence texts. `build.rs` increments `build-number` on every
build that follows a source change; set `HUSHHOUR_BUILD` in the environment for a
reproducible build (Flathub and CI do), and the file is left alone.

Debug flags: `--anchor` starts with the anchor on, `--menu` starts with the menu open, `--view=split|preview` starts in that view, `--dialog=updates` opens the update question, `--check-update` checks GitHub once, prints the result and exits, `--size=WxH` sets the window size, `--screenshot=PATH` captures the window and quits, `--line=N` puts the caret on a line, `--dialog=welcome|appearance|sound|about` with that dialog open, `--soundtest` plays a few key sounds and exits, `--end` moves the caret
to the document end, `--selftest` runs a scripted undo/redo check and exits.

## Layout

- `src/main.rs` – application state, messages, view, shortcuts, file I/O
- `src/editor/widget.rs` – iced's `text_editor` widget, vendored and extended with the anchor (typewriter scrolling)
- `src/editor/mod.rs` – cosmic-text buffer access and caret geometry
- `src/history.rs` – undo/redo as coalesced text deltas
- `src/highlight.rs` – line-based Markdown highlighter plus spotlight dimming
- `src/update.rs` – the optional daily update check against GitHub Releases
- `src/backdrop.rs` – the animated background as an iced shader widget (patterns and motion in `core/src/backdrop.rs`)
- `src/palette.rs` – light, sepia and dark colour schemes
- `src/prefs.rs` – user preferences and their on-disk format
- `src/sound.rs` – mixer thread for key sounds and music, played through ALSA (loaded at runtime)
- `src/music.rs` – generated soundscapes and looped playback of the user's own tracks
- `src/dsp.rs` – filters used by the soundscapes
- `assets/sounds/` – key samples from a CC0 pack (see `assets/sounds/README.md`)
- `src/stats.rs` – word, character, sentence counts and reading time
- `assets/fonts/` – IBM Plex Mono, IBM Plex Sans and Literata for the editor, Lato Regular for the menu and dialogs (all SIL OFL)

## Licence

HushHour is released under the MIT License (see `LICENSE`). The bundled music loops are
dedicated to the public domain under CC0 1.0.

## Credits and licensing of bundled assets

- **Fonts**: IBM Plex Mono and IBM Plex Sans (IBM), Literata (TypeTogether for Google
  Fonts) and Lato (Łukasz Dziedzic), all unmodified upstream files under the SIL Open
  Font License 1.1; see `assets/fonts/README.md`.
- **Code**: the text editor widget in `src/editor/widget.rs` is iced's, extended; iced is
  MIT (`packaging/LICENSE-iced.txt`). Both packages install every licence text under
  `share/licenses/hushhour`, reachable from About.
- **Typing sounds**: cut from *Keyboard Soundpack #1* by unicaegames, OpenGameArt, CC0 1.0
  public domain dedication; see `assets/sounds/README.md`.
- **Bundled music loops**: generated by the project author with Google Lyria 3 through
  OpenRouter (`packaging/gen-tracks.py`), made to loop with `packaging/loopify.py`, and
  released under CC0 1.0 by the author. Basis: they were generated on 2026-10-11 through
  OpenRouter with Google's Lyria 3 Pro, under the Gemini API terms in force that day, which
  state that Google claims no ownership of generated content; OpenRouter's terms add no
  claim of their own. That is the author's right to dedicate, not a warranty about every
  conceivable third-party claim.
  They live in `assets/music` and ship inside the AppImage.
- **Soundscapes**: synthesised in code.

## Roadmap

- Library sidebar, preview, export (HTML/PDF/DOCX), spell check, syntax highlight
  by part of speech, style check, wikilinks, hashtags, content blocks.
