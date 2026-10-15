//! Undo/redo via snapshots.
//!
//! A core design rule: *all* document mutations go through an edit that the app
//! can stack; the document is never mutated ad hoc. [`History`] is that stack —
//! a small, generic, snapshot-based undo/redo that the GUI drives and that is
//! trivially testable in isolation.
//!
//! Snapshots (a full `clone` of the state per edit) are used rather than
//! inverse-command objects. Retention is bounded to 64 edits by default, drafts
//! commit once, and gesture IDs coalesce continuous updates. Imported source bytes
//! and topology are shared Arc snapshots, so they are never cloned per edit.

use std::mem;

/// An undo/redo history over a value of type `T`.
///
/// Mutations are made through [`edit`](History::edit); each edit snapshots the
/// prior state onto the undo stack and clears the redo stack. [`undo`] and
/// [`redo`] move between snapshots.
#[derive(Clone, Debug)]
pub struct History<T> {
    past: Vec<T>,
    present: T,
    future: Vec<T>,
    limit: usize,
    group: Option<u64>,
}

impl<T: Clone> History<T> {
    /// Start a history at `initial`, with nothing to undo or redo.
    pub fn new(initial: T) -> Self {
        Self::with_limit(initial, 64)
    }

    /// Limit retained edits; zero disables undo history.
    pub fn with_limit(initial: T, limit: usize) -> Self {
        Self {
            past: Vec::new(),
            present: initial,
            future: Vec::new(),
            limit,
            group: None,
        }
    }

    /// The current state.
    pub fn current(&self) -> &T {
        &self.present
    }

    /// Apply a mutation as a single undoable edit. The state *before* the edit is
    /// pushed onto the undo stack and the redo stack is cleared.
    pub fn edit(&mut self, f: impl FnOnce(&mut T)) {
        self.group = None;
        self.checkpoint();
        f(&mut self.present);
        self.future.clear();
    }

    fn checkpoint(&mut self) {
        if self.limit == 0 {
            return;
        }
        if self.past.len() == self.limit {
            self.past.remove(0);
        }
        self.past.push(self.present.clone());
    }

    /// Consecutive updates with the same gesture ID share one undo checkpoint.
    /// A normal edit or undo/redo ends the gesture.
    pub fn edit_coalesced(&mut self, gesture: u64, f: impl FnOnce(&mut T)) {
        if self.group != Some(gesture) {
            self.checkpoint();
        }
        self.group = Some(gesture);
        f(&mut self.present);
        self.future.clear();
    }

    /// Retained undo snapshots (useful for measuring history memory).
    pub fn undo_len(&self) -> usize {
        self.past.len()
    }

    /// Mutate session-only view state without creating an undo checkpoint or
    /// changing the redo stack.
    pub fn update_view(&mut self, f: impl FnOnce(&mut T)) {
        f(&mut self.present);
    }

    /// Whether there is a prior state to return to.
    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    /// Whether there is an undone state to reapply.
    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    /// Step back to the previous state. Returns `false` if there was none.
    pub fn undo(&mut self) -> bool {
        self.group = None;
        match self.past.pop() {
            Some(prev) => {
                let now = mem::replace(&mut self.present, prev);
                self.future.push(now);
                true
            }
            None => false,
        }
    }

    /// Reapply the most recently undone state. Returns `false` if there was none.
    pub fn redo(&mut self) -> bool {
        self.group = None;
        match self.future.pop() {
            Some(next) => {
                let now = mem::replace(&mut self.present, next);
                self.past.push(now);
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_undo_redo_round_trip() {
        let mut h = History::new(0i32);
        assert!(!h.can_undo() && !h.can_redo());

        h.edit(|v| *v = 1);
        h.edit(|v| *v = 2);
        assert_eq!(*h.current(), 2);
        assert!(h.can_undo() && !h.can_redo());

        assert!(h.undo());
        assert_eq!(*h.current(), 1);
        assert!(h.undo());
        assert_eq!(*h.current(), 0);
        assert!(!h.undo(), "nothing left to undo");

        assert!(h.redo());
        assert_eq!(*h.current(), 1);
        assert!(h.can_redo());
    }

    #[test]
    fn a_new_edit_clears_the_redo_stack() {
        let mut h = History::new(0i32);
        h.edit(|v| *v = 1);
        h.edit(|v| *v = 2);
        h.undo(); // present = 1, future = [2]
        assert!(h.can_redo());

        h.edit(|v| *v = 9); // branches; the redo of 2 is gone
        assert_eq!(*h.current(), 9);
        assert!(!h.can_redo());
        assert!(h.undo() && *h.current() == 1);
    }
}

#[cfg(test)]
mod bounded_tests {
    use super::*;
    use std::sync::Arc;
    #[test]
    fn history_is_bounded_and_gestures_undo_as_one_change() {
        let mut h = History::with_limit(0, 3);
        for value in 1..10 {
            h.edit(|v| *v = value);
        }
        assert_eq!(h.undo_len(), 3);
        for value in [8, 7, 6] {
            assert!(h.undo());
            assert_eq!(*h.current(), value);
        }
        assert!(!h.undo());
        h.edit_coalesced(1, |v| *v = 20);
        h.edit_coalesced(1, |v| *v = 21);
        assert_eq!(h.undo_len(), 1);
        assert!(h.undo());
        assert_eq!(*h.current(), 6);
        assert!(h.redo());
        assert_eq!(*h.current(), 21);
        h.edit_coalesced(1, |v| *v = 22); // redo ends the old gesture
        assert!(h.undo());
        assert_eq!(*h.current(), 21);
        let mut zero = History::with_limit(0, 0);
        zero.edit(|v| *v = 1);
        assert!(!zero.undo());
    }
    #[test]
    fn snapshots_share_immutable_large_assets_at_the_history_bound() {
        let bytes = Arc::new(vec![0_u8; 4 * 1024 * 1024]);
        let mut h = History::new((bytes.clone(), 0));
        for i in 0..1000 {
            h.edit(|v| v.1 = i);
        }
        assert_eq!(h.undo_len(), 64);
        assert!(Arc::ptr_eq(&h.current().0, &bytes));
        assert_eq!(
            Arc::strong_count(&bytes),
            66,
            "one payload shared by present, retained snapshots and owner"
        );
    }
}
