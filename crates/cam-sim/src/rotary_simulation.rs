//! Four-axis verification in a stationary material frame. Each dexel retains
//! disjoint Z intervals; indexing never rotates/resamples the remaining solid.
//! Cutter poses are spaced by <= resolution/4 of relative material travel.
//! The same physical cutter is used for removal and contact at each pose, so
//! retracing a cleared surface does not collide with an inflated cutting floor.
//! Accuracy is limited by the stock grid and the pose spacing.
use crate::*;
use cam_cldata::{MoveKind, RotaryAxis, Tag};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RotaryTool {
    /// Tool identity, cutting profile and any non-cutting tip.
    pub tool: SimTool,
    /// Upper end of the cutting profile, measured from the tool tip, mm.
    pub flute_length: f64,
    /// Complete tool stickout from the tip, mm.
    pub length: f64,
    /// Upper end of the reduced neck, measured from the tip, mm.
    pub neck_length: f64,
    pub neck_radius: f64,
    pub shank_radius: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Frame {
    axis: RotaryAxis,
    center: Point3,
}
impl Frame {
    fn material(self, p: Point3, angle: f64) -> Point3 {
        self.axis.material_point(p, self.center, angle)
    }
    fn machine(self, p: Point3, rotation: (f64, f64)) -> Point3 {
        let (s, c) = rotation;
        let (x, y, z) = (
            p.x - self.center.x,
            p.y - self.center.y,
            p.z - self.center.z,
        );
        match self.axis {
            RotaryAxis::A => Point3::new(
                p.x,
                self.center.y + y * c + z * s,
                self.center.z - y * s + z * c,
            ),
            RotaryAxis::B => Point3::new(
                self.center.x + x * c - z * s,
                p.y,
                self.center.z + x * s + z * c,
            ),
        }
    }
}
fn xyz(p: Point3) -> [f64; 3] {
    [p.x, p.y, p.z]
}
fn lerp(a: Point3, b: Point3, t: f64) -> Point3 {
    Point3::new(
        a.x + (b.x - a.x) * t,
        a.y + (b.y - a.y) * t,
        a.z + (b.z - a.z) * t,
    )
}
fn distance(a: Point3, b: Point3) -> f64 {
    (a.x - b.x).hypot(a.y - b.y).hypot(a.z - b.z)
}

fn clip_tool_axis(
    a: Point3,
    b: Point3,
    min: [f64; 3],
    max: [f64; 3],
    radius: f64,
) -> Option<(Point3, Point3)> {
    let (mut lo, mut hi) = (0_f64, 1_f64);
    let (p, q) = (xyz(a), xyz(b));
    for i in 0..3 {
        let d = q[i] - p[i];
        let (min, max) = (min[i] - radius, max[i] + radius);
        if d.abs() < 1e-14 {
            if p[i] < min || p[i] > max {
                return None;
            }
        } else {
            let (u, v) = ((min - p[i]) / d, (max - p[i]) / d);
            lo = lo.max(u.min(v));
            hi = hi.min(u.max(v));
            if lo > hi {
                return None;
            }
        }
    }
    Some((lerp(a, b, lo), lerp(a, b, hi)))
}

/// Clip a tool-axis capsule to the enclosing cylinder of the original blank.
/// This only narrows the candidate window; exact cutter/ray tests stay unchanged.
fn clip_cylinder_axis(
    a: Point3,
    b: Point3,
    axis: usize,
    center: [f64; 3],
    radius: f64,
) -> Option<(Point3, Point3)> {
    let p = xyz(a);
    let q = xyz(b);
    let u = (axis + 1) % 3;
    let v = (axis + 2) % 3;
    let (x, y) = (p[u] - center[u], p[v] - center[v]);
    let (dx, dy) = (q[u] - p[u], q[v] - p[v]);
    let aa = dx * dx + dy * dy;
    let bb = x * dx + y * dy;
    // Round outward at tangencies, never inward into a material column.
    let radius = radius + 1e-9 * radius.max(1.);
    let cc = x * x + y * y - radius * radius;
    if aa < 1e-20 {
        return (cc <= 0.).then_some((a, b));
    }
    let disc = bb * bb - aa * cc;
    if disc < 0. {
        return None;
    }
    let h = disc.sqrt();
    let lo = ((-bb - h) / aa).max(0.);
    let hi = ((-bb + h) / aa).min(1.);
    (lo <= hi).then(|| (lerp(a, b, lo), lerp(a, b, hi)))
}

#[derive(Clone, Debug, PartialEq)]
struct Config {
    min: [f64; 3],
    max: [f64; 3],
    options: SimOptions,
    tools: Vec<RotaryTool>,
    initial_key: Option<String>,
    frame: Frame,
    sources: bool,
    volume_tracking: bool,
    #[cfg(test)]
    tight_bounds: bool,
}
#[derive(Clone)]
struct State {
    field: Heightfield,
    current: Option<Point3>,
    angle: f64,
    tool: usize,
    collisions: Vec<Collision>,
    rpm: f64,
    poses: usize,
    block_length: Option<f64>,
    last_tag: Option<Tag>,
}

/// Bounded stock checkpoints let edits and reordered operations reuse their
/// unchanged verified prefix. The interpolated final step is disposable.
#[derive(Default)]
pub struct RotarySimulationCache {
    config: Option<Config>,
    steps: Vec<Step>,
    state: Option<State>,
    checkpoints: Vec<(usize, State)>,
    without_volume_tracking: bool,
}
impl RotarySimulationCache {
    /// Disable unused volume accounting for geometry-only consumers. Enabled by
    /// default; when disabled, `SimResult::removed_volume` is zero. Changing this
    /// setting invalidates incompatible checkpoints on the next simulation.
    pub fn set_volume_tracking(&mut self, enabled: bool) {
        self.without_volume_tracking = !enabled;
    }

    #[allow(clippy::too_many_arguments)]
    pub fn simulate_controlled(
        &mut self,
        program: &Program,
        min: [f64; 3],
        max: [f64; 3],
        options: &SimOptions,
        tools: &[RotaryTool],
        initial: Option<(&str, &Heightfield)>,
        surface_sources: bool,
        cancelled: &dyn Fn() -> bool,
        mut progress: impl FnMut(usize, usize),
    ) -> Result<SimResult, String> {
        if cancelled() {
            return Err("Stock simulation cancelled".into());
        }
        if initial.is_some_and(|(_, f)| !f.matches_grid(min, max, options.resolution)) {
            return Err("Initial rotary stock must use the requested bounds and resolution".into());
        }
        let mut config = config(
            program,
            min,
            max,
            options,
            tools,
            initial.map(|v| v.0),
            surface_sources,
        )?;
        config.volume_tracking = !self.without_volume_tracking;
        if cancelled() {
            return Err("Stock simulation cancelled".into());
        }
        let stable = program.steps().len().saturating_sub(1);
        if self.config.as_ref() != Some(&config) {
            self.steps.clear();
            self.state = None;
            self.checkpoints.clear();
        }
        let common = self
            .steps
            .iter()
            .zip(program.steps())
            .take_while(|(a, b)| a == b)
            .count()
            .min(stable);
        let reusable = self.state.is_some() && common == self.steps.len();
        self.checkpoints.retain(|(count, _)| *count <= common);
        let (mut state, start) = if reusable {
            (self.state.as_ref().unwrap().clone(), self.steps.len())
        } else if let Some((count, state)) = self.checkpoints.last() {
            (state.clone(), *count)
        } else {
            (State::new(&config, initial.map(|v| v.1)), 0)
        };
        // Only commit new checkpoints after the traversal completes. Cancellation
        // keeps previously verified checkpoints usable by a replacement job.
        let mut checkpoints = self.checkpoints.clone();
        for index in start..stable {
            progress(index, program.steps().len());
            state.step(
                &program.steps()[index],
                index,
                &config,
                cancelled,
                &mut None,
            )?;
            let current = program.steps()[index].tag().map(|tag| tag.op_id);
            let next = program
                .steps()
                .get(index + 1)
                .and_then(Step::tag)
                .map(|tag| tag.op_id);
            if current.is_some() && current != next {
                let limit = (64 * 1024 * 1024 / state.field.storage_bytes().max(1)).clamp(1, 8);
                checkpoints.push((index + 1, state.clone()));
                while checkpoints.len() > limit {
                    let remove = if checkpoints.len() <= 2 {
                        0
                    } else {
                        (0..checkpoints.len() - 1)
                            .min_by_key(|&i| {
                                let previous = if i == 0 { 0 } else { checkpoints[i - 1].0 };
                                checkpoints[i + 1].0 - previous
                            })
                            .unwrap()
                    };
                    checkpoints.remove(remove);
                }
            }
        }
        if cancelled() {
            return Err("Stock simulation cancelled".into());
        }
        self.steps = program.steps()[..stable].to_vec();
        self.config = Some(config.clone());
        self.state = Some(state.clone());
        self.checkpoints = checkpoints;
        if let Some(step) = program.steps().last() {
            state.step(step, stable, &config, cancelled, &mut None)?;
        }
        if cancelled() {
            return Err("Stock simulation cancelled".into());
        }
        progress(program.steps().len(), program.steps().len());
        Ok(SimResult {
            removed_volume: state.field.tracked_removed_volume(),
            field: state.field,
            collisions: state.collisions,
        })
    }
}

#[allow(clippy::too_many_arguments)]
pub fn analyze_rotary_removal_controlled(
    program: &Program,
    min: [f64; 3],
    max: [f64; 3],
    options: &SimOptions,
    tools: &[RotaryTool],
    initial: Option<&Heightfield>,
    cancelled: &dyn Fn() -> bool,
    mut progress: impl FnMut(usize, usize),
) -> Result<Vec<RemovalSample>, String> {
    if cancelled() {
        return Err("Stock simulation cancelled".into());
    }
    if initial.is_some_and(|f| !f.matches_grid(min, max, options.resolution)) {
        return Err("Initial rotary stock must use the requested bounds and resolution".into());
    }
    let config = config(program, min, max, options, tools, None, false)?;
    let mut state = State::new(&config, initial);
    let mut samples = Some(Vec::new());
    for (index, step) in program.steps().iter().enumerate() {
        progress(index, program.steps().len());
        state.step(step, index, &config, cancelled, &mut samples)?;
    }
    progress(program.steps().len(), program.steps().len());
    Ok(samples.unwrap())
}

#[allow(clippy::too_many_arguments)]
fn config(
    program: &Program,
    min: [f64; 3],
    max: [f64; 3],
    options: &SimOptions,
    tools: &[RotaryTool],
    key: Option<&str>,
    sources: bool,
) -> Result<Config, String> {
    program
        .validate()
        .map_err(|e| format!("Invalid rotary simulation program: {e}"))?;
    if !options.resolution.is_finite()
        || options.resolution <= 0.
        || (0..3).any(|i| !min[i].is_finite() || !max[i].is_finite() || min[i] >= max[i])
    {
        return Err(
            "Rotary simulation requires finite stock bounds and positive resolution".into(),
        );
    }
    if ((max[0] - min[0]) / options.resolution).ceil()
        * ((max[1] - min[1]) / options.resolution).ceil()
        > 4_000_000.
    {
        return Err("Rotary stock exceeds 4,000,000 columns; lower simulation accuracy".into());
    }
    if tools.is_empty() {
        return Err("Rotary verification requires a defined tool".into());
    }
    for t in tools {
        if [
            t.tool.profile.radius,
            t.flute_length,
            t.length,
            t.neck_radius,
            t.shank_radius,
        ]
        .iter()
        .any(|v| !v.is_finite() || *v <= 0.)
            || !t.neck_length.is_finite()
            || t.neck_length < 0.
            || t.flute_length > t.length
            || t.neck_length > t.length
        {
            return Err(format!(
                "Invalid rotary verification dimensions for tool {}",
                t.tool.number
            ));
        }
    }
    let mut frame = None;
    for step in program.steps() {
        if let Step::Rotary { axis, center, .. } = step {
            let next = Frame {
                axis: *axis,
                center: *center,
            };
            if frame.is_some_and(|f: Frame| {
                f.axis != next.axis
                    || match f.axis {
                        RotaryAxis::A => {
                            (f.center.y - next.center.y).hypot(f.center.z - next.center.z) > 1e-6
                        }
                        RotaryAxis::B => {
                            (f.center.x - next.center.x).hypot(f.center.z - next.center.z) > 1e-6
                        }
                    }
            }) {
                return Err(
                    "Rotary simulation requires one physical rotary axis and centre per setup"
                        .into(),
                );
            }
            frame = Some(next);
        }
        if matches!(step,Step::CutterComp(value) if *value!=cam_cldata::CutterComp::Off) {
            return Err("Rotary stock verification requires computed cutter compensation".into());
        }
    }
    Ok(Config {
        min,
        max,
        options: *options,
        tools: tools.to_vec(),
        initial_key: key.map(str::to_owned),
        frame: frame.unwrap_or(Frame {
            axis: RotaryAxis::A,
            center: Point3::new(0., 0., 0.),
        }),
        sources,
        volume_tracking: true,
        #[cfg(test)]
        tight_bounds: true,
    })
}

impl State {
    fn new(c: &Config, initial: Option<&Heightfield>) -> Self {
        let mut field = initial.cloned().unwrap_or_else(|| {
            Heightfield::new(
                [c.min[0], c.min[1]],
                [c.max[0], c.max[1]],
                c.options.resolution,
                c.max[2],
            )
        });
        field.set_surface_tracking(c.sources);
        if c.volume_tracking {
            field.start_removal_tracking(c.min[2]);
        } else {
            field.stop_removal_tracking();
        }
        Self {
            field,
            current: None,
            angle: 0.,
            tool: 0,
            collisions: Vec::new(),
            rpm: 0.,
            poses: 0,
            block_length: None,
            last_tag: None,
        }
    }
    fn step(
        &mut self,
        step: &Step,
        index: usize,
        c: &Config,
        cancelled: &dyn Fn() -> bool,
        samples: &mut Option<Vec<RemovalSample>>,
    ) -> Result<(), String> {
        if cancelled() {
            return Err("Stock simulation cancelled".into());
        }
        if let Some(tag) = step.tag() {
            self.last_tag = Some(tag);
        }
        match step {
            Step::ToolChange { tool } => {
                self.tool = c
                    .tools
                    .iter()
                    .position(|t| t.tool.number == *tool)
                    .ok_or_else(|| format!("Missing rotary simulation tool {tool}"))?;
                if let (Some(p), Some(tag)) = (self.current, self.last_tag) {
                    self.motion(
                        p,
                        p,
                        self.angle,
                        self.angle,
                        None,
                        Tag {
                            kind: MoveKind::Traverse,
                            ..tag
                        },
                        index,
                        (0., 1.),
                        c,
                        cancelled,
                        samples,
                    )?;
                }
            }
            Step::Spindle { rpm, .. } => self.rpm = *rpm,
            Step::SpindleOff => self.rpm = 0.,
            Step::Datum(_) => self.current = None,
            Step::Rotary {
                to,
                angle,
                feed,
                tag,
                length,
                ..
            } => {
                self.block_length = Some(*length);
                self.motion(
                    self.current.unwrap_or(*to),
                    *to,
                    self.angle,
                    *angle,
                    *feed,
                    *tag,
                    index,
                    (0., 1.),
                    c,
                    cancelled,
                    samples,
                )?;
                self.current = Some(*to);
                self.angle = *angle;
            }
            Step::Rapid { to, tag } | Step::Linear { to, tag, .. } => {
                let feed = if let Step::Linear { feed, .. } = step {
                    Some(*feed)
                } else {
                    None
                };
                self.motion(
                    self.current.unwrap_or(*to),
                    *to,
                    self.angle,
                    self.angle,
                    feed,
                    *tag,
                    index,
                    (0., 1.),
                    c,
                    cancelled,
                    samples,
                )?;
                self.current = Some(*to);
            }
            Step::Arc {
                end,
                center,
                dir,
                feed,
                tag,
            } => {
                if let Some(start) = self.current {
                    let pts = crate::arc_points(start, *end, *center, *dir, c.options.resolution);
                    let n = pts.len() - 1;
                    for (i, w) in pts.windows(2).enumerate() {
                        self.motion(
                            Point3::new(w[0][0], w[0][1], w[0][2]),
                            Point3::new(w[1][0], w[1][1], w[1][2]),
                            self.angle,
                            self.angle,
                            Some(*feed),
                            *tag,
                            index,
                            (i as f64 / n as f64, (i + 1) as f64 / n as f64),
                            c,
                            cancelled,
                            samples,
                        )?;
                    }
                }
                self.current = Some(*end);
            }
            Step::Drill(cycle) => {
                for &[x, y] in &cycle.points {
                    let retract = Point3::new(x, y, cycle.retract);
                    let top = Point3::new(x, y, cycle.z_top);
                    let bottom = Point3::new(x, y, cycle.depth);
                    let link = Tag {
                        kind: MoveKind::Link,
                        ..cycle.tag
                    };
                    self.motion(
                        self.current.unwrap_or(retract),
                        retract,
                        self.angle,
                        self.angle,
                        None,
                        link,
                        index,
                        (0., 1.),
                        c,
                        cancelled,
                        samples,
                    )?;
                    self.motion(
                        retract,
                        top,
                        self.angle,
                        self.angle,
                        None,
                        link,
                        index,
                        (0., 1.),
                        c,
                        cancelled,
                        samples,
                    )?;
                    self.motion(
                        top,
                        bottom,
                        self.angle,
                        self.angle,
                        Some(cycle.feed),
                        Tag {
                            kind: MoveKind::Plunge,
                            ..cycle.tag
                        },
                        index,
                        (0., 1.),
                        c,
                        cancelled,
                        samples,
                    )?;
                    self.current = Some(retract);
                }
            }
            _ => {}
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn motion(
        &mut self,
        a: Point3,
        b: Point3,
        aa: f64,
        ba: f64,
        feed: Option<f64>,
        tag: Tag,
        index: usize,
        range: (f64, f64),
        c: &Config,
        cancelled: &dyn Fn() -> bool,
        samples: &mut Option<Vec<RemovalSample>>,
    ) -> Result<(), String> {
        let tool = c.tools[self.tool];
        let path_length = self.block_length.take().unwrap_or_else(|| distance(a, b));
        // Rotation only moves points perpendicular to the axis. Including the
        // blank's axial length oversamples long shafts without improving the
        // spatial error bound.
        let radial_axis = if c.frame.axis == RotaryAxis::A { 1 } else { 0 };
        let center = xyz(c.frame.center);
        let extent = |i: usize| {
            (c.min[i] - center[i])
                .abs()
                .max((c.max[i] - center[i]).abs())
        };
        let reach = extent(radial_axis).hypot(extent(2));
        let travel = distance(a, b) + (ba - aa).to_radians().abs() * reach;
        let count = (travel / (c.options.resolution * 0.25)).ceil().max(1.) as usize;
        if count > 1_000_000 || self.poses.saturating_add(count) > 5_000_000 {
            return Err("Rotary verification exceeds the pose budget; lower simulation accuracy or shorten the run".into());
        }
        self.poses += count;
        let reposition = feed.is_none()
            || matches!(
                tag.kind,
                MoveKind::Link | MoveKind::Retract | MoveKind::Traverse
            );
        let lifting = (aa - ba).abs() < 1e-12 && (a.x - b.x).hypot(a.y - b.y) < 1e-9 && b.z > a.z;
        let neck_end = tool.neck_length.max(tool.flute_length);
        let mut probes = Vec::with_capacity(3);
        if reposition && !lifting {
            probes.push((
                if matches!(tool.tool.profile.shape, ProfileShape::Thread { .. }) {
                    ToolProfile::flat(tool.tool.profile.radius)
                } else {
                    tool.tool.profile
                },
                0.,
                tool.flute_length,
            ));
        }
        if !reposition {
            if let ProfileShape::Thread { root_radius, .. } = tool.tool.profile.shape {
                probes.push((ToolProfile::flat(root_radius), 0., tool.flute_length));
            }
        }
        if !reposition && b.z < a.z - 1e-9 {
            if let Some(r) = tool.tool.noncutting_tip_radius.filter(|r| *r > 1e-9) {
                let r = if (a.x - b.x).hypot(a.y - b.y) < 1e-9
                    && matches!(
                        tool.tool.profile.shape,
                        ProfileShape::Flat | ProfileShape::Thread { .. }
                    ) {
                    tool.tool.profile.radius
                } else {
                    r
                };
                probes.push((ToolProfile::flat(r), 0., 0.01));
            }
        }
        if neck_end > tool.flute_length {
            probes.push((
                ToolProfile::flat(tool.neck_radius),
                tool.flute_length,
                neck_end,
            ));
        }
        if tool.length > neck_end {
            probes.push((ToolProfile::flat(tool.shank_radius), neck_end, tool.length));
        }
        // A body section wholly outside the stock's enclosing cylinder cannot
        // collide at any index angle. In particular, long shanks above a small
        // round blank need no ray intersection at every sampled stock column.
        let radial_clear = |(profile, lo, hi): &(ToolProfile, f64, f64)| {
            let lateral = |p: Point3| if radial_axis == 0 { p.x } else { p.y };
            let distance_to_interval = |lo: f64, hi: f64| lo.max(0.).max(-hi);
            let lateral_distance = distance_to_interval(
                lateral(a).min(lateral(b)) - profile.radius - center[radial_axis],
                lateral(a).max(lateral(b)) + profile.radius - center[radial_axis],
            );
            let height_distance =
                distance_to_interval(a.z.min(b.z) + lo - center[2], a.z.max(b.z) + hi - center[2]);
            lateral_distance.hypot(height_distance) > reach + 1e-6
        };
        #[cfg(not(test))]
        probes.retain(|probe| !radial_clear(probe));
        #[cfg(test)]
        if c.tight_bounds {
            probes.retain(|probe| !radial_clear(probe));
        }
        let mut contact = false;
        let mut sample_before = self.field.tracked_removed_volume();
        let cylinder = self.field.cylindrical_envelope();
        for j in 0..=count {
            if cancelled() {
                return Err("Stock simulation cancelled".into());
            }
            let t = j as f64 / count as f64;
            let tip = lerp(a, b, t);
            let angle = aa + (ba - aa) * t;
            let rotation = (-angle).to_radians().sin_cos();
            let ray_direction = match c.frame.axis {
                RotaryAxis::A => [0., rotation.0, rotation.1],
                RotaryAxis::B => [-rotation.0, 0., rotation.1],
            };
            let fraction = range.0 + (range.1 - range.0) * t;
            let material_tip = c.frame.material(tip, angle);
            let mut source = None;
            // Bound the search window between neighbouring poses as well.
            let pad = travel / count as f64 * 0.5;
            let body_radius = tool
                .tool
                .profile
                .radius
                .max(tool.shank_radius)
                .max(tool.neck_radius);
            let end = c
                .frame
                .material(Point3::new(tip.x, tip.y, tip.z + tool.length), angle);
            // Clip the tool axis to an expanded stock box before projecting it.
            // The capsule encloses every flute/neck/shank section. This rejects
            // air poses and avoids scanning the long stickout outside the blank.
            let bounds = clip_tool_axis(material_tip, end, c.min, c.max, body_radius + pad).and_then(
                |(a, b)| match cylinder {
                    Some((axis, center, radius)) => {
                        clip_cylinder_axis(a, b, axis, center, radius + body_radius + pad)
                    }
                    None => Some((a, b)),
                },
            );
            #[cfg(test)]
            let bounds = if c.tight_bounds {
                bounds
            } else {
                Some((material_tip, end))
            };
            if let Some((start, end)) = bounds {
                let (x0, x1) = self.field.index_range(
                    start.x.min(end.x) - body_radius - pad,
                    start.x.max(end.x) + body_radius + pad,
                    0,
                );
                let (y0, y1) = self.field.index_range(
                    start.y.min(end.y) - body_radius - pad,
                    start.y.max(end.y) + body_radius + pad,
                    1,
                );
                for y in y0..=y1 {
                    if cancelled() {
                        return Err("Stock simulation cancelled".into());
                    }
                    for x in x0..=x1 {
                        let (cx, cy) = self.field.center(x, y);
                        let column = y * self.field.dims().0 + x;
                        let floor = c.min[2];
                        let ceiling = self.field.column_top(column);
                        if ceiling <= floor + 1e-6 {
                            continue;
                        }
                        let p = c.frame.machine(Point3::new(cx, cy, 0.), rotation);
                        let q = [p.x - tip.x, p.y - tip.y, p.z - tip.z];
                        let v = ray_direction;
                        if !contact {
                            for &(profile, lo, hi) in &probes {
                                if let Some((lo, hi)) =
                                    ray_tool(q, v, floor, ceiling, profile, lo, hi, 0.)
                                {
                                    if self.field.interval_contact(column, lo, hi, floor) {
                                        self.collisions.push(Collision{kind:if feed.is_none(){CollisionKind::RapidThroughStock}else{CollisionKind::NonCuttingThroughStock},at:xyz(material_tip),message:format!("{} enters remaining rotary stock (resolution {:.3} mm)",if reposition{"Non-cutting motion"}else{"Non-cutting tool body"},c.options.resolution),op_id:Some(tag.op_id),pattern_id:tag.pattern_id,instance_index:tag.instance_index,step_index:Some(index),fraction});
                                        contact = true;
                                        break;
                                    }
                                }
                            }
                        }
                        if !reposition && !contact {
                            if matches!(tool.tool.profile.shape, ProfileShape::Thread { .. }) {
                                for (lo, hi) in
                                    thread_intervals(q, v, floor, ceiling, tool.tool.profile)
                                {
                                    if self.field.interval_material(column, lo, hi, floor, 0.) {
                                        let source = *source.get_or_insert_with(|| {
                                            self.field.rotary_source(
                                                xyz(material_tip),
                                                tool.tool.profile,
                                                index,
                                                fraction,
                                            )
                                        });
                                        self.field.subtract_interval(
                                            column,
                                            lo,
                                            hi,
                                            Some(tag.op_id),
                                            source,
                                        );
                                    }
                                }
                            } else if let Some((lo, hi)) = ray_tool(
                                q,
                                v,
                                floor,
                                ceiling,
                                tool.tool.profile,
                                0.,
                                tool.flute_length,
                                0.,
                            ) {
                                if self.field.interval_material(column, lo, hi, floor, 0.) {
                                    let source = *source.get_or_insert_with(|| {
                                        self.field.rotary_source(
                                            xyz(material_tip),
                                            tool.tool.profile,
                                            index,
                                            fraction,
                                        )
                                    });
                                    self.field.subtract_interval(
                                        column,
                                        lo,
                                        hi,
                                        Some(tag.op_id),
                                        source,
                                    );
                                }
                            }
                        }
                    }
                }
            }
            if let (Some(out), Some(feed)) = (samples.as_mut(), feed) {
                if j > 0 && path_length > 1e-12 {
                    if out.len() >= 500_000 {
                        return Err("Rotary force analysis exceeds 500000 samples; lower simulation accuracy".into());
                    }
                    out.push(RemovalSample {
                        step_index: index,
                        start_fraction: range.0
                            + (range.1 - range.0) * (j - 1) as f64 / count as f64,
                        end_fraction: fraction,
                        seconds: path_length / feed * 60. / count as f64,
                        removed_volume_mm3: self.field.tracked_removed_volume() - sample_before,
                        tool: Some(tool.tool.number),
                        spindle_rpm: self.rpm,
                        collision: !self.collisions.is_empty(),
                    });
                    sample_before = self.field.tracked_removed_volume();
                }
            }
            if contact {
                break;
            }
        }
        Ok(())
    }
}

fn thread_intervals(
    q: [f64; 3],
    v: [f64; 3],
    floor: f64,
    ceiling: f64,
    tool: ToolProfile,
) -> Vec<(f64, f64)> {
    let ProfileShape::Thread {
        root_radius,
        tooth_height,
        pitch,
        tooth_count,
    } = tool.shape
    else {
        return Vec::new();
    };
    if tooth_height <= 0. || root_radius >= tool.radius {
        return Vec::new();
    }
    let cone = ToolProfile {
        radius: tool.radius,
        shape: ProfileShape::Cone {
            half_angle_rad: (2. * (tool.radius - root_radius) / tooth_height).atan(),
            flat_radius: root_radius,
        },
    };
    let mut result = Vec::new();
    for tooth in 0..tooth_count {
        let base = f64::from(tooth) * pitch;
        let Some((a, b)) = ray_tool(q, v, floor, ceiling, cone, base, base + tooth_height, 0.)
        else {
            continue;
        };
        let Some((c, d)) = ray_tool(
            [q[0], q[1], -q[2]],
            [v[0], v[1], -v[2]],
            floor,
            ceiling,
            cone,
            -base - tooth_height,
            -base,
            0.,
        ) else {
            continue;
        };
        let (lo, hi) = (a.max(c), b.min(d));
        if hi <= lo {
            continue;
        }
        if let Some((a, b)) = ray_tool(
            q,
            v,
            lo,
            hi,
            ToolProfile::flat(root_radius),
            base,
            base + tooth_height,
            0.,
        ) {
            if a > lo + 1e-6 {
                result.push((lo, a));
            }
            if hi > b + 1e-6 {
                result.push((b, hi));
            }
        } else {
            result.push((lo, hi));
        }
    }
    result
}

/// Intersection of a vertical material ray with a finite revolved cutter.
/// Flat, ball, bull, cone and rounded V profiles are convex epigraphs. Restrict
/// to the radial cylinder and axial slab first, then bracket both profile roots.
#[allow(clippy::too_many_arguments)]
fn ray_tool(
    q: [f64; 3],
    v: [f64; 3],
    mut lo: f64,
    mut hi: f64,
    profile: ToolProfile,
    bottom: f64,
    top: f64,
    pad: f64,
) -> Option<(f64, f64)> {
    let r = profile.radius + pad;
    let a = v[0] * v[0] + v[1] * v[1];
    let b = q[0] * v[0] + q[1] * v[1];
    let c = q[0] * q[0] + q[1] * q[1] - r * r;
    if a < 1e-20 {
        if c > 0. {
            return None;
        }
    } else {
        let disc = b * b - a * c;
        if disc < 0. {
            return None;
        }
        let h = disc.sqrt();
        lo = lo.max((-b - h) / a);
        hi = hi.min((-b + h) / a);
    }
    if v[2].abs() < 1e-12 {
        if q[2] < bottom - pad || q[2] > top + pad {
            return None;
        }
    } else {
        let z0 = (bottom - pad - q[2]) / v[2];
        let z1 = (top + pad - q[2]) / v[2];
        lo = lo.max(z0.min(z1));
        hi = hi.min(z0.max(z1));
    }
    if hi <= lo {
        return None;
    }
    if matches!(profile.shape, ProfileShape::Flat) {
        return Some((lo, hi));
    }
    let f = |z: f64| {
        let d = (q[0] + v[0] * z).hypot(q[1] + v[1] * z);
        profile.offset((d - pad).max(0.)) + bottom - pad - (q[2] + v[2] * z)
    };
    let (mut left, mut right) = (lo, hi);
    for _ in 0..32 {
        let m1 = left + (right - left) / 3.;
        let m2 = right - (right - left) / 3.;
        if f(m1) < f(m2) {
            right = m2;
        } else {
            left = m1;
        }
    }
    let mid = (left + right) * 0.5;
    let mid = if f(lo) < f(mid) {
        lo
    } else if f(hi) < f(mid) {
        hi
    } else {
        mid
    };
    if f(mid) > 0. {
        return None;
    }
    if f(lo) > 0. {
        let (mut a, mut b) = (lo, mid);
        for _ in 0..32 {
            let m = (a + b) * 0.5;
            if f(m) > 0. {
                a = m;
            } else {
                b = m;
            }
        }
        lo = b;
    }
    if f(hi) > 0. {
        let (mut a, mut b) = (mid, hi);
        for _ in 0..32 {
            let m = (a + b) * 0.5;
            if f(m) > 0. {
                b = m;
            } else {
                a = m;
            }
        }
        hi = a;
    }
    (hi > lo).then_some((lo, hi))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tag(kind: MoveKind) -> Tag {
        Tag {
            op_id: 8,
            kind,
            pattern_id: Some(17),
            instance_index: Some(2),
        }
    }
    fn tool() -> RotaryTool {
        RotaryTool {
            tool: SimTool {
                number: 1,
                profile: ToolProfile::flat(1.),
                noncutting_tip_radius: None,
            },
            flute_length: 10.,
            length: 10.,
            neck_length: 0.,
            neck_radius: 1.,
            shank_radius: 1.,
        }
    }
    fn movement(
        axis: RotaryAxis,
        x: f64,
        z: f64,
        angle: f64,
        feed: Option<f64>,
        kind: MoveKind,
    ) -> Step {
        Step::Rotary {
            to: if axis == RotaryAxis::A {
                Point3::new(x, 0., z)
            } else {
                Point3::new(0., x, z)
            },
            angle,
            axis,
            center: Point3::new(0., 0., 0.),
            feed,
            length: 10.,
            tag: tag(kind),
        }
    }
    fn blank(axis: RotaryAxis) -> CylindricalStock {
        CylindricalStock::new(
            if axis == RotaryAxis::A { 0 } else { 1 },
            [0.; 3],
            [-4., 4.],
            5.,
            0.,
            0.25,
        )
        .unwrap()
    }
    fn run(
        cache: &mut RotarySimulationCache,
        p: &Program,
        stock: &CylindricalStock,
        tools: &[RotaryTool],
    ) -> SimResult {
        cache
            .simulate_controlled(
                p,
                stock.min,
                stock.max,
                &SimOptions {
                    resolution: 0.25,
                    tool_radius: 1.,
                },
                tools,
                Some(("blank", &stock.field)),
                true,
                &|| false,
                |_, _| {},
            )
            .unwrap()
    }
    fn groove(axis: RotaryAxis, angle: f64) -> Vec<Step> {
        vec![
            movement(axis, -2., 7., angle, None, MoveKind::Traverse),
            movement(axis, -2., 4., angle, Some(100.), MoveKind::Plunge),
            movement(axis, 2., 4., angle, Some(100.), MoveKind::Cutting),
            movement(axis, 2., 7., angle, None, MoveKind::Retract),
        ]
    }
    #[test]
    fn clipped_long_tool_matches_unclipped_stock_collisions_and_removal() {
        for axis in [RotaryAxis::A, RotaryAxis::B] {
            let stock = blank(axis);
            let mut steps = Vec::new();
            for angle in [0., 37., 90., 173., 270., 360.] {
                steps.extend(groove(axis, angle));
            }
            // Include an air-only full turn and a colliding index move.
            steps.push(movement(axis, 0., 70., 720., None, MoveKind::Traverse));
            steps.push(movement(axis, 0., 3., 720., None, MoveKind::Traverse));
            let p = Program { steps };
            let mut long = tool();
            long.length = 57.;
            long.shank_radius = 1.5;
            let options = SimOptions {
                resolution: 0.25,
                tool_radius: 1.,
            };
            let mut c = config(&p, stock.min, stock.max, &options, &[long], None, true).unwrap();
            let mut results = Vec::new();
            for tight in [false, true] {
                c.tight_bounds = tight;
                let mut state = State::new(&c, Some(&stock.field));
                let mut samples = Some(Vec::new());
                for (i, step) in p.steps().iter().enumerate() {
                    state.step(step, i, &c, &|| false, &mut samples).unwrap();
                }
                results.push((
                    state.collisions,
                    state.field.tracked_removed_volume(),
                    state.field.to_solid_mesh(stock.min[2]),
                    samples.unwrap(),
                ));
            }
            assert_eq!(results[0], results[1], "{axis:?}");
        }
    }
    #[test]
    fn paged_intervals_match_tree_storage_for_rotary_tools_and_force_samples() {
        for axis in [RotaryAxis::A, RotaryAxis::B] {
            for center in [[0.; 3], [2.3, -1.4, 10_000.2]] {
                let axial_axis = if axis == RotaryAxis::A { 0 } else { 1 };
                let stock = CylindricalStock::new(
                    axial_axis,
                    center,
                    [-4.1 + center[axial_axis], 4.3 + center[axial_axis]],
                    5.,
                    1.,
                    0.25,
                )
                .unwrap();
                for shape in [
                    ProfileShape::Flat,
                    ProfileShape::Ball,
                    ProfileShape::BullNose { corner_radius: 0.3 },
                    ProfileShape::Cone {
                        half_angle_rad: std::f64::consts::FRAC_PI_4,
                        flat_radius: 0.2,
                    },
                    ProfileShape::VTip {
                        half_angle_rad: 0.6,
                        tip_radius: 0.2,
                    },
                    ProfileShape::Thread {
                        root_radius: 0.4,
                        tooth_height: 0.5,
                        pitch: 1.,
                        tooth_count: 2,
                    },
                ] {
                    let mut cutter = tool();
                    cutter.tool.profile.shape = shape;
                    let mut steps = Vec::new();
                    for angle in [0., 37., 90., 179., 270., 360.] {
                        steps.extend(groove(axis, angle));
                    }
                    // Repeated material, an air turn and a colliding index.
                    steps.extend(groove(axis, 360.));
                    steps.push(movement(axis, 0., 20., 720., None, MoveKind::Traverse));
                    steps.push(movement(axis, 0., 3., 720., None, MoveKind::Traverse));
                    for step in &mut steps {
                        if let Step::Rotary {
                            to,
                            center: rotary_center,
                            ..
                        } = step
                        {
                            to.x += center[0];
                            to.y += center[1];
                            to.z += center[2];
                            *rotary_center = Point3::new(center[0], center[1], center[2]);
                        }
                    }
                    let program = Program { steps };
                    let mut c = config(
                        &program,
                        stock.min,
                        stock.max,
                        &SimOptions {
                            resolution: 0.25,
                            tool_radius: 1.,
                        },
                        &[cutter],
                        None,
                        true,
                    )
                    .unwrap();
                    let mut results = Vec::new();
                    for reference in [false, true] {
                        c.tight_bounds = !reference;
                        let mut state = State::new(&c, Some(&stock.field));
                        if reference {
                            state.field.use_reference_intervals();
                        }
                        let mut samples = Some(Vec::new());
                        for (index, step) in program.steps().iter().enumerate() {
                            state
                                .step(step, index, &c, &|| false, &mut samples)
                                .unwrap();
                        }
                        results.push((
                            state.collisions,
                            state.field.tracked_removed_volume(),
                            state.field.to_solid_mesh(stock.min[2]),
                            samples.unwrap(),
                        ));
                    }
                    assert_eq!(results[0], results[1], "{axis:?} {shape:?} {center:?}");
                }
            }
        }
    }
    #[test]
    fn full_turn_removes_the_entire_annulus_without_internal_caps() {
        for axis in [RotaryAxis::A, RotaryAxis::B] {
            let axial_axis = if axis == RotaryAxis::A { 0 } else { 1 };
            let transverse_axis = 1 - axial_axis;
            let stock =
                CylindricalStock::new(axial_axis, [0.; 3], [-1., 11.], 16., 0., 0.14).unwrap();
            let mut cutter = tool();
            cutter.tool.profile = ToolProfile::flat(3.);
            cutter.flute_length = 13.;
            cutter.length = 57.;
            cutter.neck_radius = 3.;
            cutter.shank_radius = 3.;
            let p = Program {
                steps: vec![
                    movement(axis, 3., 22., -180., None, MoveKind::Traverse),
                    movement(axis, 3., 15., -180., Some(100.), MoveKind::Plunge),
                    movement(axis, 3., 15., 180., Some(100.), MoveKind::Cutting),
                ],
            };
            let options = SimOptions {
                resolution: 0.14,
                tool_radius: 3.,
            };
            let result = RotarySimulationCache::default()
                .simulate_controlled(
                    &p,
                    stock.min,
                    stock.max,
                    &options,
                    &[cutter],
                    Some(("blank", &stock.field)),
                    true,
                    &|| false,
                    |_, _| {},
                )
                .unwrap();
            assert!(result.is_clean(), "{:?}", result.collisions);
            let field = &result.field;
            let (nx, ny) = field.dims();
            for y in 0..ny {
                for x in 0..nx {
                    let (cx, cy) = field.center(x, y);
                    let axial = [cx, cy][axial_axis];
                    let radial = [cx, cy][transverse_axis];
                    if (1. ..5.).contains(&axial) && radial.abs() > 15.1 {
                        assert!(
                            !field.interval_contact(y * nx + x, -16., 16., stock.min[2]),
                            "uncut annulus {axial},{radial}"
                        );
                    }
                }
            }
            let mesh = field.to_solid_mesh(stock.min[2]);
            let mut bad_normals = 0;
            let mut checked_normals = 0;
            let mut samples = Vec::new();
            for (positions, normals) in mesh
                .positions
                .chunks_exact(4)
                .zip(mesh.normals.chunks_exact(4))
            {
                let p: [f32; 3] =
                    std::array::from_fn(|i| positions.iter().map(|p| p[i]).sum::<f32>() * 0.25);
                let n = normals[0];
                let r = p[transverse_axis].hypot(p[2]);
                if p[axial_axis] > 1.
                    && p[axial_axis] < 5.
                    && (r - 15.).abs() < 0.2
                    && n[axial_axis].abs() < 0.1
                {
                    checked_normals += 1;
                    if (p[transverse_axis] * n[transverse_axis] + p[2] * n[2]) / r < 0.8 {
                        bad_normals += 1;
                        if samples.len() < 8 {
                            samples.push((p, n));
                        }
                    }
                }
            }
            assert!(checked_normals > 100);
            assert_eq!(
            bad_normals, 0,
            "{axis:?}: {bad_normals} of {checked_normals} cut side quads have sideways normals: {samples:?}"
        );
            for point in &mesh.positions {
                if point[axial_axis] > 1. && point[axial_axis] < 5. {
                    assert!(
                        (point[transverse_axis].hypot(point[2]) - 15.).abs() < 0.3,
                        "bad skin {point:?}"
                    );
                }
            }
        }
    }
    #[test]
    fn indexed_a_and_b_cuts_retain_all_sides_and_original_voids() {
        for axis in [RotaryAxis::A, RotaryAxis::B] {
            let stock = blank(axis);
            let mut p = Program {
                steps: groove(axis, 0.),
            };
            let one = run(&mut RotarySimulationCache::default(), &p, &stock, &[tool()]);
            p.steps.extend(groove(axis, 90.));
            p.steps.extend(groove(axis, 180.));
            let result = run(&mut RotarySimulationCache::default(), &p, &stock, &[tool()]);
            assert!(result.is_clean(), "{:?}", result.collisions);
            assert!(result.removed_volume > one.removed_volume * 2.5);
            assert!(result.field.sample(0., 0.) <= 4.001);
            let mesh = result.field.to_solid_mesh(stock.min[2]);
            assert!(mesh.operations.contains(&Some(8)));
            assert!(mesh
                .cut_sources
                .as_ref()
                .unwrap()
                .iter()
                .any(Option::is_some));
            // The untouched bottom of the original cylinder survives upper cuts.
            let (nx, _) = result.field.dims();
            assert!(result.field.interval_contact(
                (nx / 2) + nx * (result.field.dims().1 / 2),
                -3.,
                -2.,
                stock.min[2]
            ));
        }
    }
    #[test]
    fn pure_rotation_between_clear_endpoints_hits_stock_and_does_not_remove_it() {
        let mut stock = blank(RotaryAxis::A);
        let cleared = run(
            &mut RotarySimulationCache::default(),
            &Program {
                steps: groove(RotaryAxis::A, 0.),
            },
            &stock,
            &[tool()],
        );
        stock.field = cleared.field;
        let p = Program {
            steps: vec![
                movement(RotaryAxis::A, 0., 4., 0., None, MoveKind::Traverse),
                movement(RotaryAxis::A, 0., 4., 360., None, MoveKind::Traverse),
            ],
        };
        let result = run(&mut RotarySimulationCache::default(), &p, &stock, &[tool()]);
        assert!(!result.is_clean());
        assert_eq!(result.removed_volume, 0.);
        let hit = &result.collisions[0];
        assert_eq!(hit.step_index, Some(1));
        assert!(hit.fraction > 0. && hit.fraction < 1.);
        assert_eq!(hit.pattern_id, Some(17));
        assert_eq!(hit.instance_index, Some(2));
    }
    #[test]
    fn full_and_multiple_turn_cutting_reuse_the_same_material() {
        let stock = blank(RotaryAxis::A);
        let mut p = Program {
            steps: vec![
                movement(RotaryAxis::A, 0., 7., 0., None, MoveKind::Traverse),
                movement(RotaryAxis::A, 0., 4., 0., Some(100.), MoveKind::Plunge),
                movement(RotaryAxis::A, 0., 4., 360., Some(100.), MoveKind::Cutting),
            ],
        };
        let one = run(&mut RotarySimulationCache::default(), &p, &stock, &[tool()]);
        p.steps.push(movement(
            RotaryAxis::A,
            0.,
            4.,
            720.,
            Some(100.),
            MoveKind::Cutting,
        ));
        let two = run(&mut RotarySimulationCache::default(), &p, &stock, &[tool()]);
        assert!(two.is_clean());
        assert!(one.removed_volume > 1.);
        assert!((two.removed_volume - one.removed_volume).abs() < 0.05);
    }
    #[test]
    fn cache_forward_rewind_and_edited_prefix_match_fresh() {
        let stock = blank(RotaryAxis::A);
        let mut steps = groove(RotaryAxis::A, 0.);
        steps.extend(groove(RotaryAxis::A, 90.));
        let mut cache = RotarySimulationCache::default();
        for n in [3, 5, 8, 4, 8] {
            let p = Program {
                steps: steps[..n].to_vec(),
            };
            let cached = run(&mut cache, &p, &stock, &[tool()]);
            let fresh = run(&mut RotarySimulationCache::default(), &p, &stock, &[tool()]);
            assert_eq!(cached.collisions, fresh.collisions);
            assert_eq!(cached.removed_volume, fresh.removed_volume);
            assert_eq!(
                cached.field.to_solid_mesh(stock.min[2]),
                fresh.field.to_solid_mesh(stock.min[2])
            );
        }
        steps[1] = movement(RotaryAxis::A, -2., 3., 0., Some(100.), MoveKind::Plunge);
        let p = Program { steps };
        let a = run(&mut cache, &p, &stock, &[tool()]);
        let b = run(&mut RotarySimulationCache::default(), &p, &stock, &[tool()]);
        assert_eq!(a.removed_volume, b.removed_volume);
    }
    #[test]
    fn operation_checkpoints_reuse_prefix_after_append_and_edit() {
        let stock = blank(RotaryAxis::A);
        let op = |id, angle| {
            groove(RotaryAxis::A, angle)
                .into_iter()
                .map(|mut step| {
                    if let Step::Rotary { tag, .. } = &mut step {
                        tag.op_id = id;
                    }
                    step
                })
                .collect::<Vec<_>>()
        };
        let mut cache = RotarySimulationCache::default();
        let mut steps = op(1, 0.);
        steps.push(Step::SpindleOff);
        run(&mut cache, &Program { steps }, &stock, &[tool()]);
        let mut steps = op(1, 0.);
        steps.extend(op(2, 90.));
        steps.push(Step::SpindleOff);
        let mut reordered = op(1, 0.);
        reordered.extend(op(3, 180.));
        reordered.extend(op(2, 90.));
        reordered.push(Step::SpindleOff);
        for program in [
            Program {
                steps: steps.clone(),
            },
            {
                let mut edited = steps.clone();
                edited[5] = movement(RotaryAxis::A, -2., 3., 90., Some(100.), MoveKind::Plunge);
                if let Step::Rotary { tag, .. } = &mut edited[5] {
                    tag.op_id = 2;
                }
                Program { steps: edited }
            },
            Program { steps: reordered },
        ] {
            let mut first = None;
            let cached = cache
                .simulate_controlled(
                    &program,
                    stock.min,
                    stock.max,
                    &SimOptions {
                        resolution: 0.25,
                        tool_radius: 1.,
                    },
                    &[tool()],
                    Some(("blank", &stock.field)),
                    true,
                    &|| false,
                    |done, _| {
                        first.get_or_insert(done);
                    },
                )
                .unwrap();
            assert!(
                first.unwrap() >= 4,
                "unchanged operation must not be reverified"
            );
            let fresh = run(
                &mut RotarySimulationCache::default(),
                &program,
                &stock,
                &[tool()],
            );
            assert_eq!(cached.collisions, fresh.collisions);
            assert_eq!(cached.removed_volume, fresh.removed_volume);
            assert_eq!(
                cached.field.to_solid_mesh(stock.min[2]),
                fresh.field.to_solid_mesh(stock.min[2])
            );
        }
    }
    #[test]
    fn cancelled_suffix_keeps_the_last_verified_prefix() {
        let stock = blank(RotaryAxis::A);
        let mut first = groove(RotaryAxis::A, 0.);
        first.push(Step::SpindleOff);
        let mut cache = RotarySimulationCache::default();
        run(&mut cache, &Program { steps: first }, &stock, &[tool()]);
        let mut steps = groove(RotaryAxis::A, 0.);
        let mut second = groove(RotaryAxis::A, 90.);
        for step in &mut second {
            if let Step::Rotary { tag, .. } = step {
                tag.op_id = 9;
            }
        }
        steps.extend(second);
        steps.push(Step::SpindleOff);
        let program = Program { steps };
        let stop = std::cell::Cell::new(false);
        let options = SimOptions {
            resolution: 0.25,
            tool_radius: 1.,
        };
        assert!(cache
            .simulate_controlled(
                &program,
                stock.min,
                stock.max,
                &options,
                &[tool()],
                Some(("blank", &stock.field)),
                true,
                &|| stop.get(),
                |_, _| stop.set(true)
            )
            .is_err());
        let mut first = None;
        let result = cache
            .simulate_controlled(
                &program,
                stock.min,
                stock.max,
                &options,
                &[tool()],
                Some(("blank", &stock.field)),
                true,
                &|| false,
                |done, _| {
                    first.get_or_insert(done);
                },
            )
            .unwrap();
        assert!(first.unwrap() >= 4);
        let fresh = run(
            &mut RotarySimulationCache::default(),
            &program,
            &stock,
            &[tool()],
        );
        assert_eq!(result.collisions, fresh.collisions);
        assert_eq!(result.removed_volume, fresh.removed_volume);
    }
    #[test]
    fn short_flutes_report_noncutting_shank_contact() {
        let stock = blank(RotaryAxis::A);
        let mut t = tool();
        t.flute_length = 0.5;
        t.shank_radius = 2.;
        let p = Program {
            steps: groove(RotaryAxis::A, 0.),
        };
        let result = run(&mut RotarySimulationCache::default(), &p, &stock, &[t]);
        assert!(result
            .collisions
            .iter()
            .any(|c| c.kind == CollisionKind::NonCuttingThroughStock));
    }
    #[test]
    fn cancellation_and_mismatched_physical_axes_fail_explicitly() {
        let stock = blank(RotaryAxis::A);
        let mut p = Program {
            steps: groove(RotaryAxis::A, 0.),
        };
        let mut cache = RotarySimulationCache::default();
        assert!(cache
            .simulate_controlled(
                &p,
                stock.min,
                stock.max,
                &SimOptions::default(),
                &[tool()],
                None,
                false,
                &|| true,
                |_, _| {}
            )
            .is_err());
        p.steps.extend(groove(RotaryAxis::B, 0.));
        assert!(cache
            .simulate_controlled(
                &p,
                stock.min,
                stock.max,
                &SimOptions::default(),
                &[tool()],
                None,
                false,
                &|| false,
                |_, _| {}
            )
            .unwrap_err()
            .contains("one physical"));
    }
    #[test]
    fn rotated_profile_ray_matches_ball_and_cone_geometry() {
        let ball = ToolProfile {
            radius: 2.,
            shape: ProfileShape::Ball,
        };
        let (lo, hi) = ray_tool([0., 0., 0.], [0., 0., 1.], -10., 10., ball, 0., 5., 0.).unwrap();
        assert!(lo.abs() < 1e-6);
        assert_eq!(hi, 5.);
        let (lo, _) = ray_tool([1., 0., 0.], [0., 0., 1.], -10., 10., ball, 0., 5., 0.).unwrap();
        assert!((lo - (2. - 3_f64.sqrt())).abs() < 1e-6);
        let cone = ToolProfile {
            radius: 2.,
            shape: ProfileShape::Cone {
                half_angle_rad: std::f64::consts::FRAC_PI_4,
                flat_radius: 0.,
            },
        };
        let (lo, _) = ray_tool([1., 0., 0.], [0., 0., 1.], -10., 10., cone, 0., 5., 0.).unwrap();
        assert!((lo - 1.).abs() < 1e-6);
    }
    #[test]
    fn finite_thread_teeth_preserve_core_and_gaps_at_any_orientation() {
        let profile = ToolProfile {
            radius: 2.,
            shape: ProfileShape::Thread {
                root_radius: 1.,
                tooth_height: 1.,
                pitch: 2.,
                tooth_count: 2,
            },
        };
        let axial = thread_intervals([1.5, 0., 0.], [0., 0., 1.], -3., 4., profile);
        assert_eq!(axial.len(), 2);
        assert!((axial[0].0 - 0.25).abs() < 1e-6);
        assert!((axial[0].1 - 0.75).abs() < 1e-6);
        assert!((axial[1].0 - 2.25).abs() < 1e-6);
        assert!(thread_intervals([0.5, 0., 0.], [0., 0., 1.], -3., 4., profile).is_empty());
        let side = thread_intervals([0., 0., 0.5], [1., 0., 0.], -3., 3., profile);
        assert_eq!(side.len(), 2);
        assert!((side[0].0 + 2.).abs() < 1e-5);
        assert!((side[0].1 + 1.).abs() < 1e-5);
        assert!((side[1].0 - 1.).abs() < 1e-5);
    }
    #[test]
    fn removal_trace_matches_verification_volume_and_programmed_duration() {
        let stock = blank(RotaryAxis::A);
        let p = Program {
            steps: groove(RotaryAxis::A, 90.),
        };
        let verified = run(&mut RotarySimulationCache::default(), &p, &stock, &[tool()]);
        let samples = analyze_rotary_removal_controlled(
            &p,
            stock.min,
            stock.max,
            &SimOptions {
                resolution: 0.25,
                tool_radius: 1.,
            },
            &[tool()],
            Some(&stock.field),
            &|| false,
            |_, _| {},
        )
        .unwrap();
        assert!(
            (samples.iter().map(|s| s.removed_volume_mm3).sum::<f64>() - verified.removed_volume)
                .abs()
                < 1e-6
        );
        assert!((samples.iter().map(|s| s.seconds).sum::<f64>() - 12.).abs() < 1e-6);
        assert!(samples.iter().all(|s| !s.collision));
    }
    #[test]
    fn geometry_only_tracking_mode_preserves_stock_and_invalidates_volume_checkpoints() {
        let stock = blank(RotaryAxis::A);
        let program = Program {
            steps: groove(RotaryAxis::A, 90.),
        };
        let mut cache = RotarySimulationCache::default();
        let tracked = run(&mut cache, &program, &stock, &[tool()]);
        assert!(tracked.removed_volume > 0.);
        cache.set_volume_tracking(false);
        let geometry = run(&mut cache, &program, &stock, &[tool()]);
        assert_eq!(geometry.removed_volume, 0.);
        assert_eq!(geometry.collisions, tracked.collisions);
        assert_eq!(
            geometry.field.to_solid_mesh(stock.min[2]),
            tracked.field.to_solid_mesh(stock.min[2])
        );
        cache.set_volume_tracking(true);
        let restored = run(&mut cache, &program, &stock, &[tool()]);
        assert_eq!(restored.removed_volume, tracked.removed_volume);
        assert_eq!(restored.collisions, tracked.collisions);
        assert_eq!(
            restored.field.to_solid_mesh(stock.min[2]),
            tracked.field.to_solid_mesh(stock.min[2])
        );
    }
}
