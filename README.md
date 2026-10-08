# Inkpad

An iA Writer style Markdown editor for Linux, written in Rust with [iced](https://iced.rs) 0.14.
Working name; rename at will.

## Build and run

Requires a Rust toolchain (`rustup`), a C linker, and a Wayland or X11 session with Vulkan.

```sh
cargo run -- examples/sample.md
```

## Shortcuts

| Keys | Action |
|---|---|
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

Debug flags: `--typewriter` starts with typewriter mode on, `--end` moves the caret
to the document end, `--selftest` runs a scripted undo/redo check and exits.

## Layout

- `src/main.rs` – application state, messages, view, shortcuts, file I/O
- `src/editor/widget.rs` – iced's `text_editor` widget, vendored and extended with typewriter mode
- `src/editor/mod.rs` – cosmic-text buffer access and caret geometry
- `src/history.rs` – undo/redo as coalesced text deltas
- `src/highlight.rs` – line-based Markdown highlighter plus focus-mode dimming
- `src/palette.rs` – light, sepia and dark colour schemes
- `src/stats.rs` – word, character, sentence counts and reading time
- `assets/fonts/` – iA Writer Duo, Mono and Quattro (SIL OFL, see `LICENSE-iA-Fonts.md`)

## Roadmap

Library sidebar, preview, export (HTML/PDF/DOCX), spell check, syntax highlight
by part of speech, style check, wikilinks, hashtags, content blocks.
