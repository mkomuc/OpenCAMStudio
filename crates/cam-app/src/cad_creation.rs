//! Shared creation inputs for linked CAD operations.
use super::*;

impl AppController {
    /// Embed a cutter, resolve geometry and cutting data, and publish one change.
    pub fn create_cad_operation_from_tool(
        &mut self,
        kind: OpKind,
        entities: Vec<cam_model::EntityRef>,
        mut tool: Tool,
        cutting: CuttingData,
    ) -> Result<u32, String> {
        if ![cutting.rpm, cutting.feed, cutting.plunge_feed]
            .iter()
            .all(|v| v.is_finite())
            || cutting.rpm < 0.
            || cutting.feed <= 0.
            || cutting.plunge_feed <= 0.
        {
            return Err("Cutting data requires finite RPM and positive feeds".into());
        }
        let mut document = self.document().clone();
        let number = if let Some(existing) = document
            .setup()
            .tools
            .iter()
            .find(|saved| saved.identity() == tool.identity())
        {
            existing.number
        } else {
            tool.number = document
                .setup()
                .tools
                .iter()
                .map(|saved| saved.number)
                .max()
                .unwrap_or(0)
                .saturating_add(1)
                .max(1);
            let number = tool.number;
            document.setup_mut().tools.push(tool);
            number
        };
        let id = self.next_op_id();
        let mut defaults = self.defaults;
        defaults.spindle_rpm = cutting.rpm;
        defaults.feed = cutting.feed;
        defaults.plunge_feed = cutting.plunge_feed;
        let (operation, binding) = crate::cad_operations::create_operation(
            &document,
            kind,
            entities,
            id,
            number,
            self.active_origin,
            defaults,
        )?;
        document
            .setup_mut()
            .height_settings
            .insert(id, cam_model::OperationHeightSettings::default());
        document.setup_mut().operations.push(operation);
        document.geometry.bindings.insert(id, binding);
        document.cad_linked_operations.insert(id);
        crate::cad_operations::validate_binding_edit(&mut document, id)?;
        self.edit_document(move |saved| *saved = document);
        self.select(Selection::Operation(id));
        self.invalidate();
        Ok(id)
    }
    /// Publish the shared operation editor in a single undoable transaction.
    pub fn commit_operation_creation(
        &mut self,
        mut operation: Operation,
        binding: Option<cam_model::GeometryBinding>,
        settings: cam_model::OperationHeightSettings,
        tools: Vec<Tool>,
    ) -> Result<u32, String> {
        let replacing = self.pending_op.as_ref().and_then(|p| p.replacing);
        let id = replacing.unwrap_or_else(|| self.next_op_id());
        operation.set_id(id);
        operation.set_work_offset(self.active_origin);
        let mut document = self.document().clone();
        for tool in tools {
            if !document
                .setup()
                .tools
                .iter()
                .any(|t| t.number == tool.number)
            {
                document.setup_mut().tools.push(tool);
            }
        }
        if let Some(binding) = binding {
            document.geometry.bindings.insert(id, binding);
            document.cad_linked_operations.insert(id);
        } else {
            document.geometry.bindings.remove(&id);
            document.cad_linked_operations.remove(&id);
        }
        if let Some(slot) = document
            .setup_mut()
            .operations
            .iter_mut()
            .find(|o| o.id() == id)
        {
            *slot = operation;
        } else {
            document.setup_mut().operations.push(operation);
        }
        document.setup_mut().height_settings.insert(id, settings);
        if document.geometry.bindings.contains_key(&id) {
            crate::cad_operations::validate_binding_edit(&mut document, id)?;
        }
        let operation = document
            .setup()
            .operations
            .iter()
            .find(|o| o.id() == id)
            .unwrap();
        document.setup().height_settings[&id].resolve(&document, operation)?;
        let used: BTreeSet<_> = document
            .setup()
            .operations
            .iter()
            .flat_map(Operation::tools)
            .collect();
        document
            .setup_mut()
            .tools
            .retain(|tool| used.contains(&tool.number));
        self.edit_document(move |saved| *saved = document);
        self.pending_op = None;
        self.select(Selection::Operation(id));
        self.invalidate();
        Ok(id)
    }
}
