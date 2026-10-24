//! Headless controller service.
use super::*;
impl AppController {
    /// Validate a planar geometry field in full. The first loop is the boundary;
    /// additional closed loops are protected islands for area strategies or
    /// circular holes for Drill/Thread. Other strategies consume one chain.
    pub fn planar_operation_eligibility(
        &self,
        kind: OpKind,
        selected: &[LoopRef],
    ) -> Result<(), String> {
        let boundary = *selected.first().ok_or("Select geometry first")?;
        if selected
            .iter()
            .enumerate()
            .any(|(i, r)| selected[..i].contains(r))
        {
            return Err("The same drawing feature was selected more than once".into());
        }
        let (points, closed) = self
            .loop_points(boundary)
            .ok_or("Needs reselection: drawing feature is missing")?;
        if !closed && !op_accepts_open_paths(kind) {
            return Err(
                "This operation requires a closed boundary; open paths support Profile and Engrave"
                    .into(),
            );
        }
        if op_selects_circles(kind) && (!closed || fit_circle(&points).is_none()) {
            return Err("Drill, Thread and Bore require a circular hole".into());
        }
        if op_selects_circles(kind) {
            let radius = fit_circle(&points).unwrap().1;
            for reference in &selected[1..] {
                let contour = self
                    .loop_contour(*reference)
                    .ok_or("Select existing circular holes")?;
                let (_, other_radius) = fit_circle(contour.points()).ok_or(
                    "Drill, Thread and Bore require circular holes of the same geometry kind",
                )?;
                if matches!(kind, OpKind::Thread | OpKind::Bore)
                    && (radius - other_radius).abs() > 0.001
                {
                    return Err(if kind == OpKind::Thread {
                        "Thread uses one major diameter per operation; select equal-diameter holes or create separate operations"
                    } else {
                        "Bore uses one diameter per operation; select equal-diameter features or create separate operations"
                    }.into());
                }
            }
            return Ok(());
        }
        if kind == OpKind::Profile {
            for reference in selected {
                let (points, closed) = self
                    .loop_points(*reference)
                    .ok_or("Profile chain is missing")?;
                if points.len() < if closed { 3 } else { 2 }
                    || points.iter().any(|p| !p.x.is_finite() || !p.y.is_finite())
                {
                    return Err("Select a valid open or closed profile chain".into());
                }
            }
            return Ok(());
        }
        if selected.len() == 1 {
            return Ok(());
        }
        if !op_takes_islands(kind) {
            return Err("This strategy supports one boundary or chain per operation; select unrelated chains in separate operations".into());
        }
        let outer = cam_geo::Polygon::new(Contour::new(points)).map_err(|e| e.to_string())?;
        let mut islands = Vec::new();
        for reference in &selected[1..] {
            let contour = self
                .loop_contour(*reference)
                .ok_or("Islands must be existing closed loops")?;
            let island = cam_geo::Polygon::new(contour.clone()).map_err(|e| e.to_string())?;
            if contour
                .points()
                .iter()
                .any(|p| outer.locate(*p) != cam_geo::Containment::Inside)
                || !cam_geo::difference(std::slice::from_ref(&island), std::slice::from_ref(&outer))
                    .map_err(|e| e.to_string())?
                    .is_empty()
            {
                return Err("Additional loops must be islands wholly inside the selected boundary; unrelated chains require separate operations".into());
            }
            if !cam_geo::intersection(std::slice::from_ref(&island), &islands)
                .map_err(|e| e.to_string())?
                .is_empty()
            {
                return Err("Selected islands overlap or contain each other".into());
            }
            islands.push(island);
        }
        Ok(())
    }

    /// Seed the existing wizard from a feature-first drawing selection. No
    /// document change occurs until the same Confirm used by operation-first.
    pub fn begin_operation_from_selection(
        &mut self,
        kind: OpKind,
        selected: &[LoopRef],
    ) -> Result<(), String> {
        self.planar_operation_eligibility(kind, selected)?;
        self.begin_operation(kind);
        self.set_pending_operation_geometry(selected, None)
    }

