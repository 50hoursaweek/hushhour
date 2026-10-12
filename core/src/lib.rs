//! The platform-independent heart of HushHour, shared by the Linux app and the ports.
//!
//! Nothing here knows about a window, a widget or an audio device: the Markdown scanner
//! works on one line at a time, the preview model is a plain block tree, the undo history on text deltas, the soundscapes and file
//! decoder fill plain `f32` buffers at [`dsp::RATE`]. Each platform wraps these in its own
//! editor, renders the audio through its own device layer, and keeps the document-safety
//! rules in `docs/document-safety.md`.

pub mod dsp;
pub mod history;
pub mod markdown;
pub mod music;
pub mod preview;
pub mod rng;
pub mod stats;
