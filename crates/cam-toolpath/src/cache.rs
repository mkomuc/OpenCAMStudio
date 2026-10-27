//! Exact per-operation dependency keys. No order/transition state is cached:
//! the planner assembles links, datums, spindle and shutdown on every run.
use crate::{CancelToken, JobEnv, StrategyResult};
use cam_cldata::SpindleDir;
use cam_model::{CuttingHeights, Heights, Operation, StockToLeave, Tool};
use std::collections::{BTreeSet, VecDeque};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct Dependencies {
    #[serde(default)]
    adaptive_domain_revision: u32,
    operation: Operation,
    heights: Heights,
    tools: Vec<Tool>,
    stock: Option<([f64; 2], [f64; 2])>,
    spindle: SpindleDir,
    stock_to_leave: StockToLeave,
    geometry_fingerprint: Option<String>,
    cutting_heights: Option<CuttingHeights>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub reused: usize,
    pub computed: usize,
}
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct Entry {
    key: Dependencies,
    result: StrategyResult,
}
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct OperationCache {
    entries: VecDeque<Arc<Entry>>,
    #[serde(skip)]
    pub stats: CacheStats,
}

// Statistics describe the current request, not the persisted cutting fragments.
impl PartialEq for OperationCache {
    fn eq(&self, other: &Self) -> bool {
        self.entries == other.entries
    }
}

/// Per-operation dependencies outside the serialized operation itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CacheExtras {
    pub stock_to_leave: StockToLeave,
    pub geometry_fingerprint: Option<String>,
    pub cutting_heights: Option<CuttingHeights>,
}

impl OperationCache {
    /// Discard every retained version of the selected operations for an explicit
    /// recompute, while keeping unrelated operations available for reuse.
    pub fn invalidate_operations(&mut self, ids: &[u32]) {
        self.entries
            .retain(|entry| !ids.contains(&entry.key.operation.id()));
    }

    pub fn reset_stats(&mut self) {
        self.stats = CacheStats::default();
    }
    pub fn compute(
        &mut self,
        op: &Operation,
        env: &JobEnv,
        cancel: &CancelToken,
        compute: impl FnOnce() -> StrategyResult,
    ) -> StrategyResult {
        self.compute_with_dependencies(op, env, cancel, CacheExtras::default(), compute)
    }

