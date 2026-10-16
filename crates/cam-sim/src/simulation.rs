//! Incremental verification. Only complete steps enter the checkpoint; the final
//! (possibly interpolated) playback step runs on a disposable copy. Rewind and
//! edited prefixes restore an earlier checkpoint, never future removed stock.
use super::*;

#[derive(Clone, Debug, PartialEq)]
struct Config {
    min: [f64; 3],
    max: [f64; 3],
    options: SimOptions,
    tools: Vec<SimTool>,
}
#[derive(Clone)]
struct State {
    field: Heightfield,
    tool: ToolProfile,
    cur: Option<Point3>,
    collisions: Vec<Collision>,
}
impl State {
    fn new(config: &Config) -> Self {
        Self {
            field: Heightfield::new(
                [config.min[0], config.min[1]],
                [config.max[0], config.max[1]],
                config.options.resolution,
                config.max[2],
            ),
            tool: ToolProfile::flat(config.options.tool_radius),
            cur: None,
            collisions: Vec::new(),
        }
    }
    fn step(
        &mut self,
        step: &Step,
        tools: &[SimTool],
        default_tool: ToolProfile,
        cancelled: &dyn Fn() -> bool,
    ) -> bool {
        match step {
            Step::ToolChange { tool: number } => {
                self.tool = tools
                    .iter()
                    .find(|t| t.number == *number)
                    .map_or(default_tool, |t| t.profile);
            }
            Step::Rapid { to, .. } => {
                if let Some(a) = self.cur {
                    // A vertical lift (no XY travel, moving up) is always safe: the tool
                    // retreats straight up through the column it just cut, so it cannot
                    // plow through stock. The tool-radius disc below would otherwise catch
                    // a neighbouring ridge — e.g. the scallops a V/chamfer tool leaves
                    // between face passes — and false-flag the retract. (A *downward*
                    // in-place move is still checked: strategies plunge with a feed, not a
                    // rapid, so a descending rapid into stock is a real fault.) A lateral
                    // rapid sweeps the tool horizontally and is always checked.
                    let lateral = (a.x - to.x).hypot(a.y - to.y) > 1e-9;
                    let vertical_lift = !lateral && to.z >= a.z;
                    if !vertical_lift && a.z.min(to.z) < f64::from(self.field.top()) {
                        // Profile-aware: the tool is only as wide as its own shape at
                        // the depth it sits, so a pointed tool descending into the
                        // narrow groove it just cut is not stopped by the uncut
                        // surface alongside it. For a flat tool `offset` is zero
                        // everywhere, so this reduces exactly to the old radius test.
                        let tip_z = a.z.min(to.z);
                        if let Some((intrusion, stock)) = self.field.max_intrusion_along_controlled(
                            [a.x, a.y],
                            [to.x, to.y],
                            tip_z,
                            &self.tool,
                            cancelled,
                        ) {
                            if intrusion > CLEARANCE_EPS {
                                self.collisions.push(Collision {
                                    kind: CollisionKind::RapidThroughStock,
                                    at: [to.x, to.y, to.z],
                                    message: format!(
                                        "rapid at Z {:.3} passes through stock standing at Z {:.3}",
                                        to.z, stock
                                    ),
                                });
                            }
                        }
                    }
                }
                self.cur = Some(*to);
            }
            Step::Linear { to, .. } => {
                if let Some(a) = self.cur {
                    self.field.cut_segment_profile_controlled(
                        [a.x, a.y, a.z],
                        [to.x, to.y, to.z],
                        &self.tool,
                        cancelled,
                    );
                }
                self.cur = Some(*to);
            }
            Step::Arc {
                end, center, dir, ..
            } => {
                if let Some(a) = self.cur {
                    let pts = arc_points(a, *end, *center, *dir, self.field.resolution());
                    for w in pts.windows(2) {
                        if cancelled() {
                            return false;
                        }
                        self.field
                            .cut_segment_profile_controlled(w[0], w[1], &self.tool, cancelled);
                    }
                }
                self.cur = Some(*end);
            }
            Step::Drill(cycle) => {
                for &[x, y] in &cycle.points {
                    if cancelled() {
                        return false;
                    }
                    self.field.cut_segment_profile_controlled(
                        [x, y, cycle.depth],
                        [x, y, cycle.depth],
                        &self.tool,
                        cancelled,
                    );
                }
                self.cur = None;
            }
            _ => {}
        }
        !cancelled()
    }
}

