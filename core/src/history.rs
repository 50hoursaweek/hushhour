//! Undo/redo as a stack of text deltas, coalesced the way macOS text views do:
//! consecutive typing merges into one step, a pause or a deletion starts a new one.

use std::time::{Duration, Instant};

/// A caret position: line index and byte column within the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

const COALESCE_WINDOW: Duration = Duration::from_millis(1500);
/// Undo steps kept; the oldest are dropped beyond this so history cannot grow without bound.
const MAX_STEPS: usize = 2000;
/// Bytes of removed+inserted text kept across all steps, for the same reason. The newest
/// step is always kept, so the total may exceed this by that one step, which is itself
/// bounded by the document size limit (a step holds at most one removed and one inserted
/// document's worth of text).
const MAX_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct Delta {
    /// Byte offset in the document where the change starts.
    pub offset: usize,
    pub removed: String,
    pub inserted: String,
    pub cursor_before: Position,
    pub cursor_after: Position,
}

#[derive(Debug)]
pub struct History {
    undo: Vec<Delta>,
    redo: Vec<Delta>,
    last_push: Option<Instant>,
    max_steps: usize,
    max_bytes: usize,
}

impl Default for History {
    fn default() -> Self {
        Self { undo: Vec::new(), redo: Vec::new(), last_push: None, max_steps: MAX_STEPS, max_bytes: MAX_BYTES }
    }
}

impl History {
    #[cfg(test)]
    fn with_limits(max_steps: usize, max_bytes: usize) -> Self {
        Self { max_steps, max_bytes, ..Self::default() }
    }

    /// End the current typing group, so the next edit starts a new undo step.
    /// Called after a save, so undo never jumps across the saved revision.
    pub fn end_group(&mut self) {
        self.last_push = None;
    }

    /// Record the change from `old` to `new`. Returns false if nothing changed.
    pub fn record(&mut self, old: &str, new: &str, before: Position, after: Position) -> bool {
        let Some((offset, removed, inserted)) = diff(old, new) else {
            return false;
        };
        self.redo.clear();

        let now = Instant::now();
        let recent = self.last_push.is_some_and(|t| now.duration_since(t) < COALESCE_WINDOW);
        self.last_push = Some(now);

        if recent && let Some(prev) = self.undo.last_mut() {
            // Typing: append to the previous insertion.
            if prev.removed.is_empty()
                && removed.is_empty()
                && prev.offset + prev.inserted.len() == offset
                && !inserted.chars().all(char::is_whitespace)
                && !prev.inserted.contains('\n')
            {
                prev.inserted.push_str(&inserted);
                prev.cursor_after = after;
                self.trim();
                return true;
            }
            // Backspace: grow the previous deletion backwards.
            if prev.inserted.is_empty() && inserted.is_empty() && offset + removed.len() == prev.offset {
                prev.offset = offset;
                prev.removed.insert_str(0, &removed);
                prev.cursor_after = after;
                self.trim();
                return true;
            }
            // Delete: grow the previous deletion forwards.
            if prev.inserted.is_empty() && inserted.is_empty() && offset == prev.offset {
                prev.removed.push_str(&removed);
                prev.cursor_after = after;
                self.trim();
                return true;
            }
        }

        self.undo.push(Delta { offset, removed, inserted, cursor_before: before, cursor_after: after });
        self.trim();
        true
    }

    /// Drop the oldest steps once the history is too long or holds too much text. Runs
    /// after every mutation, a coalesced one included, so the budget holds while a step
    /// grows as well as when one is added. The newest step is never dropped.
    fn trim(&mut self) {
        let mut bytes: usize = self.undo.iter().map(|d| d.removed.len() + d.inserted.len()).sum();
        let mut drop = 0;
        while self.undo.len() - drop > self.max_steps || (bytes > self.max_bytes && self.undo.len() - drop > 1) {
            bytes -= self.undo[drop].removed.len() + self.undo[drop].inserted.len();
            drop += 1;
        }
        if drop > 0 {
            self.undo.drain(..drop);
        }
    }

    /// Pop the step to revert. The caller applies it, then it becomes redoable.
    pub fn undo(&mut self) -> Option<Delta> {
        let d = self.undo.pop()?;
        self.redo.push(d.clone());
        self.last_push = None;
        Some(d)
    }

    pub fn redo(&mut self) -> Option<Delta> {
        let d = self.redo.pop()?;
        self.undo.push(d.clone());
        self.last_push = None;
        Some(d)
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.last_push = None;
    }

    #[cfg(test)]
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    #[cfg(test)]
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

}

/// Single contiguous change between two strings: (offset, removed, inserted).
fn diff(old: &str, new: &str) -> Option<(usize, String, String)> {
    if old == new {
        return None;
    }
    let (ob, nb) = (old.as_bytes(), new.as_bytes());

    let mut prefix = ob.iter().zip(nb).take_while(|(a, b)| a == b).count();
    while !old.is_char_boundary(prefix) {
        prefix -= 1;
    }

    let max_suffix = ob.len().min(nb.len()) - prefix;
    let mut suffix = ob
        .iter()
        .rev()
        .zip(nb.iter().rev())
        .take(max_suffix)
        .take_while(|(a, b)| a == b)
        .count();
    while !old.is_char_boundary(ob.len() - suffix) || !new.is_char_boundary(nb.len() - suffix) {
        suffix -= 1;
    }

    Some((
        prefix,
        old[prefix..ob.len() - suffix].to_owned(),
        new[prefix..nb.len() - suffix].to_owned(),
    ))
}

