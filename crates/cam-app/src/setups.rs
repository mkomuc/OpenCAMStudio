//! Physical setup management. Geometry is shared; machining is evaluated in the
//! active setup orientation, with the datum translated only when posting.
use super::*;
use cam_model::{EntityRef, FrameAxisReference, SetupFrame};

impl AppController {
    pub fn active_setup_id(&self) -> u64 {
        self.active_setup_id
    }
    pub fn setups(&self) -> &[Setup] {
        &self.document().setups
    }

    pub fn activate_setup(&mut self, id: u64) -> Result<(), String> {
        if !self.setups().iter().any(|s| s.id == id) {
            return Err("Setup no longer exists".into());
        }
        if id == self.active_setup_id {
            return Ok(());
        }
        self.active_setup_id = id;
        self.document.update_view(|doc| doc.active_setup_id = id);
        self.active_origin = self.setup().origin_index;
        self.disabled_origins.clear();
        self.pending_op = None;
        self.selection = Selection::Setup;
        self.cad_selection.clear();
        self.invalidate();
        Ok(())
    }

    pub(super) fn restore_active_setup_view(&mut self) {
        if !self.setups().iter().any(|s| s.id == self.active_setup_id) {
            self.active_setup_id = self.setups()[0].id;
        }
        let id = self.active_setup_id;
        self.document.update_view(|doc| doc.active_setup_id = id);
        if !self.setup().origin_indices().contains(&self.active_origin) {
            self.active_origin = self.setup().origin_index;
        }
    }

    pub fn add_setup(&mut self) -> u64 {
        let mut setup = self.setup().clone();
        setup.id = self.document().next_setup_id();
        setup.name = format!("Setup {}", setup.id);
        setup.program_name = format!("setup-{}", setup.id);
        setup.operations.clear();
        setup.extra_origins.clear();
        let id = setup.id;
        self.document.edit(move |doc| doc.setups.push(setup));
        self.activate_setup(id).expect("new setup exists");
        id
    }

    pub fn duplicate_setup(&mut self, id: u64) -> Result<u64, String> {
        let mut copy = self
            .setups()
            .iter()
            .find(|s| s.id == id)
            .cloned()
            .ok_or("Missing setup")?;
        copy.id = self.document().next_setup_id();
        copy.name = format!("{} (copy)", copy.name);
        copy.program_name = format!("setup-{}", copy.id);
        let mut next_id = self.next_op_id();
        let mut bindings = Vec::new();
        let mut linked = Vec::new();
        for op in &mut copy.operations {
            if self.document().cad_linked_operations.contains(&op.id()) {
                linked.push(next_id);
            }
            if let Some(mut binding) = self.geometry_state().bindings.get(&op.id()).cloned() {
                binding.resolved_fingerprint = None;
                bindings.push((next_id, binding));
            }
            op.set_id(next_id);
            next_id = next_id
                .checked_add(1)
                .ok_or("Operation ID range exhausted")?;
        }
        let new_id = copy.id;
        self.document.edit(move |doc| {
            let at = doc.setups.iter().position(|s| s.id == id).unwrap() + 1;
            doc.setups.insert(at, copy);
            doc.geometry.bindings.extend(bindings);
            doc.cad_linked_operations.extend(linked);
        });
        self.activate_setup(new_id)?;
        Ok(new_id)
    }

    pub fn delete_setup(&mut self, id: u64) -> Result<(), String> {
        if self.setups().len() == 1 {
            return Err("Keep at least one setup in the project".into());
        }
        let removed = self
            .setups()
            .iter()
            .find(|s| s.id == id)
            .ok_or("Missing setup")?;
        let ids: BTreeSet<_> = removed.operations.iter().map(Operation::id).collect();
        self.document.edit(move |doc| {
            doc.setups.retain(|s| s.id != id);
            doc.geometry.bindings.retain(|op, _| !ids.contains(op));
            doc.cad_linked_operations.retain(|op| !ids.contains(op));
        });
        self.restore_active_setup_view();
        let remaining: BTreeSet<_> = self
            .setups()
            .iter()
            .flat_map(|s| &s.operations)
            .map(Operation::id)
            .collect();
        self.excluded.retain(|id| remaining.contains(id));
        self.active_origin = self.setup().origin_index;
        self.pending_op = None;
        self.selection = Selection::Setup;
        self.cad_selection.clear();
        self.disabled_origins.clear();
        self.invalidate();
        Ok(())
    }

