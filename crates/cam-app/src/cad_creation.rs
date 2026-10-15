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
}
