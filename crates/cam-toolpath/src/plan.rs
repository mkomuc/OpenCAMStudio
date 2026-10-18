//! Job planning: run every operation's strategy and assemble the results into a
//! single, machine-ready CL-data program.
//!
//! This is the thin layer that wraps the pure per-operation strategies with the
//! job-level machine control — tool changes, spindle, coolant — and splices the
//! operation fragments together in order.

use cam_cldata::{Coolant, MoveKind, Point3, Program, SpindleDir, Step, Tag};
use cam_model::{Document, Operation};

use crate::{
    CancelToken, CarveStrategy, ChamferStrategy, Diagnostic, DrillStrategy, EngraveStrategy,
    FaceStrategy, JobEnv, PocketStrategy, ProfileStrategy, Strategy, StrategyResult,
    ThreadStrategy,
};

/// Whether all requested operations finished successfully. A default result is
/// incomplete until the planner has established otherwise.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum JobCompletion {
    /// No completed result has been established yet.
    #[default]
    Incomplete,
    /// All requested operations finished without error diagnostics.
    Complete,
    /// A cancellation request or strategy cancellation stopped the job.
    Cancelled,
    /// At least one operation reported an error.
    Failed,
}

/// A planned job, including the completion state required before export.
/// Cancelled/failed jobs may retain completed operations for inspection; their
/// program is not a complete manufacturing job and must not be posted.
#[derive(Clone, Debug, Default, PartialEq)]
#[must_use = "check job completion before posting the program"]
pub struct JobResult {
    pub program: Program,
    pub diagnostics: Vec<Diagnostic>,
    pub completion: JobCompletion,
}

/// Assemble a whole-job [`Program`] from a [`Document`].
///
/// Each operation is turned into its strategy, computed against the setup's
/// heights and tools, and its motions are spliced between a job preamble (tool
/// change, spindle on, coolant on) and postamble (coolant off, spindle off). A
/// tool change is inserted whenever the tool number changes between operations.
///
/// Diagnostics from every operation are collected; if any operation errors, its
/// motions are omitted but planning continues so the caller sees all problems at
/// once. Returns the program, combined diagnostics and explicit completion state.
/// Only [`JobCompletion::Complete`] is eligible for export. An intentionally
/// empty job can be complete; cancellation is never inferred from program size.
pub fn build_job(
    doc: &Document,
    spindle_rpm: f64,
    dir: SpindleDir,
    stock: Option<([f64; 2], [f64; 2])>,
    tool_change_height: f64,
    cancel: &CancelToken,
) -> JobResult {
    build_job_with_compute(
        doc,
        spindle_rpm,
        dir,
        stock,
        tool_change_height,
        cancel,
        compute,
    )
}

/// Immutable planning configuration, shared by synchronous and worker callers.
pub struct PlanRequest<'a> {
    pub document: &'a Document,
    pub spindle_rpm: f64,
    pub dir: SpindleDir,
    pub stock: Option<([f64; 2], [f64; 2])>,
    pub tool_change_height: f64,
}

/// Reuse only matching operation fragments. Linking is always rebuilt in order.
pub fn build_job_cached(
    request: PlanRequest<'_>,
    cancel: &CancelToken,
    cache: &mut crate::OperationCache,
    mut progress: impl FnMut(u32),
) -> JobResult {
    cache.reset_stats();
    build_job_with_compute(
        request.document,
        request.spindle_rpm,
        request.dir,
        request.stock,
        request.tool_change_height,
        cancel,
        |op, env, token| {
            progress(op.id());
            cache.compute(op, env, token, || compute(op, env, token))
        },
    )
}

