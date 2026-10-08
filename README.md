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
| Ctrl+T | Toggle typewriter mode (not yet wired to scrolling) |
| Ctrl+Shift+T | Cycle theme: light, sepia, dark |
| Ctrl+Shift+F | Cycle font: Duo, Mono, Quattro |
| Ctrl+= / Ctrl+- | Zoom |

## Layout

- `src/main.rs` – application state, messages, view, shortcuts, file I/O
- `src/highlight.rs` – line-based Markdown highlighter plus focus-mode dimming
- `src/palette.rs` – light, sepia and dark colour schemes
- `src/stats.rs` – word, character, sentence counts and reading time
- `assets/fonts/` – iA Writer Duo, Mono and Quattro (SIL OFL, see `LICENSE-iA-Fonts.md`)

## Roadmap

Typewriter scrolling and undo/redo need access to the cosmic-text buffer behind
iced's `text_editor`, which the stock widget does not expose. The plan is to
vendor that widget (one file) and extend it. Then: library sidebar, preview,
export (HTML/PDF/DOCX), spell check, syntax highlight by part of speech,
style check, wikilinks, hashtags, content blocks.