    /// Populate the operation-first wizard through the same selection contract.
    /// A rejected input preserves the boundary, islands and start point.
    pub fn set_pending_operation_geometry(
        &mut self,
        selected: &[LoopRef],
        start: Option<[f64; 2]>,
    ) -> Result<(), String> {
        let kind = self
            .pending_op
            .as_ref()
            .ok_or("Choose an operation first")?
            .kind;
        if selected.is_empty() {
            let pending = self.pending_op.as_mut().unwrap();
            pending.boundary = None;
            pending.islands.clear();
            pending.additional_holes.clear();
            pending.start = None;
            return Ok(());
        }
        self.planar_operation_eligibility(kind, selected)?;
        if start.is_some_and(|p| p.iter().any(|n| !n.is_finite())) {
            return Err("Start point must be finite".into());
        }
        let boundary = selected[0];
        let nested =
            if kind == OpKind::Carve && selected.len() == 1 && boundary.part == LoopPart::Outer {
                (0..self.regions[boundary.region].holes().len())
                    .map(|h| LoopRef::hole(boundary.region, h))
                    .collect()
            } else {
                selected[1..].to_vec()
            };
        let pending = self.pending_op.as_mut().unwrap();
        pending.boundary = Some(boundary);
        if op_selects_circles(kind) {
            pending.additional_holes = selected[1..].to_vec();
            pending.islands.clear();
        } else {
            pending.islands = nested;
            pending.additional_holes.clear();
        }
        pending.start = start;
        Ok(())
    }

    /// Complete input for the active drawing geometry field, in backend order.
    pub fn pending_operation_geometry(&self) -> Vec<LoopRef> {
        let Some(pending) = &self.pending_op else {
            return Vec::new();
        };
        let additional = if op_selects_circles(pending.kind) {
            &pending.additional_holes
        } else {
            &pending.islands
        };
        pending
            .boundary
            .into_iter()
            .chain(additional.iter().copied())
            .collect()
    }
    /// Select a node. Clamped to a valid target: an out-of-range tool or a
    /// missing operation falls back to the setup.
    pub fn select(&mut self, selection: Selection) {
        if let Selection::Operation(id) = selection {
            if let Some(owner) = self
                .document()
                .setups
                .iter()
                .find(|setup| setup.operations.iter().any(|op| op.id() == id))
                .map(|s| s.id)
            {
                if owner != self.active_setup_id {
                    let _ = self.activate_setup(owner);
                }
            }
        }
        self.selection = match selection {
            Selection::Tool(i) if i < self.document.current().setup().tools.len() => selection,
            Selection::Operation(id) if self.operation(id).is_some() => selection,
            Selection::Setup | Selection::Origin | Selection::Stock | Selection::Machine => {
                selection
            }
            _ => Selection::Setup,
        };
        if let Selection::Operation(id) = self.selection {
            self.cad_selection = self
                .document()
                .geometry
                .bindings
                .get(&id)
                .map(|b| b.selected.clone())
                .unwrap_or_default();
        }
    }

    /// The boundary loop whose edge is closest to `world` within `aperture`
    /// world-mm, together with the nearest vertex (the start point). Line picking
    /// across every region; a click near no edge returns `None`.
    pub fn nearest_loop(&self, world: [f64; 2], aperture: f64) -> Option<(LoopRef, [f64; 2])> {
        let w = Point::new(world[0], world[1]);
        // (loop, nearest vertex, closest edge distance²)
        let mut best: Option<(LoopRef, [f64; 2], f64)> = None;
        for (ri, region) in self.regions.iter().enumerate() {
            let loops = std::iter::once((LoopPart::Outer, region.outer())).chain(
                region
                    .holes()
                    .iter()
                    .enumerate()
                    .map(|(hi, h)| (LoopPart::Hole(hi), h)),
            );
            for (part, contour) in loops {
                let pts = contour.points();
                if pts.len() < 2 {
                    continue;
                }
                let edge_d2 = (0..pts.len())
                    .map(|k| dist_point_seg2(w, pts[k], pts[(k + 1) % pts.len()]))
                    .fold(f64::MAX, f64::min);
                if best.is_none_or(|(_, _, bd2)| edge_d2 < bd2) {
                    let nv = pts
                        .iter()
                        .min_by(|p, q| w.distance_sq(**p).total_cmp(&w.distance_sq(**q)))
                        .unwrap();
                    best = Some((LoopRef { region: ri, part }, [nv.x, nv.y], edge_d2));
                }
            }
        }
        best.filter(|(_, _, d2)| *d2 <= aperture * aperture)
            .map(|(l, v, _)| (l, v))
    }