fn build_job_with_compute(
    doc: &Document,
    spindle_rpm: f64,
    dir: SpindleDir,
    stock: Option<([f64; 2], [f64; 2])>,
    tool_change_height: f64,
    cancel: &CancelToken,
    mut compute: impl FnMut(&Operation, &JobEnv, &CancelToken) -> StrategyResult,
) -> JobResult {
    let setup = doc.setup();
    let mut env = JobEnv {
        heights: setup.heights,
        tools: &setup.tools,
        stock,
        // The same direction the job's `Step::Spindle` will command, so a strategy's
        // climb/conventional choice and the spindle it is cut with cannot disagree.
        spindle: dir,
    };

    let mut program = Program::new();
    let mut diagnostics = Vec::new();
    let mut completion = JobCompletion::Complete;
    if let Err(reason) = setup.validate_patterns() {
        return JobResult {
            program,
            diagnostics: vec![Diagnostic::error(format!("invalid operation pattern: {reason}")).for_setup(setup.id)],
            completion: JobCompletion::Failed,
        };
    }

    // Every use of this value assumes it is *above* the work: `lift_to_tool_change`
    // is named for what it is supposed to do, and the cross that follows travels at
    // it. Nothing validated that. A setup whose Tool Change Height sits below the
    // clearance plane would turn the lift into a descent and cross the part below
    // clearance — so raise it, and say so rather than silently overruling the value
    // the operator typed.
    let max_clearance = setup
        .operations
        .iter()
        .filter(|op| doc.operation_is_included(setup.id, op.id()))
        .map(|op| {
            setup
                .height_settings
                .get(&op.id())
                .and_then(|settings| settings.resolve(doc, op).ok())
                .map_or(setup.heights_for(op.id()).clearance, |heights| {
                    heights.clearance
                })
        })
        .fold(setup.heights.clearance, f64::max);
    let tool_change_height = if tool_change_height < max_clearance {
        diagnostics.push(Diagnostic::warning(format!(
            "tool-change height {tool_change_height:.3} mm is below the clearance plane \
             {:.3} mm; using the clearance plane instead — a tool change or re-fixture \
             must not bring the tool down.",
            max_clearance
        )));
        max_clearance
    } else {
        tool_change_height
    };

    program.push(Step::Comment(setup.name.clone()));

    // Compute every operation's fragment once. Coordinates are in the part frame; the
    // post re-references each group to its origin.
    let mut seeds: Vec<Planned> = Vec::new();
    for operation in &setup.operations {
        if !doc.operation_is_included(setup.id, operation.id()) {
            continue;
        }
        if cancel.is_cancelled() {
            completion = JobCompletion::Cancelled;
            break;
        }
        env.heights = setup.heights_for(operation.id());
        let configured = setup
            .height_settings
            .get(&operation.id())
            .map(|settings| settings.resolve(doc, operation));
        if let Some(Ok(heights)) = configured {
            env.heights = heights.safety();
        }
        let mut result = if let Some(Err(reason)) = &configured {
            StrategyResult {
                diagnostics: vec![Diagnostic::error(reason.clone())],
                ..Default::default()
            }
        } else if let Some(reason) = setup
            .operation_heights
            .get(&operation.id())
            .and_then(|h| h.validate().err())
        {
            StrategyResult {
                diagnostics: vec![Diagnostic::error(reason)],
                ..Default::default()
            }
        } else if doc.cad_linked_operations.contains(&operation.id())
            && !doc.geometry.bindings.contains_key(&operation.id())
        {
            StrategyResult {
                diagnostics: vec![Diagnostic::error(
                    "Linked CAD geometry binding is missing; reselect the source geometry before planning",
                ).with_code(crate::DiagnosticCode::GeometryUnresolved)],
                ..Default::default()
            }
        } else if let Some(binding) = doc.geometry.bindings.get(&operation.id()) {
            match &binding.status {
                cam_model::ResolutionStatus::NeedsReselection { reason } => StrategyResult {
                    diagnostics: vec![Diagnostic::error(format!("Needs reselection: {reason}"))
                        .with_code(crate::DiagnosticCode::GeometryUnresolved)],
                    ..Default::default()
                },
                cam_model::ResolutionStatus::Resolved => {
                    if !binding
                        .selected
                        .iter()
                        .all(|r| doc.geometry.entity_exists(r))
                        || binding.resolved_fingerprint.as_ref()
                            != Some(&doc.geometry.binding_fingerprint(binding))
                    {
                        StrategyResult {
                            diagnostics: vec![Diagnostic::error("Linked CAD geometry has no matching resolved snapshot; resolve it before planning").with_code(crate::DiagnosticCode::GeometryUnresolved)],
                            ..Default::default()
                        }
                    } else {
                        compute_at_heights(
                            operation,
                            &env,
                            configured
                                .as_ref()
                                .and_then(|r| r.as_ref().ok())
                                .map_or(binding.effective_heights(), |h| h.cutting()),
                            cancel,
                            &mut compute,
                            true,
                        )
                    }
                }
            }
        } else if let Some(Ok(heights)) = configured {
            compute_at_heights(
                operation,
                &env,
                heights.cutting(),
                cancel,
                &mut compute,
                false,
            )
        } else {
            compute(operation, &env, cancel)
        };
        if let Some(Ok(heights)) = configured {
            if let Some(feed_z) = heights.feed {
                match apply_feed_height(&result.program, feed_z, operation) {
                    Ok(program) => result.program = program,
                    Err(reason) => {
                        result.program = Program::new();
                        result.diagnostics.push(Diagnostic::error(reason));
                    }
                }
            }
        }
        let failed = result.has_errors();
        let cancelled = result.cancelled || cancel.is_cancelled();
        // Stamp ownership here: the planner knows which operation produced these,
        // the strategy does not need to.
        diagnostics.extend(
            result
                .diagnostics
                .into_iter()
                .map(|d| d.for_op(operation.id()).for_setup(setup.id)),
        );
        if cancelled {
            // Discard the unfinished fragment, including cancellation in the last
            // operation where there is no next loop iteration to observe the token.
            completion = JobCompletion::Cancelled;
            break;
        }
        if failed {
            completion = JobCompletion::Failed;
            continue;
        }
        if result.program.is_empty() {
            continue;
        }
        // Per-operation spindle speed: the op's own value if set, else the job default
        // (which keeps existing documents — every op at rpm 0 — unchanged).
        let rpm = if operation.spindle_rpm() > 0.0 {
            operation.spindle_rpm()
        } else {
            spindle_rpm
        };
        seeds.push(Planned {
            op_id: operation.id(),
            clearance: env.heights.clearance,
            tool: operation.tools()[0],
            rpm,
            // A multi-tool operation (Carve) emits its own tool change mid-fragment;
            // whatever it leaves in the spindle is what the next instance compares
            // against, so read it back rather than assume.
            ends_with: last_tool_change(&result.program),
            datum: operation.work_offset(),
            fragment: result.program,
            pattern_id: None,
            instance_index: None,
        });
    }

    // Expand only after every included seed has been computed exactly once.
    // Transitions and machine control are assembled later around each derived fragment.
    let seeds_by_id: std::collections::HashMap<u32, Planned> = seeds.into_iter().map(|p| (p.op_id, p)).collect();
    let mut planned: Vec<Planned> = Vec::new();
    if cancel.is_cancelled() {
        // Preserve completed ordinary work as a preview if cancellation arrived while
        // computing seed fragments. Pattern expansion can multiply that work many
        // thousands of times, so a canceled run never begins or continues expansion.
        let grouped: std::collections::HashSet<u32> = setup.patterns.iter()
            .flat_map(|pattern| pattern.members.iter().copied())
            .collect();
        for operation in &setup.operations {
            if !grouped.contains(&operation.id()) {
                if let Some(seed) = seeds_by_id.get(&operation.id()) { planned.push(seed.clone()); }
            }
        }
        completion = JobCompletion::Cancelled;
    } else {
        for item in setup.execution_items() {
            if cancel.is_cancelled() { completion = JobCompletion::Cancelled; break; }
            match item {
                cam_model::ExecutionItem::Operation(operation) => {
                    if let Some(seed) = seeds_by_id.get(&operation.id()) { planned.push(seed.clone()); }
                }
                cam_model::ExecutionItem::Pattern(pattern) => {
                    if pattern.suppressed { continue; }
                    let count = match pattern.kind.instance_count() { Some(n) if n <= 10_000 => n, _ => { diagnostics.push(Diagnostic::error("pattern instance count exceeds the supported limit").for_setup(setup.id)); completion = JobCompletion::Failed; continue; } };
                    let required = pattern.members.iter().filter_map(|id| seeds_by_id.get(id)).try_fold(0usize, |sum, seed| seed.fragment.steps().len().checked_mul(count)?.checked_add(sum));
                    let current = planned.iter().try_fold(0usize, |sum, p| sum.checked_add(p.fragment.steps().len()));
                    if required.is_none() || current.and_then(|n| n.checked_add(required.unwrap_or(usize::MAX))).is_none_or(|n| n > 5_000_000) {
                        diagnostics.push(Diagnostic::error("expanded pattern run exceeds 5,000,000 motion steps").for_setup(setup.id));
                        completion = JobCompletion::Failed;
                        continue;
                    }
                    let center = match pattern.kind { cam_model::PatternKind::Circular { center_xy, .. } => center_xy, _ => [0.0, 0.0] };
                    let mut append = |op_id: u32, index: usize| {
                        if cancel.is_cancelled() { return; }
                        let Some(seed) = seeds_by_id.get(&op_id) else { return; };
                        let Some((translation, degrees)) = pattern.kind.transform_xy(index) else { completion = JobCompletion::Failed; return; };
                        let fragment = seed.fragment.transformed_xy(center, translation, degrees).with_pattern_owner(pattern.id, index as u32);
                        planned.push(Planned { fragment, pattern_id: Some(pattern.id), instance_index: Some(index as u32), ..seed.clone() });
                    };
                    match pattern.order {
                        cam_model::PatternOrder::ByInstance => for index in 0..count { for member in &pattern.members { append(*member, index); } },
                        cam_model::PatternOrder::ByOperation => for member in &pattern.members { for index in 0..count { append(*member, index); } },
                    }
                    drop(append);
                    if cancel.is_cancelled() { completion = JobCompletion::Cancelled; }
                }
            }
        }
    }
    if planned.iter().try_fold(0usize, |sum, p| sum.checked_add(p.fragment.steps().len())).is_none_or(|n| n > 5_000_000) {
        diagnostics.push(Diagnostic::error("expanded pattern run exceeds 5,000,000 motion steps").for_setup(setup.id));
        completion = JobCompletion::Failed;
        planned.clear();
    }

    // Group the operations by origin, in the setup's origin order, so each datum's
    // block is contiguous however the operations happen to be ordered in the list — a
    // reorientation is done once, never ping-ponged. A stable sort keeps each group's
    // internal order; an operation whose `work_offset` names no known origin falls into
    // the base group (rank 0), never silently dropped.
    let rank: std::collections::HashMap<u32, usize> = setup
        .origin_indices()
        .into_iter()
        .enumerate()
        .map(|(i, idx)| (idx, i))
        .collect();
    planned.sort_by_key(|pl| rank.get(&pl.datum).copied().unwrap_or(0));

    // Emit each operation once, under the origin its `work_offset` names. Every new
    // context — the first operation, a tool change, or a reorientation — opens by lifting
    // to the tool-change height and descending to clearance (a reorientation adds the
    // `M00` operator stop for re-fixturing); the emitter handles that.
    let mut st = EmitState::new();
    // A cancelled planning run retains its completed prefix as a non-exportable
    // preview. Cancellation first arriving during assembly stops at a boundary.
    let cancelled_before_assembly = cancel.is_cancelled();
    for pl in &planned {
        if !cancelled_before_assembly && cancel.is_cancelled() {
            completion = JobCompletion::Cancelled;
            break;
        }
        emit_instance(
            &mut program,
            pl,
            dir,
            &mut st,
            tool_change_height,
            pl.clearance,
        );
    }

    // The same shutdown as a re-fixture halt, and for the same reason: the operator is
    // about to put hands on the work — here to take the part out. It used to be two
    // lines in a different order with no lift, because it was written at first light
    // and the re-fixture path was written eight weeks later; nothing ever produced
    // both in one program until a multi-datum job with a tool change did.
    if st.spindle_started {
        let last_op = planned.last().map(|pl| pl.op_id).unwrap_or(0);
        shut_down(&mut program, tool_change_height, last_op);
    }

    // Covers cancellation during assembly and deliberately empty documents too.
    if cancel.is_cancelled() {
        completion = JobCompletion::Cancelled;
    }
    if let Err(error) = program.validate() {
        let mut diagnostic = Diagnostic::error(error.to_string());
        if let Some(op) = error.op {
            diagnostic = diagnostic.for_op(op);
        }
        if let Some(tag) = program.steps().get(error.step).and_then(Step::tag) {
            if let (Some(pattern_id), Some(instance_index)) = (tag.pattern_id, tag.instance_index) {
                diagnostic = diagnostic.for_pattern(pattern_id, instance_index);
            }
        }
        diagnostics.push(
            diagnostic
                .with_code(crate::DiagnosticCode::InvalidMotion)
                .for_setup(setup.id)
                .for_segment(error.step),
        );
        if completion != JobCompletion::Cancelled {
            completion = JobCompletion::Failed;
        }
    }
    JobResult {
        program,
        diagnostics,
        completion,
    }
}