    pub fn move_setup(&mut self, id: u64, up: bool) {
        let Some(at) = self.setups().iter().position(|s| s.id == id) else {
            return;
        };
        let next = if up {
            at.checked_sub(1)
        } else {
            (at + 1 < self.setups().len()).then_some(at + 1)
        };
        if let Some(next) = next {
            self.document.edit(|doc| doc.setups.swap(at, next));
        }
    }

    /// Transfer an operation definition and its CAD link, resolving in the
    /// destination frame before publishing a single undoable edit.
    pub fn transfer_operation_to_setup(
        &mut self,
        id: u32,
        destination: u64,
        copy: bool,
    ) -> Result<u32, String> {
        let source = self
            .setups()
            .iter()
            .find(|s| s.operations.iter().any(|op| op.id() == id))
            .ok_or("Missing operation")?;
        let target = self
            .setups()
            .iter()
            .find(|s| s.id == destination)
            .ok_or("Missing destination setup")?;
        if source.id == destination && !copy {
            return Ok(id);
        }
        if !self.geometry_state().bindings.contains_key(&id)
            && source.frame.world_from_setup != target.frame.world_from_setup
        {
            return Err("An operation with explicit planar geometry needs the same setup axes; create a linked CAD operation for another orientation".into());
        }
        let source_id = source.id;
        let mut definition = source.clone();
        definition.operations.retain(|op| op.id() == id);
        let required = definition.operations[0].tools();
        definition
            .tools
            .retain(|tool| required.contains(&tool.number));
        if definition.tools.len() != required.len() {
            return Err("Source operation has a missing tool".into());
        }
        let mut reserved: BTreeSet<_> = target.tools.iter().map(|tool| tool.number).collect();
        let mut mapping = std::collections::BTreeMap::new();
        for tool in &mut definition.tools {
            let old = tool.number;
            let number = if let Some(existing) = target
                .tools
                .iter()
                .find(|existing| existing.identity() == tool.identity())
            {
                existing.number
            } else {
                let mut number = old.max(1);
                while reserved.contains(&number) {
                    number = number.checked_add(1).ok_or("Tool number range exhausted")?;
                }
                number
            };
            reserved.insert(number);
            mapping.insert(old, number);
            tool.number = number;
        }
        for op in &mut definition.operations {
            op.map_tools(|old| mapping.get(&old).copied().unwrap_or(old));
        }
        let mut operation = definition.operations.remove(0);
        let new_id = if copy { self.next_op_id() } else { id };
        operation.set_id(new_id);
        operation.set_work_offset(target.origin_index);
        let mut document = self.document().clone();
        if copy {
            if document.cad_linked_operations.contains(&id) {
                document.cad_linked_operations.insert(new_id);
            }
            if let Some(mut binding) = document.geometry.bindings.get(&id).cloned() {
                binding.resolved_fingerprint = None;
                document.geometry.bindings.insert(new_id, binding);
            }
        } else {
            document
                .setups
                .iter_mut()
                .find(|s| s.id == source_id)
                .unwrap()
                .operations
                .retain(|op| op.id() != id);
            if let Some(binding) = document.geometry.bindings.get_mut(&id) {
                binding.resolved_fingerprint = None;
            }
        }
        document.active_setup_id = destination;
        let target = document.setup_mut();
        for tool in definition.tools {
            if !target
                .tools
                .iter()
                .any(|existing| existing.number == tool.number)
            {
                target.tools.push(tool);
            }
        }
        target.operations.push(operation);
        if document.geometry.bindings.contains_key(&new_id) {
            crate::cad_operations::validate_binding_edit(&mut document, new_id)?;
        }
        self.document.edit(move |current| *current = document);
        self.active_setup_id = destination;
        self.active_origin = self.setup().origin_index;
        self.pending_op = None;
        self.disabled_origins.clear();
        self.select(Selection::Operation(new_id));
        self.invalidate();
        Ok(new_id)
    }