    /// Feature-first picking includes closed loops and open drawing paths before
    /// an operation is chosen. The eventual strategy validates the entire field.
    pub fn nearest_drawing_feature(
        &self,
        world: [f64; 2],
        aperture: f64,
    ) -> Option<(LoopRef, [f64; 2])> {
        let w = Point::new(world[0], world[1]);
        self.iter_loops()
            .map(|(reference, contour)| (reference, contour.points(), true))
            .chain(
                self.open_paths
                    .iter()
                    .enumerate()
                    .map(|(i, path)| (LoopRef::open(i), path.points(), false)),
            )
            .filter(|(_, points, _)| points.len() >= 2)
            .map(|(reference, points, closed)| {
                let (point, distance) = nearest_point_on_path(points, w, closed);
                (reference, point, distance)
            })
            .filter(|(_, _, distance)| *distance <= aperture * aperture)
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(reference, point, _)| (reference, point))
    }

    /// The loop whose edge is closest to `world` within `aperture`, together with
    /// the exact **nearest point on that edge** (not a vertex). The always-works
    /// fallback for boundary selection when no object-snap catches. With
    /// `circles_only`, only circular loops are selectable (drill/thread target
    /// holes) — a rectangle or open edge is ignored.
    pub fn nearest_loop_point(
        &self,
        world: [f64; 2],
        aperture: f64,
        circles_only: bool,
    ) -> Option<(LoopRef, [f64; 2])> {
        let w = Point::new(world[0], world[1]);
        let mut best: Option<(LoopRef, [f64; 2], f64)> = None;
        for (loop_ref, pts, closed) in self.iter_pickable() {
            // Drill/thread target holes: an open stroke is never a hole.
            if circles_only && (!closed || fit_circle(pts).is_none()) {
                continue;
            }
            let (pt, d2) = nearest_point_on_path(pts, w, closed);
            if best.is_none_or(|(_, _, bd2)| d2 < bd2) {
                best = Some((loop_ref, pt, d2));
            }
        }
        best.filter(|(_, _, d2)| *d2 <= aperture * aperture)
            .map(|(l, p, _)| (l, p))
    }

    /// The best object-snap under the cursor among the `enabled` kinds, within
    /// `aperture` world-mm, or `None`. Higher-priority kinds (End → Mid →
    /// Quadrant → Nearest) win over lower ones even if slightly farther, so a
    /// corner beats a mere nearest-point when both are in the box. Read-only —
    /// the GUI calls it on hover to preview the snap marker and on click to set
    /// the start.
    pub fn snap_at(&self, world: [f64; 2], aperture: f64, enabled: &[SnapKind]) -> Option<SnapHit> {
        let w = Point::new(world[0], world[1]);
        let ap2 = aperture * aperture;
        // (priority, dist², hit) — lower priority number wins.
        let mut best: Option<(u8, f64, SnapHit)> = None;
        let mut consider = |kind: SnapKind, point: [f64; 2], loop_ref: LoopRef| {
            let d2 = w.distance_sq(Point::new(point[0], point[1]));
            if d2 > ap2 {
                return;
            }
            let prio = kind.priority();
            let better = best
                .as_ref()
                .is_none_or(|(bp, bd, _)| prio < *bp || (prio == *bp && d2 < *bd));
            if better {
                best = Some((
                    prio,
                    d2,
                    SnapHit {
                        loop_ref,
                        point,
                        kind,
                    },
                ));
            }
        };
        for (loop_ref, pts, closed) in self.iter_pickable() {
            if pts.len() < 2 {
                continue;
            }
            // An open stroke has no corner *loop*: its two free ends are the snaps a
            // machinist wants (that is where the groove starts and stops), and the
            // mid/quadrant analyses assume a closed ring, so they are skipped.
            if !closed {
                if enabled.contains(&SnapKind::End) {
                    for p in [pts[0], pts[pts.len() - 1]] {
                        consider(SnapKind::End, [p.x, p.y], loop_ref);
                    }
                }
                if enabled.contains(&SnapKind::Nearest) {
                    let (q, _) = nearest_point_on_path(pts, w, false);
                    consider(SnapKind::Nearest, q, loop_ref);
                }
                continue;
            }
            let corners = loop_corners(pts);
            if enabled.contains(&SnapKind::End) {
                for &ci in &corners {
                    consider(SnapKind::End, [pts[ci].x, pts[ci].y], loop_ref);
                }
            }
            if enabled.contains(&SnapKind::Mid) {
                for m in loop_mids(pts, &corners) {
                    consider(SnapKind::Mid, m, loop_ref);
                }
            }
            // Quadrant: the cardinal points of a **circular** loop (a cornerless
            // loop that fits a circle). Partial arcs await the arc-carrying model.
            if enabled.contains(&SnapKind::Quadrant) && corners.is_empty() {
                if let Some(([cx, cy], r)) = fit_circle(pts) {
                    for (dx, dy) in [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)] {
                        consider(SnapKind::Quadrant, [cx + dx * r, cy + dy * r], loop_ref);
                    }
                }
            }
            if enabled.contains(&SnapKind::Nearest) {
                let (q, _) = nearest_point_on_contour(pts, w);
                consider(SnapKind::Nearest, q, loop_ref);
            }
        }
        best.map(|(_, _, hit)| hit)
    }

    /// Whether open imported paths are currently pickable — true exactly when the
    /// operation being created can machine one, so hover preview and click agree.
    pub(super) fn open_paths_pickable(&self) -> bool {
        self.pending_op
            .as_ref()
            .is_some_and(|p| op_accepts_open_paths(p.kind))
    }

    /// Every pickable path: the closed region loops, plus the open imported paths
    /// when the pending operation can machine them. Yields `(ref, points, closed)`.
    pub(super) fn iter_pickable(&self) -> impl Iterator<Item = (LoopRef, &[Point], bool)> {
        let open = if self.open_paths_pickable() {
            &self.open_paths[..]
        } else {
            &[][..]
        };
        self.iter_loops().map(|(r, c)| (r, c.points(), true)).chain(
            open.iter()
                .enumerate()
                .map(|(i, p)| (LoopRef::open(i), p.points(), false)),
        )
    }

    /// Every closed loop across all regions as `(ref, contour)`.
    pub(super) fn iter_loops(&self) -> impl Iterator<Item = (LoopRef, &Contour)> {
        self.regions.iter().enumerate().flat_map(|(ri, region)| {
            std::iter::once((
                LoopRef {
                    region: ri,
                    part: LoopPart::Outer,
                },
                region.outer(),
            ))
            .chain(region.holes().iter().enumerate().map(move |(hi, h)| {
                (
                    LoopRef {
                        region: ri,
                        part: LoopPart::Hole(hi),
                    },
                    h,
                )
            }))
        })
    }

    /// Record a viewport pick into the pending operation at `world`, with a pickbox
    /// of `aperture` world-mm.
    ///
    /// **Nothing is committed here.** The pick sets (or replaces) the boundary; for a
    /// **Pocket** that already has a boundary, further picks toggle islands. The
    /// operation is created only by [`confirm_operation`], so the tool and the
    /// geometry may be chosen in **either order**, and either may be changed until
    /// Confirm. Re-picking before Confirm simply moves the boundary.
    pub fn pick_operation_geometry(
        &mut self,
        world: [f64; 2],
        aperture: f64,
        snaps: &[SnapKind],
    ) -> PickResult {
        let Some(pending) = self.pending_op.clone() else {
            return PickResult::Missed;
        };
        // Resolve the loop and start point: prefer an object-snap (corner / mid /
        // …), else the nearest point on the loop under the box. Drill/thread are
        // restricted to circular loops (holes).
        let circles = op_selects_circles(pending.kind);
        let (picked, start) = match self.snap_at(world, aperture, snaps) {
            Some(hit) => (hit.loop_ref, hit.point),
            None => match self.nearest_loop_point(world, aperture, circles) {
                Some(lp) => lp,
                None => return PickResult::Missed,
            },
        };
        // Islands are a second stage, once a boundary exists, for the kinds that
        // consume an area and so must be told what to leave standing.
        let island_mode = op_takes_islands(pending.kind) && pending.boundary.is_some();
        // Carving a letter needs its counter left uncut, and a counter is exactly a
        // hole of the picked region — so seed the islands from the geometry's own
        // nesting rather than making the operator click each one. They stay togglable.
        let nested: Vec<LoopRef> = match (pending.kind, picked.part) {
            (OpKind::Carve, LoopPart::Outer) => self
                .regions
                .get(picked.region)
                .map(|r| {
                    (0..r.holes().len())
                        .map(|h| LoopRef::hole(picked.region, h))
                        .collect()
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let p = self.pending_op.as_mut().unwrap();
        if island_mode {
            if Some(picked) == p.boundary {
                return PickResult::Selecting; // the boundary is not an island
            }
            let mut islands = p.islands.clone();
            match islands.iter().position(|l| *l == picked) {
                Some(pos) => {
                    islands.remove(pos);
                }
                None => islands.push(picked),
            }
            let selected: Vec<_> = std::iter::once(p.boundary.unwrap())
                .chain(islands)
                .collect();
            if self
                .planar_operation_eligibility(pending.kind, &selected)
                .is_err()
            {
                return PickResult::Missed;
            }
            self.pending_op.as_mut().unwrap().islands = selected[1..].to_vec();
        } else {
            if self
                .planar_operation_eligibility(pending.kind, &[picked])
                .is_err()
            {
                return PickResult::Missed;
            }
            let p = self.pending_op.as_mut().unwrap();
            p.boundary = Some(picked);
            p.start = Some(start);
            p.islands = nested;
            p.additional_holes.clear();
        }
        PickResult::Selecting
    }
}

#[cfg(test)]
mod workflow_tests {
    use super::*;

    fn drawing() -> AppController {
        let mut app = AppController::new(crate::default_machine());
        let square = |x: f64| {
            Contour::new(vec![
                Point::new(x, 0.),
                Point::new(x + 20., 0.),
                Point::new(x + 20., 20.),
                Point::new(x, 20.),
            ])
        };
        let circle = |x: f64, radius: f64| {
            Contour::new(
                (0..64)
                    .map(|i| {
                        let angle = i as f64 * std::f64::consts::TAU / 64.;
                        Point::new(x + radius * angle.cos(), 10. + radius * angle.sin())
                    })
                    .collect(),
            )
        };
        app.regions = vec![
            Polygon::with_holes(
                square(0.),
                vec![circle(4., 1.), circle(9., 1.), circle(15., 2.)],
            )
            .unwrap(),
            Polygon::new(square(30.)).unwrap(),
        ];
        app
    }

    #[test]
    fn planar_creation_orders_preserve_all_drill_and_thread_holes() {
        for kind in [OpKind::Drill, OpKind::Thread] {
            let features = [LoopRef::hole(0, 0), LoopRef::hole(0, 1)];
            let mut feature_first = drawing();
            feature_first
                .begin_operation_from_selection(kind, &features)
                .unwrap();
            assert!(!feature_first.pending_ready());
            feature_first.set_pending_tool(1);
            assert!(feature_first.confirm_operation(None));

            let mut operation_first = drawing();
            operation_first.begin_operation(kind);
            operation_first.set_pending_tool(1);
            operation_first
                .set_pending_operation_geometry(&features, None)
                .unwrap();
            assert_eq!(operation_first.pending_operation_geometry(), features);
            assert!(operation_first.confirm_operation(None));
            assert_eq!(
                feature_first.selected_operation(),
                operation_first.selected_operation()
            );
            let points = match operation_first.selected_operation().unwrap() {
                Operation::Drill(op) => &op.points,
                Operation::Thread(op) => &op.points,
                _ => panic!("wrong strategy"),
            };
            assert_eq!(points.len(), 2);
            assert!((points[0][0] - 4.).abs() < 1e-6);
            assert!((points[1][0] - 9.).abs() < 1e-6);
        }
    }

    #[test]
    fn drawing_rejects_unrelated_chains_and_incompatible_thread_diameters_atomically() {
        let mut app = drawing();
        let unrelated = [LoopRef::outer(0), LoopRef::outer(1)];
        for kind in [OpKind::Face, OpKind::Chamfer, OpKind::Engrave] {
            assert!(app
                .planar_operation_eligibility(kind, &unrelated)
                .unwrap_err()
                .contains("one boundary"));
        }
        assert!(app
            .planar_operation_eligibility(OpKind::Pocket, &unrelated)
            .unwrap_err()
            .contains("islands wholly inside"));
        app.begin_operation_from_selection(OpKind::Thread, &[LoopRef::hole(0, 0)])
            .unwrap();
        let before = app.pending_op();
        assert!(app
            .set_pending_operation_geometry(&[LoopRef::hole(0, 0), LoopRef::hole(0, 2)], None)
            .unwrap_err()
            .contains("major diameter"));
        assert_eq!(app.pending_op(), before);
        assert!(app
            .planar_operation_eligibility(
                OpKind::Drill,
                &[LoopRef::hole(0, 0), LoopRef::hole(0, 2)]
            )
            .is_ok());
        assert!(app
            .planar_operation_eligibility(OpKind::Drill, &[LoopRef::outer(0)])
            .unwrap_err()
            .contains("circular hole"));
    }

    #[test]
    fn planar_islands_round_trip_and_missing_inputs_cannot_confirm() {
        let mut app = drawing();
        let features = [LoopRef::outer(0), LoopRef::hole(0, 0), LoopRef::hole(0, 1)];
        app.begin_operation_from_selection(OpKind::Pocket, &features)
            .unwrap();
        app.set_pending_tool(1);
        assert!(app.confirm_operation(None));
        let Operation::Pocket(op) = app.selected_operation().unwrap() else {
            panic!("expected pocket");
        };
        assert_eq!(op.islands.len(), 2);
        app.begin_operation_from_selection(OpKind::Pocket, &features)
            .unwrap();
        app.set_pending_tool(1);
        app.regions.clear();
        assert!(!app.pending_ready());
        assert!(!app.confirm_operation(None));
        assert_eq!(app.setup().operations.len(), 1);
    }

    #[test]
    fn multiple_drawing_profile_contours_preserve_hole_sides_and_geometry() {
        let mut app = drawing();
        let features = [LoopRef::outer(0), LoopRef::hole(0, 0), LoopRef::outer(1)];
        app.begin_operation_from_selection(OpKind::Profile, &features)
            .unwrap();
        app.set_pending_tool(1);
        assert!(app.confirm_operation(None));
        let Some(Operation::Profile(profile)) = app.selected_operation() else {
            panic!("expected profile")
        };
        assert_eq!(profile.additional_chains.len(), 2);
        assert_eq!(profile.additional_chains[0].side, Side::Inside);
        assert_eq!(profile.additional_chains[1].side, Side::Outside);
        assert_eq!(
            &profile.additional_chains[0].chain,
            app.loop_contour(features[1]).unwrap()
        );
    }

    #[test]
    fn feature_first_open_paths_support_profile_and_engrave() {
        let mut app = drawing();
        app.open_paths = vec![Polyline::new(vec![
            Point::new(25., 2.),
            Point::new(25., 8.),
        ])];
        let (feature, start) = app.nearest_drawing_feature([25.1, 5.], 0.5).unwrap();
        assert_eq!(feature, LoopRef::open(0));
        assert_eq!(start, [25., 5.]);
        app.begin_operation_from_selection(OpKind::Profile, &[feature])
            .unwrap();
        app.set_pending_tool(1);
        assert!(app.confirm_operation(None));
        let Some(Operation::Profile(profile)) = app.selected_operation() else {
            panic!("expected profile")
        };
        assert!(!profile.closed);
        assert_eq!(profile.chain.points(), app.open_paths[0].points());
        app.begin_operation_from_selection(OpKind::Engrave, &[feature])
            .unwrap();
        app.set_pending_tool(1);
        assert!(app.confirm_operation(None));
        let Operation::Engrave(operation) = app.selected_operation().unwrap() else {
            panic!("expected engraving");
        };
        assert!(!operation.closed);
        assert_eq!(operation.chain.points(), app.open_paths[0].points());
    }
}