/// Adapt the legacy strategies to an explicit absolute cutting interval. Pocket
/// and profile compute in a local Z frame so reach checks measure the physical
/// interval, even when the imported floor lies above world zero. Posting still
/// applies the work datum exactly once.
fn compute_at_heights(
    operation: &Operation,
    env: &JobEnv,
    heights: cam_model::CuttingHeights,
    cancel: &CancelToken,
    compute: &mut impl FnMut(&Operation, &JobEnv, &CancelToken) -> StrategyResult,
    linked: bool,
) -> StrategyResult {
    if let Err(reason) = heights.validate() {
        return StrategyResult {
            diagnostics: vec![Diagnostic::error(reason)],
            ..Default::default()
        };
    }
    if env.heights.clearance < heights.top || env.heights.retract < heights.top {
        return StrategyResult {
            diagnostics: vec![Diagnostic::error(format!(
                "Clearance Z{:.3} mm and retract Z{:.3} mm must both be at or above \
                 cutting top Z{:.3} mm. Adjust the references and offsets in Operation > Heights \
                 to resolve at or above Z{:.3} mm",
                env.heights.clearance, env.heights.retract, heights.top, heights.top,
            ))],
            ..Default::default()
        };
    }
    let mut op = operation.clone();
    let depth = heights.top - heights.bottom;
    let shift = match &mut op {
        Operation::Pocket(p) => {
            p.depth = depth;
            heights.top
        }
        Operation::Profile(p) => {
            p.depth = depth;
            heights.top
        }
        Operation::Drill(p) => {
            p.depth = depth;
            p.start_offset = 0.0;
            0.0
        }
        Operation::Face(p) => {
            p.depth = depth;
            p.start_offset = heights.top;
            0.0
        }
        Operation::Chamfer(p) => {
            p.top = heights.top;
            p.depth = depth;
            0.0
        }
        Operation::Engrave(p) => {
            p.top = heights.top;
            p.depth = depth;
            0.0
        }
        Operation::Carve(p) => {
            p.top = heights.top;
            p.depth = depth;
            0.0
        }
        Operation::Thread(p) => {
            p.z_top = heights.top;
            p.z_bottom = heights.bottom;
            0.0
        }
    };
    let local = JobEnv {
        heights: cam_model::Heights::new(
            env.heights.clearance - shift,
            env.heights.retract - shift,
            if shift != 0.0 || matches!(op, Operation::Pocket(_) | Operation::Profile(_)) {
                0.0
            } else if matches!(op, Operation::Drill(_)) {
                heights.top
            } else {
                env.heights.top_of_stock
            },
        ),
        tools: env.tools,
        // The existing facing strategy sweeps a rectangle. Its resolver only
        // certifies whole-body rectangular top faces; use that selected extent.
        stock: if linked && matches!(op, Operation::Face(_)) {
            None
        } else {
            env.stock
        },
        spindle: env.spindle,
    };
    let mut result = compute(&op, &local, cancel);
    if shift != 0.0 {
        result.program = result.program.translated([0.0, 0.0, shift]);
    }
    result
}