/// Convert a byte offset into a (line, column) position, assuming LF endings.
pub fn position_at(text: &str, offset: usize) -> Position {
    let offset = offset.min(text.len());
    let before = &text[..offset];
    let line = before.matches('\n').count();
    let column = before.rfind('\n').map_or(offset, |i| offset - i - 1);
    Position { line, column }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(line: usize, column: usize) -> Position {
        Position { line, column }
    }

    #[test]
    fn diff_insert_delete_replace() {
        assert_eq!(diff("abc", "abXc"), Some((2, "".into(), "X".into())));
        assert_eq!(diff("abXc", "abc"), Some((2, "X".into(), "".into())));
        assert_eq!(diff("hello world", "hello there"), Some((6, "world".into(), "there".into())));
        assert_eq!(diff("a", "a"), None);
        assert_eq!(diff("", "x"), Some((0, "".into(), "x".into())));
        assert_eq!(diff("aa", "aaa"), Some((2, "".into(), "a".into())));
    }

    #[test]
    fn diff_respects_char_boundaries() {
        assert_eq!(diff("cafè", "café"), Some((3, "è".into(), "é".into())));
    }

    #[test]
    fn typing_coalesces_and_undoes_as_one() {
        let mut h = History::default();
        assert!(h.record("", "h", p(0, 0), p(0, 1)));
        assert!(h.record("h", "hi", p(0, 1), p(0, 2)));
        assert!(h.record("hi", "hi ", p(0, 2), p(0, 3)));
        assert!(h.record("hi ", "hi t", p(0, 3), p(0, 4)));
        assert!(h.record("hi t", "hi t\n", p(0, 4), p(1, 0)));
        assert!(h.record("hi t\n", "hi t\nx", p(1, 0), p(1, 1)));
        let d = h.undo().unwrap();
        assert_eq!((d.offset, d.inserted.as_str()), (5, "x"));
        let d = h.undo().unwrap();
        assert_eq!((d.offset, d.inserted.as_str()), (4, "\n"));
        let d = h.undo().unwrap();
        assert_eq!((d.offset, d.inserted.as_str()), (2, " t"));
        let d = h.undo().unwrap();
        assert_eq!((d.offset, d.inserted.as_str()), (0, "hi"));
        assert!(h.undo().is_none());
        assert!(h.can_redo());
    }

    #[test]
    fn backspaces_coalesce() {
        let mut h = History::default();
        h.record("abc", "ab", p(0, 3), p(0, 2));
        h.record("ab", "a", p(0, 2), p(0, 1));
        let d = h.undo().unwrap();
        assert_eq!((d.offset, d.removed.as_str()), (1, "bc"));
    }

    /// Verification V06: the budget holds while a step grows by coalescing, not only when
    /// a step is pushed; and the step limit holds as steps are pushed.
    #[test]
    fn coalescing_respects_the_byte_budget() {
        let mut h = History::with_limits(10, 8);
        assert!(h.record("", "abcd", p(0, 0), p(0, 4)));
        h.end_group();
        assert!(h.record("abcd", "abcdefgh", p(0, 4), p(0, 8)));
        assert_eq!(h.undo.len(), 2, "eight bytes: at the budget, nothing dropped");
        // Typing one more letter coalesces into the newest step and pushes the total over.
        assert!(h.record("abcdefgh", "abcdefghi", p(0, 8), p(0, 9)));
        assert_eq!(h.undo.len(), 1, "the oldest step goes once the budget is exceeded");
        assert_eq!(h.undo[0].inserted, "efghi");
        // Growing a deletion is bounded the same way.
        let mut h = History::with_limits(10, 3);
        assert!(h.record("", "ab", p(0, 0), p(0, 2)));
        h.end_group();
        assert!(h.record("abcdef", "abcde", p(0, 6), p(0, 5)));
        assert!(h.record("abcde", "abcd", p(0, 5), p(0, 4)));
        assert_eq!(h.undo.len(), 1);
        assert_eq!(h.undo[0].removed, "ef");
        // The step count is capped as steps are added.
        let mut h = History::with_limits(2, 1000);
        let mut text = String::new();
        for c in ['a', 'b', 'c', 'd', 'e'] {
            let next = format!("{text}{c}");
            assert!(h.record(&text, &next, p(0, text.len()), p(0, next.len())));
            h.end_group();
            text = next;
        }
        assert_eq!(h.undo.len(), 2);
    }

    #[test]
    fn positions() {
        assert_eq!(position_at("ab\ncd", 4), p(1, 1));
        assert_eq!(position_at("ab\ncd", 2), p(0, 2));
        assert_eq!(position_at("ab\ncd", 3), p(1, 0));
    }
}
