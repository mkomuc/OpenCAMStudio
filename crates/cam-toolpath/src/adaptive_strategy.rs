//! 3D adaptive roughing over conservative CAD-derived layer shadows.
use cam_cldata::Program;
use cam_geo::{difference, offset, Contour, JoinStyle, Point, Polygon};
use cam_geometry3d::AdaptiveGeometry;
use cam_model::{AdaptiveOp, Clearing, Lead, Plunge, StockToLeave};

use crate::{CancelToken, Diagnostic, JobEnv, Strategy, StrategyResult};

#[derive(Clone, Debug)]
pub struct AdaptiveStrategy {
    op: AdaptiveOp,
    stock_to_leave: StockToLeave,
    geometry: AdaptiveGeometry,
}

impl AdaptiveStrategy {
    pub fn new(op: AdaptiveOp, stock_to_leave: StockToLeave, geometry: AdaptiveGeometry) -> Self {
        Self {
            op,
            stock_to_leave,
            geometry,
        }
    }
}

impl Strategy for AdaptiveStrategy {
    fn name(&self) -> &str {
        "adaptive"
    }

    fn compute(&self, env: &JobEnv, cancel: &CancelToken) -> StrategyResult {
        let op = &self.op;
        let mut diagnostics = Vec::new();
        macro_rules! fail { ($($arg:tt)*) => {{ diagnostics.push(Diagnostic::error(format!($($arg)*))); return StrategyResult { diagnostics, ..Default::default() }; }} }
        if let Err(reason) = op.entry_feeds.validate() {
            fail!("operation {}: {reason}", op.id);
        }
        if !op.params.tolerance.is_finite() || op.params.tolerance <= 0.0 {
            fail!("operation {}: tolerance must be finite and positive", op.id);
        }
        if !op.params.optimal_load.is_finite() || op.params.optimal_load <= 0.0 {
            fail!(
                "operation {}: optimal load must be finite and positive",
                op.id
            );
        }
        if !op.params.stepdown.is_finite()
            || op.params.stepdown <= 0.0
            || !op.params.fine_stepdown.is_finite()
            || op.params.fine_stepdown <= 0.0
        {
            fail!(
                "operation {}: roughing and fine stepdown must be finite and positive",
                op.id
            );
        }
        if !op.params.feed.is_finite()
            || op.params.feed <= 0.0
            || !op.params.plunge_feed.is_finite()
            || op.params.plunge_feed <= 0.0
        {
            fail!(
                "operation {}: cutting and plunge feeds must be finite and positive",
                op.id
            );
        }
        if self.stock_to_leave.enabled
            && (!self.stock_to_leave.radial.is_finite()
                || self.stock_to_leave.radial < 0.0
                || !self.stock_to_leave.axial.is_finite()
                || self.stock_to_leave.axial < 0.0)
        {
            fail!(
                "operation {}: stock allowance must be finite and nonnegative",
                op.id
            );
        }
        let Some(tool) = env.tool(op.tool) else {
            fail!("operation {}: tool {} is not in the setup", op.id, op.tool);
        };
        if self.geometry.layers.is_empty() {
            fail!(
                "operation {}: adaptive geometry has no cutting levels",
                op.id
            );
        }
        let z_top = self
            .geometry
            .layers
            .iter()
            .map(|l| l.z)
            .fold(f64::NEG_INFINITY, f64::max);
        let z_bottom = self
            .geometry
            .layers
            .iter()
            .map(|l| l.z)
            .fold(f64::INFINITY, f64::min);
        if !crate::guards::check_side_milling(
            op.id,
            "3D adaptive",
            tool,
            z_top - z_bottom,
            &mut diagnostics,
        ) {
            return StrategyResult {
                diagnostics,
                ..Default::default()
            };
        }
        if !crate::guards::check_flat_floor(op.id, "3D adaptive", tool, &mut diagnostics) {
            return StrategyResult {
                diagnostics,
                ..Default::default()
            };
        }
        if !crate::guards::check_plunge(op.id, "3D adaptive", tool, &mut diagnostics) {
            return StrategyResult {
                diagnostics,
                ..Default::default()
            };
        }
        if op.params.optimal_load > tool.diameter {
            fail!(
                "operation {}: optimal load cannot exceed the tool diameter",
                op.id
            );
        }
        let (stock_min, stock_max) = match env.stock {
            Some(bounds) => bounds,
            None => fail!(
                "operation {}: 3D Adaptive requires defined stock bounds",
                op.id
            ),
        };
        if !stock_min.into_iter().chain(stock_max).all(f64::is_finite)
            || stock_min[0] >= stock_max[0]
            || stock_min[1] >= stock_max[1]
        {
            fail!("operation {}: stock XY bounds are invalid", op.id);
        }
        let stock_region = match Polygon::new(Contour::new(vec![
            Point::new(stock_min[0], stock_min[1]),
            Point::new(stock_max[0], stock_min[1]),
            Point::new(stock_max[0], stock_max[1]),
            Point::new(stock_min[0], stock_max[1]),
        ])) {
            Ok(p) => p,
            Err(e) => fail!("operation {}: invalid stock bounds: {e}", op.id),
        };

        let largest_radius = (tool.diameter.max(tool.shank_dia()).max(tool.neck_dia())) / 2.0;
        let extra_shank = largest_radius - tool.radius();
        let allowance = if self.stock_to_leave.enabled {
            self.stock_to_leave.radial
        } else {
            0.0
        };
        let protected_growth = extra_shank + allowance + self.geometry.error_bound;
        let mut program = Program::new();
        let mut cleared_levels = 0usize;
        for layer in &self.geometry.layers {
            if cancel.is_cancelled() {
                return StrategyResult {
                    program,
                    diagnostics,
                    cancelled: true,
                };
            }
            if !layer.z.is_finite() {
                fail!(
                    "operation {}: adaptive geometry contains a nonfinite level",
                    op.id
                );
            }
            // A toolpath above stock cannot remove material. In particular,
            // do not count a selected face's entry plane as a successful pass
            // when every reachable level is protected or outside its domain.
            if layer.z >= env.heights.top_of_stock - 1e-9 {
                continue;
            }
            let safe = if layer.protected.is_empty() {
                Vec::new()
            } else {
                match offset(&layer.protected, protected_growth, JoinStyle::Round) {
                    Ok(v) => v,
                    Err(e) => fail!(
                        "operation {}: could not create the cutter safety envelope: {e}",
                        op.id
                    ),
                }
            };
            let domains = if layer.body_domains.is_empty() && layer.face_domains.is_empty() {
                vec![stock_region.clone()]
            } else {
                let mut selected_domains = Vec::new();
                if !layer.body_domains.is_empty() {
                    match offset(
                        &layer.body_domains,
                        2.0 * tool.diameter
                            - self.geometry.error_bound
                            - 2.0 * cam_geo::GRID_MM,
                        JoinStyle::Round,
                    ) {
                        Ok(regions) => selected_domains.extend(regions),
                        Err(e) => fail!("operation {}: could not expand the selected body machining domain: {e}", op.id),
                    }
                }
                if !layer.face_domains.is_empty() {
                    match offset(
                        &layer.face_domains,
                        -self.geometry.error_bound - 2.0 * cam_geo::GRID_MM,
                        JoinStyle::Round,
                    ) {
                        Ok(regions) => selected_domains.extend(regions),
                        Err(e) => fail!("operation {}: could not erode the selected face machining domain by the certified path error: {e}", op.id),
                    }
                }
                match cam_geo::intersection(&selected_domains, std::slice::from_ref(&stock_region))
                {
                    Ok(regions) => regions,
                    Err(e) => fail!(
                        "operation {}: could not clip selected machining domains to stock: {e}",
                        op.id
                    ),
                }
            };
            let waste = match difference(&domains, &safe) {
                Ok(v) => v,
                Err(e) => fail!(
                    "operation {}: could not derive the layer clearing area: {e}",
                    op.id
                ),
            };
            for region in &waste {
                if cancel.is_cancelled() {
                    return StrategyResult {
                        program,
                        diagnostics,
                        cancelled: true,
                    };
                }
                let radius = tool.radius();
                let spacing = op.params.optimal_load.min(tool.diameter);
                let guard = offset(std::slice::from_ref(region), -radius, JoinStyle::Round)
                    .unwrap_or_default();
                let fine_pitch = op.params.fine_stepdown.min(op.params.stepdown);
                let job = crate::clearing::ClearJob {
                    id: op.id,
                    radius,
                    finish: 0.0,
                    first: radius,
                    spacing,
                    clearing: Clearing {
                        engagement: op.params.optimal_load,
                        climb: op.params.climb,
                    },
                    spindle: env.spindle,
                    plunge: Plunge::Helix {
                        radius: (radius * 0.5).max(0.1),
                        pitch: fine_pitch,
                    },
                    feed: op.params.feed,
                    plunge_feed: op.params.plunge_feed,
                    entry_feeds: op.entry_feeds,
                    lead_overlap: 0.0,
                    lead_in: Lead::None,
                    lead_out: Lead::None,
                    start: None,
                    guard: &guard,
                };
                match crate::clearing::clear(
                    &mut program,
                    region,
                    &job,
                    &env.heights,
                    &[layer.z],
                    cancel,
                ) {
                    Ok(n) => {
                        if n > 0 {
                            cleared_levels += 1;
                        }
                    }
                    Err(crate::rings::RingsError::Cancelled) => {
                        return StrategyResult {
                            program,
                            diagnostics,
                            cancelled: true,
                        }
                    }
                    Err(e) => fail!(
                        "operation {}: adaptive layer clearing failed at Z{:.3}: {e:?}",
                        op.id,
                        layer.z
                    ),
                }
            }
        }
        if cleared_levels == 0 {
            fail!(
                "operation {}: no accessible stock was clearable in the selected machining region below the stock top",
                op.id
            );
        }
        diagnostics.push(Diagnostic::warning("3D Adaptive uses certified CAD projection shadows and the existing 2D adaptive clearing engine; where that engine cannot certify a constant-engagement path it falls back to concentric rings. Toolholder geometry is not modeled."));
        StrategyResult {
            program,
            diagnostics,
            cancelled: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cam_cldata::{SpindleDir, Step};
    use cam_geo::{Containment, JoinStyle};
    use cam_model::{Heights, Tool, ToolKind};

    fn fixture() -> (AdaptiveStrategy, Tool) {
        let tool = Tool {
            number: 1,
            diameter: 6.0,
            length: 30.0,
            flute_length: 20.0,
            flutes: 3,
            kind: ToolKind::EndMill,
            ..Default::default()
        };
        let part = Polygon::new(Contour::new(vec![
            Point::new(10., 10.),
            Point::new(20., 10.),
            Point::new(20., 20.),
            Point::new(10., 20.),
        ]))
        .unwrap();
        let geometry = AdaptiveGeometry {
            layers: vec![
                cam_geometry3d::AdaptiveLayer {
                    z: 8.,
                    protected: vec![part.clone()],
                    body_domains: vec![],
                    face_domains: vec![],
                },
                cam_geometry3d::AdaptiveLayer {
                    z: 4.,
                    protected: vec![part],
                    body_domains: vec![],
                    face_domains: vec![],
                },
            ],
            fingerprint: "fixture".into(),
            error_bound: 0.02,
        };
        let mut op = AdaptiveOp {
            id: 3,
            tool: 1,
            spindle_rpm: 6000.,
            tool_preset: None,
            cooling: Default::default(),
            entry_feeds: Default::default(),
            work_offset: 1,
            params: Default::default(),
        };
        op.params.optimal_load = 2.0;
        op.params.stepdown = 4.0;
        op.params.fine_stepdown = 1.0;
        op.params.feed = 300.;
        op.params.plunge_feed = 100.;
        (
            AdaptiveStrategy::new(
                op,
                StockToLeave {
                    enabled: true,
                    radial: 0.5,
                    axial: 0.25,
                },
                geometry,
            ),
            tool,
        )
    }

    #[test]
    fn layer_path_stays_outside_cad_shadow_with_cutter_and_stock_allowance() {
        let (strategy, tool) = fixture();
        let env = JobEnv {
            heights: Heights::new(15., 12., 10.),
            tools: std::slice::from_ref(&tool),
            stock: Some(([0., 0.], [30., 30.])),
            spindle: SpindleDir::Cw,
        };
        let result = strategy.compute(&env, &CancelToken::new());
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        assert!(!result.program.steps().is_empty());
        let protected = offset(
            &[Polygon::new(Contour::new(vec![
                Point::new(10., 10.),
                Point::new(20., 10.),
                Point::new(20., 20.),
                Point::new(10., 20.),
            ]))
            .unwrap()],
            tool.radius() + 0.5 + 0.02,
            JoinStyle::Round,
        )
        .unwrap();
        let mut cuts = 0;
        for step in result.program.steps() {
            let p = match step {
                Step::Linear { to, .. } => Some([to.x, to.y]),
                Step::Arc { end, .. } => Some([end.x, end.y]),
                _ => None,
            };
            if let Some([x, y]) = p {
                if matches!(step, Step::Linear { .. } | Step::Arc { .. }) {
                    cuts += 1;
                    assert!(
                        protected
                            .iter()
                            .all(|poly| poly.locate(Point::new(x, y)) == Containment::Outside),
                        "cut motion enters protected stock shadow at ({x},{y})"
                    );
                }
            }
        }
        assert!(cuts > 0, "expected emitted cutting moves");
    }

    #[test]
    fn selected_face_domain_contains_sampled_lines_and_arc_refits() {
        let (mut strategy, tool) = fixture();
        let face = Polygon::new(Contour::new(vec![
            Point::new(0., 0.),
            Point::new(60., 0.),
            Point::new(60., 60.),
            Point::new(0., 60.),
        ]))
        .unwrap();
        for layer in &mut strategy.geometry.layers {
            layer.body_domains.clear();
            layer.face_domains = vec![face.clone()];
        }
        let env = JobEnv {
            heights: Heights::new(15., 12., 10.),
            tools: std::slice::from_ref(&tool),
            stock: Some(([0., 0.], [60., 60.])),
            spindle: SpindleDir::Cw,
        };
        let result = strategy.compute(&env, &CancelToken::new());
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        let center_domain = offset(
            std::slice::from_ref(&face),
            -(tool.radius() + strategy.geometry.error_bound + 2.0 * cam_geo::GRID_MM),
            JoinStyle::Round,
        )
        .unwrap();
        let mut current = None;
        let mut sampled = 0usize;
        let mut arc_samples = 0usize;
        for step in result.program.steps() {
            match step {
                Step::Rapid { to, .. } => current = Some(*to),
                Step::Linear { to, .. } => {
                    if let Some(start) = current {
                        for i in 1..=20 {
                            let t = i as f64 / 20.0;
                            let p = Point::new(
                                start.x + (to.x - start.x) * t,
                                start.y + (to.y - start.y) * t,
                            );
                            assert!(
                                center_domain
                                    .iter()
                                    .any(|poly| poly.locate(p) != Containment::Outside),
                                "linear cut leaves selected face center domain at {p:?}"
                            );
                            sampled += 1;
                        }
                    }
                    current = Some(*to);
                }
                Step::Arc {
                    end, center, dir, ..
                } => {
                    if let Some(start) = current {
                        let start_angle = (start.y - center.y).atan2(start.x - center.x);
                        let end_angle = (end.y - center.y).atan2(end.x - center.x);
                        let raw = end_angle - start_angle;
                        let sweep = match dir {
                            cam_cldata::ArcDir::Ccw => raw.rem_euclid(std::f64::consts::TAU),
                            cam_cldata::ArcDir::Cw => -(-raw).rem_euclid(std::f64::consts::TAU),
                        };
                        for i in 1..=40 {
                            let t = i as f64 / 40.0;
                            let angle = start_angle + sweep * t;
                            let p = Point::new(
                                center.x
                                    + (start.x - center.x).hypot(start.y - center.y) * angle.cos(),
                                center.y
                                    + (start.x - center.x).hypot(start.y - center.y) * angle.sin(),
                            );
                            assert!(
                                center_domain
                                    .iter()
                                    .any(|poly| poly.locate(p) != Containment::Outside),
                                "arc-refit cut leaves selected face center domain at {p:?}"
                            );
                            sampled += 1;
                            arc_samples += 1;
                        }
                    }
                    current = Some(*end);
                }
                _ => {}
            }
        }
        assert!(sampled > 0, "expected sampled cutting moves");
        assert!(arc_samples > 0, "expected sampled arc-refit moves");
    }

    #[test]
    fn selected_body_subset_clips_output_to_its_local_runout_domain() {
        let (mut strategy, tool) = fixture();
        let body = Polygon::new(Contour::new(vec![
            Point::new(20., 20.),
            Point::new(30., 20.),
            Point::new(30., 30.),
            Point::new(20., 30.),
        ]))
        .unwrap();
        for layer in &mut strategy.geometry.layers {
            layer.body_domains = vec![body.clone()];
            layer.face_domains.clear();
        }
        let env = JobEnv {
            heights: Heights::new(15., 12., 10.),
            tools: std::slice::from_ref(&tool),
            stock: Some(([0., 0.], [60., 60.])),
            spindle: SpindleDir::Cw,
        };
        let result = strategy.compute(&env, &CancelToken::new());
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        let expanded_domain = offset(
            std::slice::from_ref(&body),
            2.0 * tool.diameter - strategy.geometry.error_bound - 2.0 * cam_geo::GRID_MM,
            JoinStyle::Round,
        )
        .unwrap();
        let center_stays_in_domain = |point: Point| {
            expanded_domain
                .iter()
                .any(|poly| poly.locate(point) != Containment::Outside)
        };
        let mut current = None;
        let mut sampled = 0;
        for step in result.program.steps() {
            match step {
                Step::Rapid { to, .. } => current = Some(*to),
                Step::Linear { to, .. } => {
                    if let Some(start) = current {
                        for i in 1..=20 {
                            let t = i as f64 / 20.0;
                            let point = Point::new(
                                start.x + (to.x - start.x) * t,
                                start.y + (to.y - start.y) * t,
                            );
                            assert!(
                                center_stays_in_domain(point),
                                "linear cut leaves selected body domain at {point:?}"
                            );
                            sampled += 1;
                        }
                    }
                    current = Some(*to);
                }
                Step::Arc {
                    end, center, dir, ..
                } => {
                    if let Some(start) = current {
                        let start_angle = (start.y - center.y).atan2(start.x - center.x);
                        let raw = (end.y - center.y).atan2(end.x - center.x) - start_angle;
                        let sweep = match dir {
                            cam_cldata::ArcDir::Ccw => raw.rem_euclid(std::f64::consts::TAU),
                            cam_cldata::ArcDir::Cw => -(-raw).rem_euclid(std::f64::consts::TAU),
                        };
                        let radius = (start.x - center.x).hypot(start.y - center.y);
                        for i in 1..=40 {
                            let angle = start_angle + sweep * i as f64 / 40.0;
                            let point = Point::new(
                                center.x + radius * angle.cos(),
                                center.y + radius * angle.sin(),
                            );
                            assert!(
                                center_stays_in_domain(point),
                                "arc-refit cut leaves selected body domain at {point:?}"
                            );
                            sampled += 1;
                        }
                    }
                    current = Some(*end);
                }
                _ => {}
            }
        }
        assert!(
            sampled > 0,
            "expected emitted cutting motion within selected body domain"
        );
    }

    #[test]
    fn protected_face_and_air_only_layers_do_not_report_a_successful_cut() {
        let (mut strategy, tool) = fixture();
        let face = Polygon::new(Contour::new(vec![
            Point::new(10., 10.),
            Point::new(20., 10.),
            Point::new(20., 20.),
            Point::new(10., 20.),
        ]))
        .unwrap();
        for layer in &mut strategy.geometry.layers {
            layer.body_domains.clear();
            layer.face_domains = vec![face.clone()];
        }
        let env = JobEnv {
            heights: Heights::new(15., 12., 10.),
            tools: std::slice::from_ref(&tool),
            stock: Some(([0., 0.], [30., 30.])),
            spindle: SpindleDir::Cw,
        };
        let result = strategy.compute(&env, &CancelToken::new());
        assert!(result.has_errors());
        assert!(result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("no accessible stock")));
        assert!(!result
            .program
            .steps()
            .iter()
            .any(|step| matches!(step, Step::Linear { .. } | Step::Arc { .. })));

        strategy
            .geometry
            .layers
            .iter_mut()
            .for_each(|layer| layer.z += 10.);
        let result = strategy.compute(&env, &CancelToken::new());
        assert!(result.has_errors());
        assert!(result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("no accessible stock")));
        assert!(!result
            .program
            .steps()
            .iter()
            .any(|step| matches!(step, Step::Linear { .. } | Step::Arc { .. })));
    }