/// Separate the air approach from the feed descent at the configured feed plane.
/// Drilling cycles are expanded when their feed plane differs from their retract
/// plane, because canned cycles share those planes on several target controls.
fn apply_feed_height(
    program: &Program,
    feed_z: f64,
    operation: &Operation,
) -> Result<Program, String> {
    let plunge = match operation {
        Operation::Profile(o) => o.plunge_feed,
        Operation::Pocket(o) => o.clear.plunge_feed,
        Operation::Face(o) => o.plunge_feed,
        Operation::Drill(o) => o.feed,
        Operation::Thread(o) => o.plunge_feed,
        Operation::Chamfer(o) => o.plunge_feed,
        Operation::Engrave(o) => o.plunge_feed,
        Operation::Carve(o) => o.plunge_feed,
    };
    let mut expanded = Program::new();
    for step in program.steps() {
        if let Step::Drill(c) = step {
            if (c.retract - feed_z).abs() > 1e-9 {
                let entry = feed_z.min(c.retract);
                for &[x, y] in &c.points {
                    expanded.push(Step::Rapid {
                        to: Point3::new(x, y, c.retract.max(feed_z)),
                        tag: c.tag,
                    });
                    expanded.push(Step::Rapid {
                        to: Point3::new(x, y, entry),
                        tag: c.tag,
                    });
                    let mut previous = c.z_top;
                    let peck = c.peck.unwrap_or(c.z_top - c.depth);
                    let count = ((c.z_top - c.depth) / peck).ceil();
                    // Strategy validation bounds the interval; cap expansion against
                    // malicious/tiny peck settings, matching the post's own bound.
                    if !count.is_finite() || count * c.points.len() as f64 > 100_000. {
                        return Err(
                            "Feed-height drilling exceeds 100000 pecks; increase the peck depth"
                                .into(),
                        );
                    }
                    for index in 1..=count as usize {
                        let z = (c.z_top - index as f64 * peck).max(c.depth);
                        if index > 1 {
                            expanded.push(Step::Rapid {
                                to: Point3::new(x, y, (previous + 0.5).min(entry)),
                                tag: c.tag,
                            });
                        }
                        expanded.push(Step::Linear {
                            to: Point3::new(x, y, z),
                            feed: c.feed,
                            tag: c.tag,
                        });
                        if z <= c.depth {
                            if let Some(seconds) = c.dwell {
                                expanded.push(Step::Dwell { seconds });
                            }
                        }
                        expanded.push(Step::Rapid {
                            to: Point3::new(x, y, c.retract.max(feed_z)),
                            tag: c.tag,
                        });
                        previous = z;
                    }
                }
                continue;
            }
        }
        expanded.push(step.clone());
    }
    let mut out = Program::new();
    let mut previous: Option<Point3> = None;
    for step in expanded.steps() {
        match (previous, step) {
            (Some(from), Step::Rapid { to, tag })
                if (from.x - to.x).abs() < 1e-9
                    && (from.y - to.y).abs() < 1e-9
                    && to.z < from.z
                    && to.z < feed_z =>
            {
                if from.z > feed_z {
                    out.push(Step::Rapid {
                        to: Point3::new(to.x, to.y, feed_z),
                        tag: *tag,
                    });
                }
                out.push(Step::Linear {
                    to: *to,
                    feed: plunge,
                    tag: *tag,
                });
            }
            (Some(from), Step::Linear { to, feed, tag })
                if matches!(tag.kind, MoveKind::Plunge | MoveKind::Link)
                    && (from.x - to.x).abs() < 1e-9
                    && (from.y - to.y).abs() < 1e-9
                    && to.z < from.z
                    && from.z > feed_z =>
            {
                out.push(Step::Rapid {
                    to: Point3::new(to.x, to.y, feed_z.max(to.z)),
                    tag: *tag,
                });
                if to.z < feed_z {
                    out.push(Step::Linear {
                        to: *to,
                        feed: *feed,
                        tag: *tag,
                    });
                }
            }
            _ => out.push(step.clone()),
        }
        previous = match step {
            Step::Rapid { to, .. } | Step::Linear { to, .. } => Some(*to),
            Step::Arc { end, .. } => Some(*end),
            Step::Drill(c) => c.points.last().map(|[x, y]| Point3::new(*x, *y, c.retract)),
            _ => previous,
        };
    }
    Ok(out)
}

/// One operation, computed and ready to be emitted. The fragment's coordinates are in
/// the part frame; the post re-references them to the operation's origin.
#[derive(Clone)]
struct Planned {
    /// The emitting operation's id — for tagging the group's start-point rapid.
    op_id: u32,
    clearance: f64,
    fragment: Program,
    tool: u32,
    rpm: f64,
    /// The tool a multi-tool fragment (Carve) leaves in the spindle, if any.
    ends_with: Option<u32>,
    /// The operation's origin index (its `work_offset`) — the group it belongs to.
    datum: u32,
    pattern_id: Option<u64>,
    instance_index: Option<u32>,
}

impl Planned {
    fn tag(&self, kind: MoveKind) -> Tag {
        let mut tag = Tag::new(self.op_id, kind);
        tag.pattern_id = self.pattern_id;
        tag.instance_index = self.instance_index;
        tag
    }
}

/// Running machine state threaded across emitted operation instances, so a tool,
/// spindle speed, datum or coolant is re-commanded only when it actually changes.
struct EmitState {
    spindle_started: bool,
    current_rpm: Option<f64>,
    current_tool: Option<u32>,
    current_datum: u32,
    last_clearance: Option<f64>,
}

impl EmitState {
    fn new() -> Self {
        // Datum 1 is the base WCS and the initial state, so an op on datum 1 emits no
        // `Step::Datum`; a single-datum job's output is unchanged from before multi-WCS.
        Self {
            spindle_started: false,
            current_rpm: None,
            current_tool: None,
            current_datum: 1,
            last_clearance: None,
        }
    }
}

