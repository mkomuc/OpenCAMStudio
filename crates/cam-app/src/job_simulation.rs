//! Cancellable stock verification and mesh preparation.
use super::*;

#[derive(Default)]
pub(super) struct StockSimulation {
    pub vertices: Vec<MeshVertex>,
    pub indices: Vec<u32>,
    pub machined: Vec<bool>,
    pub operations: Vec<Option<u32>>,
    pub collisions: Vec<Collision>,
}
impl JobInputs {
    pub(crate) fn stock_resolution(&self, program: &Program) -> f64 {
        let span = (self.max[0] - self.min[0]).max(self.max[1] - self.min[1]);
        let setup = self.document.setup();
        let mut feature = narrowest_cut_width(program, &setup.tools, self.max[2]);
        for operation in &setup.operations {
            if let cam_model::Operation::Thread(thread) = operation {
                let included = program.steps().iter().any(|step| {
                    matches!(step,
                    cam_cldata::Step::Linear { tag, .. } | cam_cldata::Step::Arc { tag, .. }
                        if tag.op_id == thread.id)
                });
                if included {
                    feature = feature.min(thread.pitch);
                }
            }
        }
        simulation_resolution(span, feature, self.simulation_accuracy)
    }

    /// Simulate material removal for `program`: triangulate the remaining stock
    /// into render-ready vertices, indices and a machined-surface mask, and
    /// return any collisions found.
    /// Empty when there is no stock (no geometry loaded).
    pub(super) fn simulate_stock(
        &self,
        program: &Program,
        cancel: &CancelToken,
        progress: &Progress,
        simulation: &mut SimulationCache,
    ) -> StockSimulation {
        let (min, max) = (self.min, self.max);
        if min[0] >= max[0] || min[1] >= max[1] {
            return StockSimulation::default();
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
            .map(|t| {
                let outline = t.profile();
                SimTool {
                    number: t.number,
                    profile: sim_profile(t),
                    noncutting_tip_radius: outline
                        .segs
                        .first()
                        .filter(|s| !s.cutting && s.end.y.abs() < 1e-9)
                        .map(|s| s.end.x),
                }
            })
            .collect();
        let resolution = self
            .preview_resolution
            .unwrap_or_else(|| self.stock_resolution(program));
        let Some(sim) = simulation.simulate_controlled(
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
            return StockSimulation::default();
        };
        progress.set(JobPhase::RenderPreparation, 0, 1, None);
        // Render the stock as a watertight solid down to its floor, so the full
        // thickness shows and cut walls shade crisply (not a smooth top sheet).
        let Some(surface) = sim
            .field
            .to_solid_mesh_controlled(min[2], &|| cancel.is_cancelled())
        else {
            return StockSimulation::default();
        };
        let mut vertices = mesh_vertices(&surface.positions, &surface.normals);
        for vertex in &mut vertices {
            if cancel.is_cancelled() {
                return StockSimulation::default();
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
        StockSimulation {
            vertices,
            indices: surface.indices,
            machined: surface.machined,
            operations: surface.operations,
            collisions: sim.collisions,
        }
    }
}

/// Keep playback and verified previews on the same fine grid. Compact flat
/// surfaces and analytic contours avoid tying GPU memory to the grid cell count.
fn simulation_resolution(span: f64, feature: f64, accuracy: f32) -> f64 {
    let accuracy = f64::from(crate::settings::clamp_simulation_accuracy(accuracy));
    let by_span = span / (600.0 * accuracy);
    let by_feature = feature / (6.0 * accuracy);
    by_span
        .min(by_feature)
        .clamp(0.02, 2.0)
        .max(span / MAX_SIM_CELLS_ACROSS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accuracy_refines_stock_and_playback_within_the_cell_budget() {
        let coarse = simulation_resolution(100., 5., 0.5);
        let normal = simulation_resolution(100., 5., 1.);
        let fine = simulation_resolution(100., 5., 2.);
        assert!(coarse > normal && normal > fine);
        let narrow = simulation_resolution(100., 0.001, 2.);
        assert!(100. / narrow <= MAX_SIM_CELLS_ACROSS);
        let large = simulation_resolution(10_000., 0.001, 2.);
        assert!(10_000. / large <= MAX_SIM_CELLS_ACROSS);
    }
}