    /// Commit an entire operation inspector draft, including linked-height
    /// overrides, without publishing any intermediate dropdown/checkbox edit.
    pub fn apply_operation_settings(
        &mut self,
        operation: Operation,
        overrides: Option<cam_model::GeometryOverrides>,
    ) -> Result<(), String> {
        self.apply_operation_settings_with_tools(operation, overrides, Vec::new())
    }

    pub fn apply_operation_settings_with_tools(
        &mut self,
        operation: Operation,
        overrides: Option<cam_model::GeometryOverrides>,
        tools: Vec<Tool>,
    ) -> Result<(), String> {
        let id = operation.id();
        let old = self
            .operation(id)
            .ok_or("Operation is not in the active setup")?;
        if self.document().cad_linked_operations.contains(&id)
            && !self.geometry_state().bindings.contains_key(&id)
        {
            return Err("Linked CAD geometry is missing; repair the geometry before applying operation settings".into());
        }
        let mut document = self.document().clone();
        for tool in tools {
            if let Some(existing) = document
                .setup()
                .tools
                .iter()
                .find(|existing| existing.number == tool.number)
            {
                if existing.identity() != tool.identity() {
                    return Err(
                        "A draft tool number conflicts with the setup; choose the tool again"
                            .into(),
                    );
                }
            } else {
                document.setup_mut().tools.push(tool);
            }
        }
        if operation
            .tools()
            .iter()
            .any(|number| !document.setup().tools.iter().any(|t| t.number == *number))
        {
            return Err("Operation refers to a missing tool".into());
        }
        let heights = document.setup().heights;
        let before = old.legacy_cutting_heights(&heights);
        if let Some(binding) = document.geometry.bindings.get_mut(&id) {
            let previous_overrides = binding.overrides.clone();
            crate::cad_operations::capture_height_override(
                Some(binding),
                before,
                &operation,
                &heights,
            );
            if let Some(overrides) = overrides {
                // Unchanged linked-height fields must not erase a depth edit
                // captured from the ordinary numeric operation controls.
                if overrides != previous_overrides {
                    binding.overrides = overrides;
                }
            }
            binding.resolved_fingerprint = None;
        } else if overrides.is_some() {
            return Err("Height links are available only for CAD operations".into());
        }
        *document
            .setup_mut()
            .operations
            .iter_mut()
            .find(|op| op.id() == id)
            .unwrap() = operation;
        if document.geometry.bindings.contains_key(&id) {
            crate::cad_operations::validate_binding_edit(&mut document, id)?;
        }
        if document == *self.document() {
            return Ok(());
        }
        self.document.edit(move |current| *current = document);
        self.invalidate();
        Ok(())
    }

