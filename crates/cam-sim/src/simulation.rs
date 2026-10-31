//! Incremental verification. Only complete steps enter the checkpoint; the final
//! (possibly interpolated) playback step runs on a disposable copy. Rewind and
//! edited prefixes restore an earlier checkpoint, never future removed stock.
use super::*;
#[path = "removal.rs"]
mod removal;
pub use removal::{analyze_removal_controlled, RemovalSample};

#[derive(Clone, Debug, PartialEq)]
struct Config {
    min: [f64; 3],
    max: [f64; 3],
    options: SimOptions,
    tools: Vec<SimTool>,
    initial_key: Option<String>,
    surface_sources: bool,
}
#[derive(Clone)]
struct State {
    field: Heightfield,
    tool: ToolProfile,
    noncutting_tip_radius: Option<f64>,
    cur: Option<Point3>,
    collisions: Vec<Collision>,
}
impl State {
    fn new(config: &Config) -> Self {
        let mut field = Heightfield::new(
                [config.min[0], config.min[1]],
                [config.max[0], config.max[1]],
                config.options.resolution,
                config.max[2],
            );
        field.set_surface_tracking(config.surface_sources);
        Self {
            field,
            tool: ToolProfile::flat(config.options.tool_radius),
            noncutting_tip_radius: None,
            cur: None,
            collisions: Vec::new(),
        }
    }
    fn step(
        &mut self,
        step: &Step,
        step_index: usize,
        tools: &[SimTool],
        default_tool: ToolProfile,
        cancelled: &dyn Fn() -> bool,
    ) -> bool {
        match step {
            Step::ToolChange { tool: number } => {
                let tool = tools.iter().find(|t| t.number == *number);
                self.tool = tool.map_or(default_tool, |t| t.profile);
                self.noncutting_tip_radius = tool.and_then(|t| t.noncutting_tip_radius);
            }
            Step::Rapid { to, tag } | Step::Linear { to, tag, .. } => {
                if let Some(a) = self.cur {
                    self.segment(
                        [a.x, a.y, a.z],
                        [to.x, to.y, to.z],
                        tag,
                        matches!(step, Step::Rapid { .. }),
                        step_index,
                        (0., 1.),
                        cancelled,
                    );
                }
                self.cur = Some(*to);
            }
            Step::Arc {
                end,
                center,
                dir,
                tag,
                ..
            } => {
                if let Some(a) = self.cur {
                    let pts = arc_points(a, *end, *center, *dir, self.field.resolution());
                    let count = (pts.len() - 1) as f64;
                    for (index, w) in pts.windows(2).enumerate() {
                        if cancelled() {
                            return false;
                        }
                        if self.segment(
                            w[0],
                            w[1],
                            tag,
                            false,
                            step_index,
                            (index as f64 / count, (index + 1) as f64 / count),
                            cancelled,
                        ) {
                            break;
                        }
                    }
                }
                self.cur = Some(*end);
            }
            Step::Drill(cycle) => {
                for &[x, y] in &cycle.points {
                    if cancelled() {
                        return false;
                    }
                    let retract = [x, y, cycle.retract];
                    let top = [x, y, cycle.z_top];
                    let link = cam_cldata::Tag {
                        kind: cam_cldata::MoveKind::Link,
                        ..cycle.tag
                    };
                    if let Some(a) = self.cur {
                        self.segment(
                            [a.x, a.y, a.z],
                            retract,
                            &link,
                            true,
                            step_index,
                            (0., 1.),
                            cancelled,
                        );
                    }
                    self.segment(retract, top, &link, true, step_index, (0., 1.), cancelled);
                    self.segment(
                        top,
                        [x, y, cycle.depth],
                        &cam_cldata::Tag {
                            kind: cam_cldata::MoveKind::Plunge,
                            ..cycle.tag
                        },
                        false,
                        step_index,
                        (0., 1.),
                        cancelled,
                    );
                    // Peck returns follow the already cut hole. Retain the
                    // final retract pose so later links are checked as well.
                    self.cur = Some(Point3::new(x, y, cycle.retract));
                }
            }
            _ => {}
        }
        !cancelled()
    }
    /// Returns true on collision. Non-cutting motion never clears stock.
    #[allow(clippy::too_many_arguments)]
    fn segment(
        &mut self,
        a: [f64; 3],
        b: [f64; 3],
        tag: &cam_cldata::Tag,
        rapid: bool,
        index: usize,
        range: (f64, f64),
        cancelled: &dyn Fn() -> bool,
    ) -> bool {
        use cam_cldata::MoveKind;
        let reposition = matches!(
            tag.kind,
            MoveKind::Link | MoveKind::Retract | MoveKind::Traverse
        );
        let axial = (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-9;
        let lifting = axial && b[2] >= a[2];
        let probe = if rapid || reposition {
            (!lifting).then_some(self.tool)
        } else if b[2] < a[2] - 1e-9 {
            self.noncutting_tip_radius
                .filter(|r| *r > 1e-9)
                .map(|radius| {
                    // A flat-envelope cutter without a cutting tip (thread mill)
                    // needs clearance for its entire diameter on a straight plunge.
                    // Checking only its neck would miss a core surrounding a small
                    // pilot hole. Side-cutting helices still use the tip-only probe.
                    if axial
                        && matches!(
                            self.tool.shape,
                            ProfileShape::Flat | ProfileShape::Thread { .. }
                        )
                    {
                        self.tool
                    } else {
                        ToolProfile::flat(radius)
                    }
                })
        } else {
            None
        };
        if let Some(tool) = probe {
            if let Some((fraction, at, stock)) =
                collision::first_contact(&self.field, a, b, &tool, cancelled)
            {
                let kind = if rapid {
                    CollisionKind::RapidThroughStock
                } else {
                    CollisionKind::NonCuttingThroughStock
                };
                let motion = if rapid {
                    "rapid"
                } else if reposition {
                    "non-cutting feed"
                } else if axial
                    && matches!(
                        self.tool.shape,
                        ProfileShape::Flat | ProfileShape::Thread { .. }
                    )
                {
                    "non-plunging cutter"
                } else {
                    "non-cutting tool tip"
                };
                self.collisions.push(Collision {
                    kind,
                    at,
                    message: format!(
                        "{motion} at Z {:.3} enters stock standing at Z {stock:.3}",
                        at[2]
                    ),
                    op_id: Some(tag.op_id),
                    pattern_id: tag.pattern_id,
                    instance_index: tag.instance_index,
                    step_index: Some(index),
                    fraction: range.0 + fraction * (range.1 - range.0),
                });
                return true;
            }
        }
        if !rapid && !reposition {
            self.field.set_cut_source(index, range);
            self.field
                .cut_segment_profile_controlled(a, b, &self.tool, Some(tag.op_id), cancelled);
        }
        false
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
    surface_sources: bool,
}
impl SimulationCache {
    /// Surface attribution is opt-in; changing it invalidates prefix checkpoints.
    pub fn set_surface_sources(&mut self, enabled: bool) {
        self.surface_sources = enabled;
    }
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
        progress: impl FnMut(usize, usize),
    ) -> Option<SimResult> {
        self.simulate_seeded(program, min, max, options, tools, None, cancelled, progress)
    }

    /// Continue machining an already processed blank. The caller's key must
    /// identify all inputs that produced `initial`, including its grid and axes.
    #[allow(clippy::too_many_arguments)]
    pub fn simulate_from_stock_controlled(
        &mut self,
        program: &Program,
        min: [f64; 3],
        max: [f64; 3],
        options: &SimOptions,
        tools: &[SimTool],
        initial: &Heightfield,
        key: &str,
        cancelled: &dyn Fn() -> bool,
        progress: impl FnMut(usize, usize),
    ) -> Option<SimResult> {
        self.simulate_seeded(
            program,
            min,
            max,
            options,
            tools,
            Some((key, initial)),
            cancelled,
            progress,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn simulate_seeded(
        &mut self,
        program: &Program,
        min: [f64; 3],
        max: [f64; 3],
        options: &SimOptions,
        tools: &[SimTool],
        initial: Option<(&str, &Heightfield)>,
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
            initial_key: initial.map(|(key, _)| key.to_owned()),
            surface_sources: self.surface_sources,
        };
        if self.config.as_ref() != Some(&config) {
            *self = Self {
                config: Some(config.clone()),
                surface_sources: self.surface_sources,
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
                || {
                    let mut state = State::new(&config);
                    if let Some((_, field)) = initial {
                        state.field = field.clone();
                        state.field.set_surface_tracking(config.surface_sources);
                    }
                    (state, 0)
                },
                |(count, state)| (state.clone(), *count),
            )
        };
        let default_tool = ToolProfile::flat(options.tool_radius);
        for (index, step) in program.steps()[start..stable].iter().enumerate() {
            let index = start + index;
            if index % 128 == 0 {
                progress(index, program.steps().len());
            }
            if cancelled() || !state.step(step, index, tools, default_tool, cancelled) {
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
            if !state.step(step, stable, tools, default_tool, cancelled) {
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
        initial_key: None,
        surface_sources: false,
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
                index,
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
