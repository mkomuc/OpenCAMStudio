//! Headless controller service.
use super::*;
impl AppController {
    /// Delete the selected operation, selecting a neighbour (or the setup). A
    /// no-op unless an operation is selected.
    pub fn delete_selected_operation(&mut self) {
        let Selection::Operation(id) = self.selection else {
            return;
        };
        self.edit_document(|doc| {
            doc.setup_mut().operations.retain(|o| o.id() != id);
            doc.geometry.bindings.remove(&id);
            doc.excluded_operations.remove(&id);
            doc.cad_linked_operations.remove(&id);
        });
        self.selection = self
            .document
            .current()
            .setup()
            .operations
            .first()
            .map_or(Selection::Setup, |o| Selection::Operation(o.id()));
        self.invalidate();
    }

    /// Whether operation `id` is excluded from toolpath generation.
    pub fn is_operation_excluded(&self, id: u32) -> bool {
        self.document().excluded_operations.contains(&id)
    }

    /// Clusters of **included** operations that are exact duplicates of one
    /// another — identical in every field but their id (see
    /// [`Operation::same_work`]). Each returned group holds ≥2 ids in document
    /// order; the first is the "original", the rest its exact copies.
    ///
    /// Excluded operations never reach the post, so they are ignored: excluding a
    /// twin is the sanctioned way to keep an exact duplicate around (e.g. a spring
    /// pass that is currently switched off) without tripping the warning.
    pub fn duplicate_operation_groups(&self) -> Vec<Vec<u32>> {
        let ops: Vec<&Operation> = self
            .document()
            .setup()
            .operations
            .iter()
            .filter(|op| !self.is_operation_excluded(op.id()))
            .collect();
        let mut groups: Vec<Vec<u32>> = Vec::new();
        let mut taken = vec![false; ops.len()];
        for i in 0..ops.len() {
            if taken[i] {
                continue;
            }
            let mut group = vec![ops[i].id()];
            for j in (i + 1)..ops.len() {
                if !taken[j] && ops[i].same_work(ops[j]) {
                    taken[j] = true;
                    group.push(ops[j].id());
                }
            }
            if group.len() > 1 {
                groups.push(group);
            }
        }
        groups
    }

    /// The set of operation ids that are exact duplicates of another included
    /// operation (the flattened [`Self::duplicate_operation_groups`]). Drives the
    /// project-tree "duplicate" marker.
    pub fn duplicate_operation_ids(&self) -> BTreeSet<u32> {
        self.duplicate_operation_groups()
            .into_iter()
            .flatten()
            .collect()
    }

    /// Include or exclude operation `id` from toolpath generation (it stays in
    /// the tree either way).
    pub fn set_operation_excluded(&mut self, id: u32, excluded: bool) {
        if !self
            .setups()
            .iter()
            .any(|s| s.operations.iter().any(|o| o.id() == id))
            || self.is_operation_excluded(id) == excluded
        {
            return;
        }
        self.edit_document(|doc| {
            if excluded {
                doc.excluded_operations.insert(id);
            } else {
                doc.excluded_operations.remove(&id);
            }
        });
        self.invalidate();
    }

    /// Move operation `id` one step earlier (`up`) or later in execution order.
    /// A no-op if it can't move that way.
    pub fn move_operation(&mut self, id: u32, up: bool) {
        let owner = self
            .setups()
            .iter()
            .find(|s| s.operations.iter().any(|op| op.id() == id))
            .map(|s| s.id);
        if let Some(owner) = owner {
            if owner != self.active_setup_id {
                let _ = self.activate_setup(owner);
            }
        } else {
            return;
        }
        self.edit_document(|doc| {
            let ops = &mut doc.setup_mut().operations;
            if let Some(i) = ops.iter().position(|o| o.id() == id) {
                // Reorder within the operation's own origin group: swap with the nearest
                // sibling of the same `work_offset` in the chosen direction. This keeps
                // moves confined to a group, matching how the tree renders them.
                let wo = ops[i].work_offset();
                let j = if up {
                    ops[..i].iter().rposition(|o| o.work_offset() == wo)
                } else {
                    ops[i + 1..]
                        .iter()
                        .position(|o| o.work_offset() == wo)
                        .map(|k| i + 1 + k)
                };
                if let Some(j) = j {
                    ops.swap(i, j);
                }
            }
        });
        self.invalidate();
    }

    /// Move the selected operation one step in execution order.
    pub fn move_selected_operation(&mut self, up: bool) {
        if let Selection::Operation(id) = self.selection {
            self.move_operation(id, up);
        }
    }

    /// Undo the last document edit.
    pub fn undo(&mut self) -> bool {
        let changed = self.document.undo();
        if changed {
            self.restore_active_setup_view();
            self.sync_drawing_geometry();
            self.reconcile_selection();
            self.invalidate();
        }
        changed
    }

    /// Redo an undone document edit.
    pub fn redo(&mut self) -> bool {
        let changed = self.document.redo();
        if changed {
            self.restore_active_setup_view();
            self.sync_drawing_geometry();
            self.reconcile_selection();
            self.invalidate();
        }
        changed
    }

    /// Whether an undo / redo is available.
    pub fn can_undo(&self) -> bool {
        self.document.can_undo()
    }
    pub fn can_redo(&self) -> bool {
        self.document.can_redo()
    }
}