    #[test]
    fn cancellation_and_missing_stock_fail_explicitly() {
        let (strategy, tool) = fixture();
        let env = JobEnv {
            heights: Heights::new(15., 12., 10.),
            tools: std::slice::from_ref(&tool),
            stock: Some(([0., 0.], [30., 30.])),
            spindle: SpindleDir::Cw,
        };
        let token = CancelToken::new();
        token.cancel();
        assert!(strategy.compute(&env, &token).cancelled);
        let no_stock = JobEnv { stock: None, ..env };
        assert!(strategy
            .compute(&no_stock, &CancelToken::new())
            .has_errors());
    }

    #[test]
    fn rejects_tools_that_cannot_side_mill_the_full_depth() {
        let (strategy, mut tool) = fixture();
        tool.kind = ToolKind::Drill {
            point_angle_deg: 118.,
        };
        let env = JobEnv {
            heights: Heights::new(15., 12., 10.),
            tools: std::slice::from_ref(&tool),
            stock: Some(([0., 0.], [30., 30.])),
            spindle: SpindleDir::Cw,
        };
        let result = strategy.compute(&env, &CancelToken::new());
        assert!(result.has_errors());
        assert!(result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("drill bit")));

        let short_flute = Tool {
            flute_length: 2.,
            kind: ToolKind::EndMill,
            ..tool.clone()
        };
        let short_flute_env = JobEnv {
            tools: std::slice::from_ref(&short_flute),
            ..env
        };
        let result = strategy.compute(&short_flute_env, &CancelToken::new());
        assert!(result.has_errors());
        assert!(result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("length of cut")));
    }
}
