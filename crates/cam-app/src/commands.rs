//! Headless controller service.
use super::*;
impl AppController {
    /// Atomically group eligible operations into a new setup pattern.
    pub fn create_pattern(&mut self, setup_id: u64, requested: &[u32], name: String, kind: cam_model::PatternKind, order: cam_model::PatternOrder) -> Result<u64, String> {
        kind.validate()?;
        let setup = self.setups().iter().find(|s| s.id == setup_id).ok_or("setup not found")?;
        let members: Vec<u32> = setup.operations.iter().map(Operation::id).filter(|id| requested.contains(id)).collect();
        if members.is_empty() || members.len() != requested.iter().collect::<BTreeSet<_>>().len() { return Err("select one or more existing source operations".into()); }
        if members.iter().any(|id| setup.patterns.iter().any(|p| p.members.contains(id))) { return Err("a source operation already belongs to a pattern".into()); }
        let mut datum = None;
        for id in &members {
            let op = setup.operations.iter().find(|op| op.id() == *id).ok_or("source operation not found")?;
            if datum.is_some_and(|d| d != op.work_offset()) { return Err("pattern sources must share one work datum".into()); }
            datum = Some(op.work_offset());
        }
        let first = setup.operations.iter().position(|op| op.id() == members[0]).ok_or("source operation not found")?;
        let id = self.next_pattern_id().max(self.setups().iter().flat_map(|s| &s.patterns).map(|p| p.id.saturating_add(1)).max().unwrap_or(1));
        let next_floor = id.checked_add(1).ok_or("Pattern ID range exhausted")?;
        self.edit_document(|doc| {
            let setup = doc.setups.iter_mut().find(|s| s.id == setup_id).expect("prevalidated setup");
            let selected: BTreeSet<u32> = members.iter().copied().collect();
            let selected_ops: Vec<Operation> = setup.operations.iter().filter(|o| selected.contains(&o.id())).cloned().collect();
            setup.operations.retain(|o| !selected.contains(&o.id()));
            setup.operations.splice(first..first, selected_ops);
            setup.patterns.push(cam_model::OperationPattern { id, name, members: members.clone(), kind, order, suppressed: false });
            doc.pattern_id_floor = next_floor;
        });
        self.invalidate();
        Ok(id)
    }

    /// Edit a pattern and its ordered membership as one history transaction.
    pub fn edit_pattern(&mut self, setup_id: u64, pattern_id: u64, requested: &[u32], name: String, kind: cam_model::PatternKind, order: cam_model::PatternOrder) -> Result<(), String> {
        kind.validate()?;
        let setup = self.setups().iter().find(|s| s.id == setup_id).ok_or("setup not found")?;
        let pattern = setup.patterns.iter().find(|p| p.id == pattern_id).ok_or("pattern not found")?;
        let members: Vec<u32> = setup.operations.iter().map(Operation::id).filter(|id| requested.contains(id)).collect();
        if members.is_empty() || members.len() != requested.iter().collect::<BTreeSet<_>>().len() { return Err("select one or more existing source operations".into()); }
        for id in &members {
            if setup.patterns.iter().any(|p| p.id != pattern_id && p.members.contains(id)) { return Err("a source operation already belongs to another pattern".into()); }
            let op = setup.operations.iter().find(|op| op.id() == *id).ok_or("source operation not found")?;
            if pattern.members.iter().find_map(|old| setup.operations.iter().find(|op| op.id() == *old)).is_some_and(|first| first.work_offset() != op.work_offset()) { return Err("pattern sources must share one work datum".into()); }
        }
        let old_members: BTreeSet<u32> = pattern.members.iter().copied().collect();
        let selected: BTreeSet<u32> = members.iter().copied().collect();
        let affected: BTreeSet<u32> = old_members.union(&selected).copied().collect();
        let first = setup.operations.iter().position(|op| affected.contains(&op.id())).ok_or("pattern has no source operations")?;
        let grouped: Vec<Operation> = setup.operations.iter().filter(|op| selected.contains(&op.id())).cloned().collect();
        let removed_old: Vec<Operation> = setup.operations.iter().filter(|op| old_members.contains(&op.id()) && !selected.contains(&op.id())).cloned().collect();
        let mut candidate = setup.clone();
        candidate.operations.retain(|op| !affected.contains(&op.id()));
        candidate.operations.splice(first..first, grouped.into_iter().chain(removed_old));
        let pattern = candidate.patterns.iter_mut().find(|p| p.id == pattern_id).unwrap();
        pattern.name = name;
        pattern.members = members;
        pattern.kind = kind;
        pattern.order = order;
        candidate.validate_patterns()?;
        self.edit_document(|doc| {
            *doc.setups.iter_mut().find(|s| s.id == setup_id).unwrap() = candidate;
        });
        self.invalidate();
        Ok(())
    }