/// Emit one operation instance, opening every new context from the tool-change height.
///
/// The datum select precedes the tool change so a post can state `G15 H<n>` in the
/// tool-section head; tool/spindle/coolant are re-commanded only on change. Then, for
/// the first operation, a tool change, or a reorientation, the planner crosses to the
/// operation's start at the tool-change height and descends to clearance before handing
/// off — so the tool always reaches a known safe height before descending, whatever the
/// machine's prior state. A reorientation additionally halts (`M00`) for the manual
/// re-fixture, and its cross is left visually broken in the backplot (the pen-lift at the
/// `Datum`). Same-tool, same-datum consecutive operations get no transition — they hop at
/// clearance via the operation's own approach.
fn emit_instance(
    program: &mut Program,
    pl: &Planned,
    dir: SpindleDir,
    st: &mut EmitState,
    tool_change_height: f64,
    clearance: f64,
) {
    let datum_changing = st.current_datum != pl.datum;
    let tool_changing = st.current_tool != Some(pl.tool);

    if datum_changing {
        // A new datum is a physical reorientation of the part. Before the operator
        // handles it, stop the spindle and coolant (`M05`/`M09`) so nothing is live
        // during the flip, lift clear to the tool-change height so the part is free to
        // handle, then halt (`M00`) — but not before the *first* group (nothing to
        // re-fixture from). Force spindle and coolant to be re-commanded for the new
        // group by clearing their running state.
        if st.spindle_started {
            shut_down(program, tool_change_height, pl.op_id);
            program.push(Step::Stop);
            st.current_rpm = None;
            st.spindle_started = false;
        }
        program.push(Step::Datum(pl.datum));
        st.current_datum = pl.datum;
    } else if tool_changing && st.spindle_started {
        // A same-datum tool change: lift clear to the tool-change height in place before
        // the `M6`, so the ATC swings with the tool out of the work. The first tool load
        // (spindle not yet started) has nothing to lift away from — the cross below still
        // takes it up to the tool-change height first.
        lift_to_tool_change(program, tool_change_height, pl.op_id);
    }
    if tool_changing {
        program.push(Step::ToolChange { tool: pl.tool });
        st.current_tool = Some(pl.tool);
    }
    if st
        .current_rpm
        .is_none_or(|c| (c - pl.rpm).abs() > f64::EPSILON)
    {
        program.push(Step::Spindle { rpm: pl.rpm, dir });
        st.current_rpm = Some(pl.rpm);
    }
    // Coolant on once, after the spindle first starts.
    if !st.spindle_started {
        program.push(Step::Coolant(Coolant::Flood));
        st.spindle_started = true;
    }
    // Open every new context from the tool-change height: cross to this operation's start
    // XY at that height, then descend to clearance and hand off (the operation owns
    // clearance and below). Tagged `Traverse` so the backplot draws it apart. For the
    // first op and same-datum tool changes it draws as one continuous up-over-down; for a
    // reorientation the pen-lift at the `Datum` breaks the cross, leaving it distinct.
    // Same-tool, same-datum consecutive operations skip this and hop at clearance.
    if tool_changing || datum_changing {
        if let Some(start) = first_position(&pl.fragment) {
            program.push(Step::Rapid {
                to: Point3::new(start.x, start.y, tool_change_height),
                tag: pl.tag(MoveKind::Traverse),
            });
            program.push(Step::Rapid {
                to: Point3::new(start.x, start.y, clearance),
                tag: pl.tag(MoveKind::Traverse),
            });
        }
    } else if let (Some(previous), Some(start)) =
        (last_position(program), first_position(&pl.fragment))
    {
        // Operations may now have different safety planes. Lift vertically before
        // crossing XY, then descend at the destination; never traverse diagonally
        // upward from a lower operation's clearance into a taller workpiece.
        if (previous.z - clearance).abs() > 1e-9 {
            let travel_z = previous.z.max(clearance).max(st.last_clearance.unwrap_or(previous.z));
            let tag = pl.tag(MoveKind::Traverse);
            program.push(Step::Rapid {
                to: Point3::new(previous.x, previous.y, travel_z),
                tag,
            });
            program.push(Step::Rapid {
                to: Point3::new(start.x, start.y, travel_z),
                tag,
            });
            program.push(Step::Rapid {
                to: Point3::new(start.x, start.y, clearance),
                tag,
            });
        }
    }
    program.extend(pl.fragment.clone());
    st.last_clearance = Some(clearance);
    if let Some(t) = pl.ends_with {
        st.current_tool = Some(t);
    }
}

/// Bring the machine to a state a person can safely approach: spindle stopped,
/// coolant off, tool lifted clear of the work.
///
/// **One function, two callers, because they are one physical situation** — the
/// re-fixture halt and the end of the program both end with the operator reaching in,
/// to flip the part or to take it out. They were written eight weeks apart and had
/// drifted: the end of the program stopped coolant before the spindle and never
/// lifted at all, so a finished job left the tool sitting at clearance over the part.
/// Neither was wrong in isolation; nothing had ever emitted both in one program.
///
/// **Spindle before coolant**, deliberately: the spindle is the half that hurts, so it
/// is the half that stops first.
fn shut_down(program: &mut Program, tool_change_height: f64, op_id: u32) {
    program.push(Step::SpindleOff);
    program.push(Step::Coolant(Coolant::Off));
    lift_to_tool_change(program, tool_change_height, op_id);
}

/// Rapid straight up to the tool-change height, in place (X/Y unchanged), so a tool
/// change or manual re-fixture happens with the tool clear of the work. Tagged
/// [`MoveKind::Traverse`] so the backplot renders it apart from an operation's own
/// rapids. A no-op if nothing has moved yet — there is no prior position to lift from.
///
/// **Never descends.** The name is a promise: a "lift" that moved the tool down would
/// be the exact opposite of what every caller wants it for. `build_job` already raises
/// a below-clearance tool-change height, so this is the second line of defence rather
/// than the first — but a lift that can go the wrong way is not worth leaving to one
/// guard.
fn lift_to_tool_change(program: &mut Program, z: f64, op_id: u32) {
    if let Some(p) = last_position(program) {
        if p.z >= z {
            return;
        }
        program.push(Step::Rapid {
            to: Point3::new(p.x, p.y, z),
            tag: Tag::new(op_id, MoveKind::Traverse),
        });
    }
}

/// The tool's current position: the endpoint of the last positional step emitted so
/// far. A drill cycle ends above its last hole, at that cycle's retract plane.
fn last_position(program: &Program) -> Option<Point3> {
    program.steps().iter().rev().find_map(|s| match s {
        Step::Rapid { to, .. } | Step::Linear { to, .. } => Some(*to),
        Step::Arc { end, .. } => Some(*end),
        Step::Drill(c) => c.points.last().map(|[x, y]| Point3::new(*x, *y, c.retract)),
        _ => None,
    })
}

/// The first point an operation's fragment moves to — its approach start. Used to aim
/// the tool-change cross-and-descend at the next operation before handing off.
fn first_position(program: &Program) -> Option<Point3> {
    program.steps().iter().find_map(|s| match s {
        Step::Rapid { to, .. } | Step::Linear { to, .. } => Some(*to),
        Step::Arc { end, .. } => Some(*end),
        Step::Drill(c) => c
            .points
            .first()
            .map(|[x, y]| Point3::new(*x, *y, c.retract)),
        _ => None,
    })
}

