//! The text editor widget, vendored from iced 0.14 and extended with
//! typewriter scrolling. Undo/redo lives in [`crate::history`].

pub mod widget;

use iced::advanced::text::editor::Position;

/// Access to the cosmic-text buffer behind a renderer's editor.
pub trait BufferAccess {
    fn cosmic_buffer(&self) -> &cosmic_text::Buffer;
}

impl BufferAccess for iced::advanced::graphics::text::Editor {
    fn cosmic_buffer(&self) -> &cosmic_text::Buffer {
        self.buffer()
    }
}

/// Top edge of the visual line holding the caret, relative to the buffer
/// viewport. `None` if that line is not laid out (scrolled out of view).
pub fn caret_top(buffer: &cosmic_text::Buffer, position: Position) -> Option<f32> {
    let mut last = None;

    for run in buffer.layout_runs() {
        if run.line_i < position.line {
            continue;
        }
        if run.line_i > position.line {
            break;
        }

        last = Some(run.line_top);

        let covers = run
            .glyphs
            .iter()
            .any(|g| g.start <= position.column && position.column < g.end);
        if covers {
            return last;
        }
    }

    last
}
