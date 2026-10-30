//! Headless controller service.
use super::*;
impl AppController {
    /// Post the most recent complete run to the selected controller, caching and
    /// returning it. Cancelled, incomplete or failed runs cannot be exported.
    pub fn export_nc(&mut self) -> Result<&str, ExportError> {
        let outcome = self.outcome.as_ref().ok_or(ExportError::NothingToExport)?;
        match outcome.completion {
            JobCompletion::Complete => {}
            JobCompletion::Cancelled => return Err(ExportError::Cancelled),
            JobCompletion::Incomplete => return Err(ExportError::Incomplete),
            JobCompletion::Failed => return Err(ExportError::HasErrors),
        }
        if outcome.has_errors() {
            return Err(ExportError::HasErrors);
        }
        // A rapid plowing through stock would crash the machine — block by default.
        let rapids = outcome
            .collisions
            .iter()
            .filter(|c| c.kind == CollisionKind::RapidThroughStock)
            .count();
        if rapids > 0 {
            return Err(ExportError::RapidThroughStock(rapids));
        }
        let noncutting = outcome
            .collisions
            .iter()
            .filter(|c| c.kind == CollisionKind::NonCuttingThroughStock)
            .count();
        if noncutting > 0 {
            return Err(ExportError::NonCuttingThroughStock(noncutting));
        }
        let mut options = self.post_options();
        if self.post_kind == PostKind::Estlcam {
            options.tools = self.setup().tools.clone();
        }
        // Re-reference each group to its own workpiece origin (datum): shift every
        // coordinate by −origin so that group's chosen part point becomes G-code
        // (0,0,0), matched by the operator's touch-off for `G15 H<n>`. Operations under
        // different origins (a reorientation) each subtract their own; with one origin
        // this is the whole job shifted by −origin. Design/sim stay in part space.
        let setup = &self.document.current().setup();
        let program = outcome.program.translated_per_datum(|idx| {
            let o = setup.origin_position(idx);
            [-o[0], -o[1], -o[2]]
        });
        let nc = self.post_kind.post(&program, &self.machine, &options)?;
        self.nc = Some(nc);
        Ok(self.nc.as_deref().unwrap())
    }

    /// Export the current toolpath as G-code to `path`. Runs the same gating as
    /// [`AppController::export_nc`] (non-cutting stock collisions block the write).
    pub fn export_nc_to(&mut self, path: impl AsRef<Path>) -> Result<(), ExportToError> {
        let nc = self.export_nc().map_err(ExportToError::Export)?.to_string();
        std::fs::write(path.as_ref(), nc).map_err(|e| ExportToError::Io(e.to_string()))?;
        Ok(())
    }
}
