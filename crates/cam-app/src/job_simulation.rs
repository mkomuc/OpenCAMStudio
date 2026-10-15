//! Cancellable stock verification and mesh preparation.
use super::*;
impl JobInputs {
    /// Simulate material removal for `program`: triangulate the remaining stock
    /// into render-ready vertices, indices and a machined-surface mask, and
    /// return any collisions found.
    /// Empty when there is no stock (no geometry loaded).
    pub(super) fn simulate_stock(
        &self,
        program: &Program,
        cancel: &CancelToken,
        progress: &Progress,
    ) -> (Vec<MeshVertex>, Vec<u32>, Vec<bool>, Vec<Collision>) {
        let (min, max) = (self.min, self.max);
        if min[0] >= max[0] || min[1] >= max[1] {
            return (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        }
        let setup_tools = &self.document.setup().tools;
        let tool_diameter = setup_tools
            .first()
            .map(|t| t.diameter)
            .unwrap_or(self.tool_diameter);
        // The sim's tool table: each setup tool as its cutting profile, selected by
        // the program's tool-change numbers.
        let sim_tools: Vec<SimTool> = setup_tools
            .iter()
            .map(|t| SimTool {
                number: t.number,
                profile: sim_profile(t),
            })
            .collect();
        let span = (max[0] - min[0]).max(max[1] - min[1]);
        let feature = narrowest_cut_width(program, setup_tools, max[2]);
        let resolution = simulation_resolution(
            span,
            feature,
            self.simulation_accuracy,
            self.preview_program.is_some(),
        );
        let Some(sim) = cam_sim::simulate_controlled(
            program,
            min,
            max,
            &SimOptions {
                resolution,
                tool_radius: tool_diameter / 2.0,
            },
            &sim_tools,
            &|| cancel.is_cancelled(),
            |done, total| progress.set(JobPhase::Verification, done, total, None),
        ) else {
            return (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        };
        progress.set(JobPhase::RenderPreparation, 0, 1, None);
        // Render the stock as a watertight solid down to its floor, so the full
        // thickness shows and cut walls shade crisply (not a smooth top sheet).
        let Some(surface) = sim
            .field
            .to_solid_mesh_controlled(min[2], &|| cancel.is_cancelled())
        else {
            return (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        };
        let mut vertices = mesh_vertices(&surface.positions, &surface.normals);
        for vertex in &mut vertices {
            if cancel.is_cancelled() {
                return (Vec::new(), Vec::new(), Vec::new(), Vec::new());
            }
            vertex.position = self
                .document
                .setup()
                .frame
                .to_world_vector(vertex.position.map(f64::from))
                .map(|n| n as f32);
            vertex.normal = self
                .document
                .setup()
                .frame
                .to_world_vector(vertex.normal.map(f64::from))
                .map(|n| n as f32);
        }
        (vertices, surface.indices, surface.machined, sim.collisions)
    }
}

/// Refine both the part-sized grid and narrow-feature sampling with the user's
/// quality setting. Playback uses a smaller grid; all settings retain the hard
/// cell budget so tiny grooves on large stock cannot exhaust memory.
fn simulation_resolution(span: f64, feature: f64, accuracy: f32, playback: bool) -> f64 {
    let accuracy = f64::from(crate::settings::clamp_simulation_accuracy(accuracy));
    let by_span = span / (if playback { 120.0 } else { 300.0 } * accuracy);
    let by_feature = feature / (3.0 * accuracy);
    let cells = if playback {
        240.0 * accuracy
    } else {
        MAX_SIM_CELLS_ACROSS
    };
    let budget = span / cells;
    by_span.min(by_feature).clamp(0.02, 2.0).max(budget)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accuracy_refines_stock_and_playback_within_the_cell_budget() {
        for playback in [false, true] {
            let coarse = simulation_resolution(100., 5., 0.5, playback);
            let normal = simulation_resolution(100., 5., 1., playback);
            let fine = simulation_resolution(100., 5., 2., playback);
            assert!(coarse > normal && normal > fine);
            let narrow = simulation_resolution(100., 0.001, 2., playback);
            assert!(100. / narrow <= MAX_SIM_CELLS_ACROSS);
            let large = simulation_resolution(10_000., 0.001, 2., playback);
            assert!(10_000. / large <= MAX_SIM_CELLS_ACROSS);
        }
    }
}