/// Dispatch an operation to its strategy and compute it.
fn compute(operation: &Operation, env: &JobEnv, cancel: &CancelToken) -> StrategyResult {
    match operation {
        Operation::Profile(op) => ProfileStrategy::new(op.clone()).compute(env, cancel),
        Operation::Drill(op) => DrillStrategy::new(op.clone()).compute(env, cancel),
        Operation::Pocket(op) => PocketStrategy::new(op.clone()).compute(env, cancel),
        Operation::Face(op) => FaceStrategy::new(op.clone()).compute(env, cancel),
        Operation::Chamfer(op) => ChamferStrategy::new(op.clone()).compute(env, cancel),
        Operation::Thread(op) => ThreadStrategy::new(op.clone()).compute(env, cancel),
        Operation::Engrave(op) => EngraveStrategy::new(op.clone()).compute(env, cancel),
        Operation::Carve(op) => CarveStrategy::new(op.clone()).compute(env, cancel),
    }
}

/// The tool left in the spindle by `fragment` — the last [`Step::ToolChange`] it emits,
/// or `None` if it emits none (the single-tool case, which is every operation but Carve).
fn last_tool_change(fragment: &Program) -> Option<u32> {
    fragment.steps().iter().rev().find_map(|s| match s {
        Step::ToolChange { tool } => Some(*tool),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cam_model::{DrillOp, Heights, Setup, Stock, Tool, ToolKind};

    fn document(operations: u32) -> Document {
        Document::new(Setup {
            id: 0,
            frame: Default::default(),
            material: String::new(),
            material_id: None,
            model_instances: Vec::new(),
            program_name: String::new(),
            name: "completion test".into(),
            heights: Heights::new(5.0, 2.0, 0.0),
            stock: Stock::BoundingBox {
                x_offset: 0.0,
                y_offset: 0.0,
                top: 0.0,
                thickness: 10.0,
            },
            tools: vec![Tool {
                number: 1,
                diameter: 3.0,
                length: 30.0,
                kind: ToolKind::Drill {
                    point_angle_deg: 118.0,
                },
                ..Default::default()
            }],
            operations: (0..operations)
                .map(|id| {
                    Operation::Drill(DrillOp {
                        id,
                        tool: 1,
                        points: vec![[10.0 + id as f64, 10.0]],
                        depth: 1.0,
                        start_offset: 0.0,
                        peck: None,
                        dwell: None,
                        spindle_rpm: 1000.0,
                        tool_preset: None,
                        entry_feeds: Default::default(),
                        work_offset: 1,
                        feed: 100.0,
                    })
                })
                .collect(),
            patterns: Vec::new(),
            origin: [0.0; 3],
            extra_origins: Vec::new(),
            origin_index: 1,
            operation_heights: Default::default(),
            height_settings: Default::default(),
            tool_change_height: None,
        })
    }

    #[test]
    fn rectangular_pattern_expands_row_major_with_each_seed_once_and_instance_provenance() {
        let mut doc = document(2);
        doc.setup_mut().patterns.push(cam_model::OperationPattern {
            id: 77, name: "Grid".into(), members: vec![0, 1],
            kind: cam_model::PatternKind::Rectangular { nx: 3, ny: 2, dx: 20.0, dy: -15.0 },
            order: cam_model::PatternOrder::ByInstance, suppressed: false,
        });
        let mut cache = crate::OperationCache::default();
        let request = crate::PlanRequest { document: &doc, spindle_rpm: 1000.0, dir: SpindleDir::Cw, stock: None, tool_change_height: 50.0 };
        let result = crate::build_job_cached(request, &CancelToken::new(), &mut cache, |_| {});
        assert_eq!(result.completion, JobCompletion::Complete, "{:?}", result.diagnostics);
        assert_eq!(cache.stats.computed, 2, "each source fragment is generated once");
        let holes: Vec<_> = result.program.steps().iter().filter_map(|step| match step { Step::Drill(c) => Some((c.points[0], c.tag)), _ => None }).collect();
        assert_eq!(holes.len(), 12);
        let expected = [[10.,10.],[11.,10.],[30.,10.],[31.,10.],[50.,10.],[51.,10.],[10.,-5.],[11.,-5.],[30.,-5.],[31.,-5.],[50.,-5.],[51.,-5.]];
        for ((point, tag), [x,y]) in holes.iter().zip(expected) {
            assert!((point[0]-x).abs() < 1e-9 && (point[1]-y).abs() < 1e-9);
            assert_eq!(tag.pattern_id, Some(77));
            assert!(tag.instance_index.is_some());
        }
    }

    #[test]
    fn suppressed_pattern_skips_seed_generation_and_bad_membership_fails_headless_planning() {
        let mut doc = document(1);
        doc.setup_mut().patterns.push(cam_model::OperationPattern {
            id: 78, name: "Off".into(), members: vec![0],
            kind: cam_model::PatternKind::Circular { center_xy: [1.0, 1.0], count: 4, distribution: cam_model::CircularDistribution::Full, direction: cam_model::PatternDirection::Ccw },
            order: cam_model::PatternOrder::ByInstance, suppressed: true,
        });
        let mut cache = crate::OperationCache::default();
        let request = crate::PlanRequest { document: &doc, spindle_rpm: 1000.0, dir: SpindleDir::Cw, stock: None, tool_change_height: 50.0 };
        let result = crate::build_job_cached(request, &CancelToken::new(), &mut cache, |_| {});
        assert_eq!(result.completion, JobCompletion::Complete);
        assert_eq!(cache.stats.computed, 0);
        doc.setup_mut().patterns[0].members.push(404);
        let result = build_job(&doc, 1000.0, SpindleDir::Cw, None, 50.0, &CancelToken::new());
        assert_eq!(result.completion, JobCompletion::Failed);
        assert!(result.diagnostics[0].message.contains("missing operation 404"));
    }

    #[test]
    fn circular_pattern_rotates_drill_cycles_about_nonzero_center_and_by_operation_orders_instances() {
        let mut doc = document(1);
        doc.setup_mut().patterns.push(cam_model::OperationPattern {
            id: 79, name: "Rotor".into(), members: vec![0],
            kind: cam_model::PatternKind::Circular {
                center_xy: [5.0, 5.0], count: 4,
                distribution: cam_model::CircularDistribution::Full,
                direction: cam_model::PatternDirection::Ccw,
            },
            order: cam_model::PatternOrder::ByInstance, suppressed: false,
        });
        let result = build_job(&doc, 1000.0, SpindleDir::Cw, None, 50.0, &CancelToken::new());
        assert_eq!(result.completion, JobCompletion::Complete, "{:?}", result.diagnostics);
        let cycles: Vec<_> = result.program.steps().iter().filter_map(|step| match step {
            Step::Drill(cycle) => Some((cycle.points[0], cycle.tag.instance_index)), _ => None,
        }).collect();
        assert_eq!(cycles.len(), 4);
        assert_eq!(cycles[0].0, [10.0, 10.0]);
        assert!((cycles[1].0[0] - 0.0).abs() < 1e-9);
        assert!((cycles[1].0[1] - 10.0).abs() < 1e-9);
        assert_eq!(cycles.iter().map(|(_, index)| *index).collect::<Vec<_>>(), vec![Some(0), Some(1), Some(2), Some(3)]);

        let mut ordered = document(2);
        ordered.setup_mut().patterns.push(cam_model::OperationPattern {
            id: 80, name: "Operations".into(), members: vec![0, 1],
            kind: cam_model::PatternKind::Rectangular { nx: 2, ny: 1, dx: 3.0, dy: 0.0 },
            order: cam_model::PatternOrder::ByOperation, suppressed: false,
        });
        let result = build_job(&ordered, 1000.0, SpindleDir::Cw, None, 50.0, &CancelToken::new());
        let ownership: Vec<_> = result.program.steps().iter().filter_map(|step| match step {
            Step::Drill(cycle) => Some((cycle.tag.op_id, cycle.tag.instance_index)), _ => None,
        }).collect();
        assert_eq!(ownership, vec![(0, Some(0)), (0, Some(1)), (1, Some(0)), (1, Some(1))]);
    }

    #[test]
    fn reference_heights_drive_cutting_limits_and_independent_feed_with_pecks_and_dwell() {
        use cam_model::{HeightPlaneKind as P, HeightReference as R, OperationHeightSettings};
        let mut doc = document(1);
        if let Operation::Drill(op) = &mut doc.setup_mut().operations[0] {
            op.depth = 4.;
            op.peck = Some(1.);
            op.dwell = Some(0.25);
        }
        let mut settings = OperationHeightSettings::default();
        settings.definition_mut(P::Feed).offset = 2.;
        settings.definition_mut(P::Bottom).offset = -0.5;
        doc.setup_mut().height_settings.insert(0, settings);
        let result = build_job(&doc, 1000., SpindleDir::Cw, None, 50., &CancelToken::new());
        assert_eq!(
            result.completion,
            JobCompletion::Complete,
            "{:?}",
            result.diagnostics
        );
        assert!(!result
            .program
            .steps()
            .iter()
            .any(|s| matches!(s, Step::Drill(_))));
        assert!(result
            .program
            .steps()
            .iter()
            .any(|s| matches!(s,Step::Linear{to,..} if to.z == -4.5)));
        assert!(result
            .program
            .steps()
            .iter()
            .any(|s| matches!(s,Step::Dwell{seconds} if *seconds == 0.25)));
        let mut previous = None;
        for step in result.program.steps() {
            match step {
                Step::Rapid { to, .. } => {
                    if let Some(from) = previous {
                        assert!(
                            to.z >= from || to.z >= 2.,
                            "rapid descended below feed height"
                        );
                    }
                    previous = Some(to.z);
                }
                Step::Linear { to, .. } => previous = Some(to.z),
                _ => {}
            }
        }
        doc.setup_mut()
            .height_settings
            .get_mut(&0)
            .unwrap()
            .definition_mut(P::Feed)
            .reference = R::Disabled;
        let result = build_job(&doc, 1000., SpindleDir::Cw, None, 50., &CancelToken::new());
        assert_eq!(result.completion, JobCompletion::Complete);
        assert!(result
            .program
            .steps()
            .iter()
            .any(|s| matches!(s,Step::Drill(c) if c.depth == -4.5)));
    }

    #[test]
    fn feed_height_edits_reuse_strategy_fragments_but_rebuild_the_approach() {
        use cam_model::{HeightPlaneKind as P, OperationHeightSettings};
        let mut doc = document(1);
        let mut settings = OperationHeightSettings::default();
        settings.definition_mut(P::Feed).offset = 2.;
        doc.setup_mut().height_settings.insert(0, settings);
        let mut cache = crate::OperationCache::default();
        fn request(doc: &Document) -> PlanRequest<'_> {
            PlanRequest {
                document: doc,
                spindle_rpm: 1000.,
                dir: SpindleDir::Cw,
                stock: None,
                tool_change_height: 50.,
            }
        }
        let first = build_job_cached(request(&doc), &CancelToken::new(), &mut cache, |_| {});
        assert_eq!(first.completion, JobCompletion::Complete);
        assert_eq!(cache.stats.computed, 1);
        doc.setup_mut()
            .height_settings
            .get_mut(&0)
            .unwrap()
            .definition_mut(P::Feed)
            .offset = 3.;
        let next = build_job_cached(request(&doc), &CancelToken::new(), &mut cache, |_| {});
        assert_eq!(next.completion, JobCompletion::Complete);
        assert_eq!(cache.stats.reused, 1);
        assert_ne!(first.program, next.program);
    }

    #[test]
    fn operation_planes_reach_strategies_and_safe_inter_operation_links() {
        let mut doc = document(2);
        doc.setup_mut()
            .operation_heights
            .insert(1, Heights::new(20., 12., 3.));
        let mut seen = Vec::new();
        let result = build_job_with_compute(
            &doc,
            1000.,
            SpindleDir::Cw,
            None,
            10.,
            &CancelToken::new(),
            |op, env, _| {
                seen.push((op.id(), env.heights));
                let mut program = Program::new();
                program.push(Step::Rapid {
                    to: Point3::new(op.id() as f64 * 10., 0., env.heights.clearance),
                    tag: Tag::new(op.id(), MoveKind::Link),
                });
                StrategyResult {
                    program,
                    ..Default::default()
                }
            },
        );
        assert_eq!(result.completion, JobCompletion::Complete);
        assert_eq!(
            seen,
            [
                (0, Heights::new(5., 2., 0.)),
                (1, Heights::new(20., 12., 3.))
            ]
        );
        let rapids: Vec<_> = result
            .program
            .steps()
            .iter()
            .filter_map(|s| match s {
                Step::Rapid { to, .. } => Some(*to),
                _ => None,
            })
            .collect();
        // The tool-change height is raised above *all* operation clearances.
        assert_eq!(rapids[0].z, 20.);
        // Before crossing from op 0 to taller op 1, lift at the previous XY.
        assert!(rapids.windows(2).any(
            |pair| pair[0] == Point3::new(0., 0., 20.) && pair[1] == Point3::new(10., 0., 20.)
        ));
    }

    #[test]
    fn invalid_operation_planes_fail_before_strategy_computation() {
        let mut doc = document(1);
        doc.setup_mut()
            .operation_heights
            .insert(0, Heights::new(1., 2., 0.));
        let result = build_job_with_compute(
            &doc,
            1000.,
            SpindleDir::Cw,
            None,
            50.,
            &CancelToken::new(),
            |_, _, _| panic!("invalid planes must not reach a strategy"),
        );
        assert_eq!(result.completion, JobCompletion::Failed);
        assert!(result.diagnostics.iter().any(|d| d.op == Some(0)));
    }

    #[test]
    fn headless_planning_honors_persistent_manufacturing_suppression() {
        let mut doc = document(2);
        doc.excluded_operations.insert(0);
        let result = build_job(&doc, 1000., SpindleDir::Cw, None, 50., &CancelToken::new());
        assert_eq!(result.completion, JobCompletion::Complete);
        assert!(!result
            .program
            .steps()
            .iter()
            .any(|step| matches!(step, Step::Drill(cycle) if cycle.tag.op_id == 0)));
        assert!(result
            .program
            .steps()
            .iter()
            .any(|step| matches!(step, Step::Drill(cycle) if cycle.tag.op_id == 1)));
        doc.disabled_origins
            .entry(doc.setup().id)
            .or_default()
            .insert(1);
        let result = build_job(&doc, 1000., SpindleDir::Cw, None, 50., &CancelToken::new());
        assert_eq!(result.completion, JobCompletion::Complete);
        assert!(!result
            .program
            .steps()
            .iter()
            .any(|step| matches!(step, Step::Drill(_))));
    }

    fn fragment(operation: &Operation) -> StrategyResult {
        let mut program = Program::new();
        program.push(Step::Rapid {
            to: Point3::new(10.0 + operation.id() as f64, 10.0, 5.0),
            tag: Tag::new(operation.id(), MoveKind::Link),
        });
        program.push(Step::Linear {
            to: Point3::new(10.0 + operation.id() as f64, 10.0, -1.0),
            feed: 100.0,
            tag: Tag::new(operation.id(), MoveKind::Cutting),
        });
        StrategyResult {
            program,
            ..Default::default()
        }
    }

    fn contains_cut(job: &JobResult, op: u32) -> bool {
        job.program.steps().iter().any(|step| {
            matches!(step, Step::Linear { tag, .. }
                if tag.op_id == op && tag.kind == MoveKind::Cutting)
        })
    }

    #[test]
    fn linked_operation_without_binding_fails_instead_of_using_cached_contour() {
        let mut doc = document(1);
        doc.cad_linked_operations.insert(0);
        let job = build_job_with_compute(
            &doc,
            1000.0,
            SpindleDir::Cw,
            None,
            50.0,
            &CancelToken::new(),
            |op, _, _| fragment(op),
        );
        assert_eq!(job.completion, JobCompletion::Failed);
        assert!(!contains_cut(&job, 0));
        assert!(job
            .diagnostics
            .iter()
            .any(|d| d.message.contains("binding is missing")));
    }

    #[test]
    fn invalid_motion_from_a_strategy_fails_the_job_with_operation_provenance() {
        let job = build_job_with_compute(
            &document(1),
            1000.0,
            SpindleDir::Cw,
            None,
            50.0,
            &CancelToken::new(),
            |op, _, _| {
                let mut result = fragment(op);
                if let Step::Linear { feed, .. } = &mut result.program.steps[1] {
                    *feed = f64::NAN;
                }
                result
            },
        );
        assert_eq!(job.completion, JobCompletion::Failed);
        assert_eq!(job.diagnostics.len(), 1);
        assert_eq!(job.diagnostics[0].op, Some(0));
        assert!(job.diagnostics[0].message.contains("feed must be finite"));
    }

    #[test]
    fn pre_cancelled_jobs_never_compute_an_operation() {
        let token = CancelToken::new();
        token.cancel();
        let job = build_job_with_compute(
            &document(1),
            1000.0,
            SpindleDir::Cw,
            None,
            50.0,
            &token,
            |_, _, _| panic!("cancelled work must not begin"),
        );
        assert_eq!(job.completion, JobCompletion::Cancelled);
        assert!(!contains_cut(&job, 0));
    }

    #[test]
    fn strategy_cancellation_discards_its_fragment_and_stops_later_operations() {
        let token = CancelToken::new();
        let mut called = Vec::new();
        let job = build_job_with_compute(
            &document(2),
            1000.0,
            SpindleDir::Cw,
            None,
            50.0,
            &token,
            |op, _, _| {
                called.push(op.id());
                StrategyResult {
                    cancelled: true,
                    ..fragment(op)
                }
            },
        );
        assert!(
            !token.is_cancelled(),
            "the strategy's own flag must propagate"
        );
        assert_eq!(called, [0]);
        assert_eq!(job.completion, JobCompletion::Cancelled);
        assert!(!contains_cut(&job, 0));
        assert!(!contains_cut(&job, 1));
    }

    #[test]
    fn cancellation_during_the_last_operation_retains_only_completed_fragments() {
        let token = CancelToken::new();
        let job = build_job_with_compute(
            &document(2),
            1000.0,
            SpindleDir::Cw,
            None,
            50.0,
            &token,
            |op, _, token| {
                if op.id() == 1 {
                    token.cancel();
                }
                // The planner must observe the token even if a strategy misses it.
                fragment(op)
            },
        );
        assert_eq!(job.completion, JobCompletion::Cancelled);
        assert!(contains_cut(&job, 0));
        assert!(!contains_cut(&job, 1));
    }

    #[test]
    fn failed_fragments_are_omitted_while_diagnostics_and_other_operations_continue() {
        let mut called = Vec::new();
        let job = build_job_with_compute(
            &document(2),
            1000.0,
            SpindleDir::Cw,
            None,
            50.0,
            &CancelToken::new(),
            |op, _, _| {
                called.push(op.id());
                let mut result = fragment(op);
                if op.id() == 0 {
                    result
                        .diagnostics
                        .push(Diagnostic::error("cannot finish this operation"));
                }
                result
            },
        );
        assert_eq!(called, [0, 1]);
        assert_eq!(job.completion, JobCompletion::Failed);
        assert!(!contains_cut(&job, 0));
        assert!(contains_cut(&job, 1));
        assert_eq!(job.diagnostics[0].op, Some(0));
    }

    #[test]
    fn warnings_do_not_make_successful_empty_operations_incomplete() {
        let job = build_job_with_compute(
            &document(1),
            1000.0,
            SpindleDir::Cw,
            None,
            50.0,
            &CancelToken::new(),
            |_, _, _| StrategyResult {
                diagnostics: vec![Diagnostic::warning("nothing to cut")],
                ..Default::default()
            },
        );
        assert_eq!(job.completion, JobCompletion::Complete);
        assert!(!contains_cut(&job, 0));
        assert_eq!(job.diagnostics[0].op, Some(0));
    }

    #[test]
    fn empty_jobs_are_complete_unless_cancelled() {
        let token = CancelToken::new();
        let empty = document(0);
        let job = build_job(&empty, 1000.0, SpindleDir::Cw, None, 50.0, &token);
        assert_eq!(job.completion, JobCompletion::Complete);
        token.cancel();
        let job = build_job(&empty, 1000.0, SpindleDir::Cw, None, 50.0, &token);
        assert_eq!(job.completion, JobCompletion::Cancelled);
    }
}