    /// Dissolve a pattern while retaining its source operations and their order.
    pub fn dissolve_pattern(&mut self, setup_id: u64, pattern_id: u64) {
        self.edit_document(|doc| if let Some(setup) = doc.setups.iter_mut().find(|s| s.id == setup_id) { setup.patterns.retain(|p| p.id != pattern_id); });
        self.invalidate();
    }

    /// Delete a pattern and all its source operations as one history edit.
    pub fn delete_pattern_and_operations(&mut self, setup_id: u64, pattern_id: u64) {
        self.edit_document(|doc| {
            if let Some(setup) = doc.setups.iter_mut().find(|s| s.id == setup_id) {
                if let Some(pattern) = setup.patterns.iter().find(|p| p.id == pattern_id).cloned() {
                    let ids: BTreeSet<u32> = pattern.members.into_iter().collect();
                    setup.operations.retain(|o| !ids.contains(&o.id()));
                    setup.patterns.retain(|p| p.id != pattern_id);
                    for id in &ids { setup.operation_heights.remove(id); setup.height_settings.remove(id); doc.excluded_operations.remove(id); doc.cad_linked_operations.remove(id); doc.geometry.bindings.remove(id); }
                }
            }
        });
        self.invalidate();
    }

    pub fn set_pattern_suppressed(&mut self, setup_id: u64, pattern_id: u64, suppressed: bool) {
        self.edit_document(|doc| if let Some(pattern) = doc.setups.iter_mut().find(|s| s.id == setup_id).and_then(|s| s.patterns.iter_mut().find(|p| p.id == pattern_id)) { pattern.suppressed = suppressed; });
        self.invalidate();
    }
    /// Delete the selected operation, selecting a neighbour (or the setup). A
    /// no-op unless an operation is selected.
    pub fn delete_selected_operation(&mut self) {
        let Selection::Operation(id) = self.selection else {
            return;
        };
        self.edit_document(|doc| {
            let setup = doc.setup_mut();
            setup.operations.retain(|o| o.id() != id);
            for pattern in &mut setup.patterns { pattern.members.retain(|member| *member != id); }
            setup.patterns.retain(|pattern| !pattern.members.is_empty());
            setup.operation_heights.remove(&id);
            setup.height_settings.remove(&id);
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
                if !taken[j]
                    && ops[i].same_work(ops[j])
                    && self.setup().height_settings.get(&ops[i].id())
                        == self.setup().height_settings.get(&ops[j].id())
                    && self.setup().heights_for(ops[i].id())
                        == self.setup().heights_for(ops[j].id())
                {
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
        if self.setup().patterns.iter().any(|pattern| pattern.members.contains(&id)) { return; }
        let setup = self.setup();
        let Some(i) = setup.operations.iter().position(|o| o.id() == id) else { return; };
        // Reorder within the operation's own origin group: swap with the nearest
        // sibling of the same `work_offset` in the chosen direction. This keeps
        // moves confined to a group, matching how the tree renders them.
        let wo = setup.operations[i].work_offset();
        let j = if up {
            setup.operations[..i].iter().rposition(|o| o.work_offset() == wo)
        } else {
            setup.operations[i + 1..].iter().position(|o| o.work_offset() == wo).map(|k| i + 1 + k)
        };
        let Some(j) = j else { return; };
        // A plain operation cannot move one member across a grouped block:
        // swapping only that member would invalidate contiguity.
        let moved_id = setup.operations[j].id();
        if setup.patterns.iter().any(|pattern| pattern.members.contains(&moved_id)) { return; }
        self.edit_document(|doc| doc.setup_mut().operations.swap(i, j));
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