    /// Compute with setup-map allowances and resolved geometry/heights dependencies.
    pub(crate) fn compute_with_dependencies(
        &mut self,
        op: &Operation,
        env: &JobEnv,
        cancel: &CancelToken,
        extras: CacheExtras,
        compute: impl FnOnce() -> StrategyResult,
    ) -> StrategyResult {
        if cancel.is_cancelled() {
            return StrategyResult {
                cancelled: true,
                ..Default::default()
            };
        }
        let required = op.tools();
        let (radial, axial) = extras.stock_to_leave.effective();
        let stock_to_leave = StockToLeave {
            enabled: radial != 0.0 || axial != 0.0,
            radial,
            axial,
        };
        let key = Dependencies {
            adaptive_domain_revision: if matches!(op, Operation::Parallel(_)) { crate::ParallelStrategy::DOMAIN_REVISION } else if matches!(op, Operation::Adaptive(_)) {
                crate::AdaptiveStrategy::DOMAIN_REVISION
            } else {
                0
            },
            operation: op.clone(),
            heights: env.heights,
            tools: required
                .iter()
                .filter_map(|n| env.tool(*n).cloned())
                .collect(),
            stock: env.stock,
            spindle: env.spindle,
            stock_to_leave,
            geometry_fingerprint: extras.geometry_fingerprint,
            cutting_heights: extras.cutting_heights,
        };
        if let Some(at) = self.entries.iter().position(|entry| entry.key == key) {
            let entry = self.entries.remove(at).unwrap();
            let result = entry.result.clone();
            self.entries.push_back(entry);
            self.stats.reused += 1;
            return result;
        }
        self.stats.computed += 1;
        let result = compute();
        if !result.cancelled && !result.has_errors() && !cancel.is_cancelled() {
            self.entries.push_back(Arc::new(Entry {
                key,
                result: result.clone(),
            }));
            // Bound undo history, but always retain the latest path for every
            // operation. Evicting a current path would force recalculation merely
            // because a large project was reopened. Worker snapshots share Arcs.
            while self.entries.len() > 64
                || self
                    .entries
                    .iter()
                    .map(|entry| entry.result.program.steps().len())
                    .sum::<usize>()
                    > 500_000
            {
                let mut seen = BTreeSet::new();
                let obsolete = self
                    .entries
                    .iter()
                    .enumerate()
                    .rev()
                    .filter_map(|(index, entry)| {
                        (!seen.insert(entry.key.operation.id())).then_some(index)
                    })
                    .last();
                let Some(index) = obsolete else {
                    break;
                };
                self.entries.remove(index);
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Diagnostic;
    use cam_model::DrillOp;
    fn operation(id: u32) -> Operation {
        Operation::Drill(DrillOp {
            id,
            tool: 1,
            points: vec![[1., 1.]],
            depth: 2.,
            start_offset: 0.,
            peck: None,
            dwell: None,
            spindle_rpm: 1000.,
            tool_preset: None,
            cooling: Default::default(),
            entry_feeds: Default::default(),
            work_offset: 1,
            feed: 100.,
        })
    }

    #[test]
    fn persisted_adaptive_paths_with_outdated_safety_revisions_are_recomputed() {
        let adaptive = Operation::Adaptive(cam_model::AdaptiveOp {
            id: 17,
            tool: 1,
            spindle_rpm: 1000.0,
            tool_preset: None,
            cooling: Default::default(),
            entry_feeds: Default::default(),
            work_offset: 1,
            params: Default::default(),
        });
        let env = JobEnv {
            heights: Heights::new(10., 5., 0.),
            tools: &[], stock: None, spindle: SpindleDir::Cw,
        };
        let token = CancelToken::new();
        let mut cache = OperationCache::default();
        cache.compute(&adaptive, &env, &token, StrategyResult::default);
        for old_revision in 0..crate::AdaptiveStrategy::DOMAIN_REVISION {
            let mut saved = serde_json::to_value(&cache).unwrap();
            if old_revision == 0 {
                saved["entries"][0]["key"].as_object_mut().unwrap().remove("adaptive_domain_revision");
            } else {
                saved["entries"][0]["key"]["adaptive_domain_revision"] = old_revision.into();
            }
            let mut reopened: OperationCache = serde_json::from_value(saved).unwrap();
            reopened.compute(&adaptive, &env, &token, StrategyResult::default);
            assert_eq!(reopened.stats.computed, 1);
            assert_eq!(reopened.stats.reused, 0);
            reopened.compute(&adaptive, &env, &token, || panic!("current path must be reused"));
            assert_eq!(reopened.stats.reused, 1);
        }
    }

    #[test]
    fn changed_resolved_cutting_heights_do_not_reuse_adaptive_fragments() {
        let adaptive = Operation::Adaptive(cam_model::AdaptiveOp {
            id: 17,
            tool: 1,
            spindle_rpm: 1000.0,
            tool_preset: None,
            cooling: Default::default(),
            entry_feeds: Default::default(),
            work_offset: 1,
            params: Default::default(),
        });
        let env = JobEnv {
            heights: Heights::new(10.0, 5.0, 0.0),
            tools: &[],
            stock: None,
            spindle: SpindleDir::Cw,
        };
        let mut cache = OperationCache::default();
        let token = CancelToken::new();
        let mut calls = 0;
        for bottom in [-2.0, -2.5] {
            cache.compute_with_dependencies(
                &adaptive,
                &env,
                &token,
                CacheExtras {
                    stock_to_leave: StockToLeave::default(),
                    geometry_fingerprint: Some("same binding".into()),
                    cutting_heights: Some(CuttingHeights { top: 0.0, bottom }),
                },
                || {
                    calls += 1;
                    StrategyResult::default()
                },
            );
        }
        assert_eq!(calls, 2);
        assert_eq!(cache.stats.computed, 2);
        assert_eq!(cache.stats.reused, 0);
    }
    #[test]
    fn cache_bound_shared_snapshots_and_unsuccessful_results() {
        let env = JobEnv {
            heights: Heights::new(5., 2., 0.),
            tools: &[],
            stock: None,
            spindle: SpindleDir::Cw,
        };
        let mut cache = OperationCache::default();
        let token = CancelToken::new();
        for id in 0..100 {
            cache.compute(&operation(id), &env, &token, StrategyResult::default);
        }
        assert_eq!(cache.entries.len(), 100);
        let copy = cache.clone();
        assert!(
            Arc::ptr_eq(&cache.entries[0], &copy.entries[0]),
            "cache snapshots share inputs as well as motions"
        );
        cache.compute(&operation(99), &env, &token, || {
            panic!("matching input must reuse")
        });
        assert_eq!(cache.stats.reused, 1);
        let count = cache.entries.len();
        cache.compute(&operation(100), &env, &token, || StrategyResult {
            diagnostics: vec![Diagnostic::error("bad geometry")],
            ..Default::default()
        });
        assert_eq!(cache.entries.len(), count);
        cache.compute(&operation(100), &env, &token, || {
            token.cancel();
            StrategyResult::default()
        });
        assert_eq!(
            cache.entries.len(),
            count,
            "cancelled work cannot populate the cache"
        );
    }
}