    /// Validate every tab before a single undoable commit. The caller owns the
    /// draft; rejected settings leave committed state and toolpaths untouched.
    pub fn apply_setup_settings(&mut self, mut setup: Setup) -> Result<(), String> {
        let old = self
            .setups()
            .iter()
            .find(|s| s.id == setup.id)
            .ok_or("Missing setup")?;
        if setup.id != self.active_setup_id {
            return Err("Activate the setup before editing its settings".into());
        }
        if old.operations != setup.operations || old.tools != setup.tools {
            return Err("Operations or tools changed while editing; reopen setup settings".into());
        }
        if setup.name.trim().is_empty() {
            return Err("Enter a setup name".into());
        }
        self.validate_setup_frame(&setup)?;
        if !setup.origin.iter().all(|v| v.is_finite()) {
            return Err("Origin must contain finite coordinates".into());
        }
        if setup.origin_index == 0
            || setup
                .extra_origins
                .iter()
                .any(|o| o.index == setup.origin_index)
        {
            return Err(
                "Choose a positive work offset distinct from the legacy extra origins".into(),
            );
        }
        if ![
            setup.heights.clearance,
            setup.heights.retract,
            setup.heights.top_of_stock,
        ]
        .iter()
        .all(|v| v.is_finite())
            || setup.heights.clearance < setup.heights.retract
            || setup.heights.retract < setup.heights.top_of_stock
        {
            return Err(
                "Clearance must be at or above retract, and retract at or above stock top".into(),
            );
        }
        if let Some(height) = setup.tool_change_height {
            if !height.is_finite() || height < setup.heights.clearance {
                return Err("Tool-change height must be at or above clearance".into());
            }
        }
        if setup
            .model_instances
            .iter()
            .any(|id| self.geometry_state().instance(*id).is_none())
        {
            return Err("A selected machining model no longer exists".into());
        }
        let (min, max) = self.pending_setup_model_bounds(&setup);
        match setup.stock {
            Stock::FixedBox {
                size,
                model_position,
            } if size.iter().any(|v| !v.is_finite() || *v <= 0.)
                || model_position
                    .iter()
                    .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v)) =>
            {
                return Err(
                    "Choose positive stock dimensions and model positions within the stock".into(),
                )
            }
            Stock::RelativeBox { offsets }
                if offsets.iter().flatten().any(|v| !v.is_finite() || *v < 0.) =>
            {
                return Err("Stock allowances must be finite and non-negative".into())
            }
            Stock::BoundingBox {
                x_offset,
                y_offset,
                top,
                thickness,
            } if ![x_offset, y_offset, top, thickness]
                .iter()
                .all(|v| v.is_finite())
                || thickness <= 0. =>
            {
                return Err("Stock dimensions must be finite and thickness positive".into())
            }
            _ => {}
        }
        let (stock_min, stock_max) = setup.stock.resolve_bounds(min, max);
        if (0..3).any(|i| {
            !stock_min[i].is_finite() || !stock_max[i].is_finite() || stock_max[i] <= stock_min[i]
        }) {
            return Err("Stock must have finite, positive dimensions on all axes".into());
        }
        if !self.geometry_state().instances.is_empty()
            && (0..3).any(|i| stock_min[i] > min[i] + 1e-6 || stock_max[i] < max[i] - 1e-6)
        {
            return Err("Stock does not contain the selected machining model".into());
        }
        let previous_offset = old.origin_index;
        if setup.origin_index != previous_offset {
            for operation in &mut setup.operations {
                if operation.work_offset() == previous_offset {
                    operation.set_work_offset(setup.origin_index);
                }
            }
        }
        if *old == setup {
            return Ok(());
        }
        self.document.edit(move |doc| {
            let slot = doc.setups.iter_mut().find(|s| s.id == setup.id).unwrap();
            *slot = setup;
            crate::cad_operations::refresh_binding_heights(doc);
        });
        self.active_origin = self.setup().origin_index;
        self.invalidate();
        Ok(())
    }

    pub fn pending_frame_from_model(
        &self,
        instance_id: u64,
    ) -> Result<(SetupFrame, [f64; 3]), String> {
        let frame = SetupFrame::from_model_instance(self.geometry_state(), instance_id)?;
        let instance = self
            .geometry_state()
            .instance(instance_id)
            .ok_or("Missing model instance")?;
        let origin = frame.to_setup_vector(instance.placement.translation);
        Ok((frame, origin))
    }

    pub fn pending_frame_from_edges(
        &self,
        references: [Option<EntityRef>; 3],
        flips: [bool; 3],
    ) -> Result<SetupFrame, String> {
        let [x, y, z] = references;
        SetupFrame::from_edge_references(
            self.geometry_state(),
            x.map(|entity| FrameAxisReference {
                entity,
                flipped: flips[0],
            }),
            y.map(|entity| FrameAxisReference {
                entity,
                flipped: flips[1],
            }),
            z.map(|entity| FrameAxisReference {
                entity,
                flipped: flips[2],
            }),
        )
    }

    pub(super) fn validate_setup_frame(&self, setup: &Setup) -> Result<(), String> {
        setup.frame.validate()?;
        let refs = &setup.frame.references;
        let linked = if refs.x.is_some() || refs.y.is_some() || refs.z.is_some() {
            Some(SetupFrame::from_edge_references(
                self.geometry_state(),
                refs.x.clone(),
                refs.y.clone(),
                refs.z.clone(),
            )?)
        } else if let Some(id) = refs.model_instance {
            Some(SetupFrame::from_model_instance(self.geometry_state(), id)?)
        } else {
            None
        };
        if let Some(linked) = linked {
            if linked
                .world_from_setup
                .rotation
                .iter()
                .flatten()
                .zip(setup.frame.world_from_setup.rotation.iter().flatten())
                .any(|(a, b)| (a - b).abs() > 1e-6)
            {
                return Err("WCS references changed; reselect the setup coordinate system".into());
            }
        }
        Ok(())
    }

    pub fn setup_geometry_state(&self) -> Result<cam_model::GeometryState, String> {
        self.validate_setup_frame(self.setup())?;
        crate::cad_operations::geometry_for_setup(self.document())
    }

    pub fn setup_model_bounds(&self) -> ([f64; 3], [f64; 3]) {
        self.pending_setup_model_bounds(self.setup())
    }

    pub fn pending_setup_model_bounds(&self, setup: &Setup) -> ([f64; 3], [f64; 3]) {
        crate::cad_operations::model_bounds(&self.document().geometry, setup)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn drill() -> Operation {
        Operation::Drill(DrillOp {
            id: 0,
            tool: 1,
            points: vec![[10., 10.]],
            depth: 2.,
            start_offset: 0.,
            peck: None,
            dwell: None,
            spindle_rpm: 1000.,
            work_offset: 1,
            feed: 100.,
        })
    }

    #[test]
    fn operation_copy_remaps_conflicting_tools_and_undoes_as_one_command() {
        let mut app = AppController::new(crate::default_machine());
        app.add_operation(drill());
        let first = app.active_setup_id();
        let id = app.setup().operations[0].id();
        let second = app.add_setup();
        app.edit_tool(0, |t| t.diameter = 2.);
        app.activate_setup(first).unwrap();
        let copied = app.transfer_operation_to_setup(id, second, true).unwrap();
        assert_ne!(copied, id);
        let tool = app
            .setup()
            .tools
            .iter()
            .find(|t| t.number == app.operation(copied).unwrap().tool())
            .unwrap();
        assert_eq!(tool.diameter, 6.);
        assert_ne!(tool.number, 1);
        assert!(app
            .setups()
            .iter()
            .find(|s| s.id == first)
            .unwrap()
            .operations
            .iter()
            .any(|op| op.id() == id));
        assert!(app.undo());
        assert!(app.setup().operations.is_empty());
        assert_eq!(app.setup().tools.len(), 1);
        assert!(app.redo());
        assert_eq!(app.setup().operations[0].id(), copied);
    }

    #[test]
    fn failed_transfer_and_invalid_setup_drafts_never_modify_committed_state() {
        let mut app = AppController::new(crate::default_machine());
        app.add_operation(drill());
        let first = app.active_setup_id();
        let second = app.add_setup();
        app.edit_setup(|s| {
            s.frame = SetupFrame::from_axes(Some([1., 0., 0.]), Some([0., -1., 0.]), None).unwrap()
        });
        app.activate_setup(first).unwrap();
        let before = app.document().clone();
        assert!(app.transfer_operation_to_setup(0, second, false).is_err());
        assert_eq!(app.document(), &before);
        let mut draft = app.setup().clone();
        draft.stock = Stock::FixedBox {
            size: [10., 10., 10.],
            model_position: [2.; 3],
        };
        assert!(app.apply_setup_settings(draft).is_err());
        assert_eq!(app.document(), &before);
    }

    #[test]
    fn work_offset_edits_update_operations_and_are_one_undoable_transaction() {
        let mut app = AppController::new(crate::default_machine());
        app.edit_stock(|s| {
            *s = Stock::FixedBox {
                size: [20.; 3],
                model_position: [0.5; 3],
            }
        });
        app.add_operation(drill());
        let mut draft = app.setup().clone();
        draft.name = "Top".into();
        draft.material = "Wood".into();
        draft.origin_index = 2;
        app.apply_setup_settings(draft).unwrap();
        assert_eq!(app.setup().operations[0].work_offset(), 2);
        assert_eq!(app.setup().material, "Wood");
        assert!(app.undo());
        assert_eq!(app.setup().origin_index, 1);
        assert_eq!(app.setup().operations[0].work_offset(), 1);
        assert!(app.setup().material.is_empty());
    }
}