/// Bounded reusable simulation state for forward playback and repeated operation
/// previews. Stock bounds, tool table and accuracy are part of the cache key.
#[derive(Default)]
pub struct SimulationCache {
    config: Option<Config>,
    steps: Vec<Step>,
    state: Option<State>,
    checkpoints: Vec<(usize, State)>,
}
impl SimulationCache {
    /// The same result and collision checks as a fresh simulation. Cancelled work
    /// cannot become a checkpoint or a published result.
    #[allow(clippy::too_many_arguments)] // Matches the stateless public API plus the cache receiver.
    pub fn simulate_controlled(
        &mut self,
        program: &Program,
        min: [f64; 3],
        max: [f64; 3],
        options: &SimOptions,
        tools: &[SimTool],
        cancelled: &dyn Fn() -> bool,
        mut progress: impl FnMut(usize, usize),
    ) -> Option<SimResult> {
        if cancelled() {
            return None;
        }
        let config = Config {
            min,
            max,
            options: *options,
            tools: tools.to_vec(),
        };
        if self.config.as_ref() != Some(&config) {
            *self = Self {
                config: Some(config.clone()),
                ..Self::default()
            };
        }
        let stable = program.steps().len().saturating_sub(1);
        let common = self
            .steps
            .iter()
            .zip(program.steps())
            .take_while(|(a, b)| a == b)
            .count()
            .min(stable);
        let reusable = self.state.is_some() && common == self.steps.len();
        let (mut state, start) = if reusable {
            (self.state.take().unwrap(), self.steps.len())
        } else {
            self.state = None;
            self.checkpoints.retain(|(count, _)| *count <= common);
            self.checkpoints.last().map_or_else(
                || (State::new(&config), 0),
                |(count, state)| (state.clone(), *count),
            )
        };
        let default_tool = ToolProfile::flat(options.tool_radius);
        for (index, step) in program.steps()[start..stable].iter().enumerate() {
            let index = start + index;
            if index % 128 == 0 {
                progress(index, program.steps().len());
            }
            if cancelled() || !state.step(step, tools, default_tool, cancelled) {
                // Earlier checkpoints are complete and can still be reused.
                return None;
            }
            let count = index + 1;
            if count % 256 == 0 {
                let checkpoint_limit =
                    (32 * 1024 * 1024 / state.field.storage_bytes().max(1)).min(4);
                if checkpoint_limit > 0 {
                    self.checkpoints.push((count, state.clone()));
                } else {
                    self.checkpoints.clear();
                }
                while self.checkpoints.len() > checkpoint_limit {
                    // Drop the most crowded interior sample, retaining broad
                    // coverage of the timeline and the most recent checkpoint.
                    let remove = (0..self.checkpoints.len() - 1)
                        .min_by_key(|&i| {
                            let previous = if i == 0 { 0 } else { self.checkpoints[i - 1].0 };
                            self.checkpoints[i + 1].0 - previous
                        })
                        .unwrap();
                    self.checkpoints.remove(remove);
                }
            }
        }
        self.steps = program.steps()[..stable].to_vec();
        self.state = Some(state.clone());
        if let Some(step) = program.steps().last() {
            if !state.step(step, tools, default_tool, cancelled) {
                return None;
            }
        }
        if cancelled() {
            return None;
        }
        progress(program.steps().len(), program.steps().len());
        let removed_volume = state.field.removed_volume();
        Some(SimResult {
            field: state.field,
            collisions: state.collisions,
            removed_volume,
        })
    }
}

/// Cancellable simulation. A stopped traversal has no complete result.
pub fn simulate_controlled(
    program: &Program,
    min: [f64; 3],
    max: [f64; 3],
    options: &SimOptions,
    tools: &[SimTool],
    cancelled: &dyn Fn() -> bool,
    progress: impl FnMut(usize, usize),
) -> Option<SimResult> {
    if cancelled() {
        return None;
    }
    let config = Config {
        min,
        max,
        options: *options,
        tools: tools.to_vec(),
    };
    let mut state = State::new(&config);
    let mut progress = progress;
    for (index, step) in program.steps().iter().enumerate() {
        if index % 128 == 0 {
            progress(index, program.steps().len());
        }
        if cancelled()
            || !state.step(
                step,
                tools,
                ToolProfile::flat(options.tool_radius),
                cancelled,
            )
        {
            return None;
        }
    }
    if cancelled() {
        return None;
    }
    progress(program.steps().len(), program.steps().len());
    let removed_volume = state.field.removed_volume();
    Some(SimResult {
        field: state.field,
        collisions: state.collisions,
        removed_volume,
    })
}
