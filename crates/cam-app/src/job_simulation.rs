//! Cancellable stock verification and mesh preparation.
use super::*;
impl JobInputs {
    /// Simulate material removal for `program`: triangulate the remaining stock
    /// into render-ready vertices + indices, and return any collisions found.
    /// Empty when there is no stock (no geometry loaded).
    pub(super) fn simulate_stock(
        &self,
        program: &Program,
        cancel: &CancelToken,
        progress: &Progress,
    ) -> (Vec<MeshVertex>, Vec<u32>, Vec<Collision>) {
        let (min, max) = (self.min, self.max);
        if min[0] >= max[0] || min[1] >= max[1] {
            return (Vec::new(), Vec::new(), Vec::new());
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
        // Aim for ~300 cells across the larger side, bounded so a big part stays
        // cheap and a small one stays crisp. (A modest, 2.5D-cosmetic bump from
        // 200 to sharpen curved-feature silhouettes; dial back if a rerun lags.)
        let span = (max[0] - min[0]).max(max[1] - min[1]);
        // …but a part-sized grid can be blind to a *narrow* feature: an engraved
        // V-groove is often under a millimetre wide, so at the span-derived cell size
        // it would simply not appear and the backplot would read as a failed
        // toolpath. Refine until the narrowest cut spans a few cells, capped by a
        // cell budget so refining never turns a preview into a hang.
        let feature = narrowest_cut_width(program, setup_tools, max[2]);
        let by_span = span / 300.0;
        let by_feature = feature / 3.0;
        let budget = span / MAX_SIM_CELLS_ACROSS;
        let resolution = by_span.min(by_feature).max(budget).clamp(0.02, 2.0);
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
            return (Vec::new(), Vec::new(), Vec::new());
        };
        progress.set(JobPhase::RenderPreparation, 0, 1, None);
        // Render the stock as a watertight solid down to its floor, so the full
        // thickness shows and cut walls shade crisply (not a smooth top sheet).
        let Some(surface) = sim
            .field
            .to_solid_mesh_controlled(min[2], &|| cancel.is_cancelled())
        else {
            return (Vec::new(), Vec::new(), Vec::new());
        };
        let mut vertices = mesh_vertices(&surface.positions, &surface.normals);
        for vertex in &mut vertices {
            if cancel.is_cancelled() {
                return (Vec::new(), Vec::new(), Vec::new());
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
        (vertices, surface.indices, sim.collisions)
    }
}
