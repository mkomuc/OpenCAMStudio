//! Exact per-operation dependency keys. No order/transition state is cached:
//! the planner assembles links, datums, spindle and shutdown on every run.
use crate::{CancelToken, JobEnv, StrategyResult};
use cam_cldata::SpindleDir;
use cam_model::{Heights, Operation, Tool};
use std::collections::VecDeque;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
struct Dependencies {
    operation: Operation,
    heights: Heights,
    tools: Vec<Tool>,
    stock: Option<([f64; 2], [f64; 2])>,
    spindle: SpindleDir,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub reused: usize,
    pub computed: usize,
}
#[derive(Clone, Debug)]
struct Entry {
    key: Dependencies,
    result: StrategyResult,
}
#[derive(Clone, Debug, Default)]
pub struct OperationCache {
    entries: VecDeque<Arc<Entry>>,
    pub stats: CacheStats,
}
impl OperationCache {
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
        if cancel.is_cancelled() {
            return StrategyResult {
                cancelled: true,
                ..Default::default()
            };
        }
        let required = op.tools();
        let key = Dependencies {
            operation: op.clone(),
            heights: env.heights,
            tools: required
                .iter()
                .filter_map(|n| env.tool(*n).cloned())
                .collect(),
            stock: env.stock,
            spindle: env.spindle,
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
            // Bound both entry count and retained motions. Arc clones share entries
            // with worker snapshots; completed workers replace the controller cache.
            while self.entries.len() > 64
                || self
                    .entries
                    .iter()
                    .map(|entry| entry.result.program.steps().len())
                    .sum::<usize>()
                    > 500_000
            {
                self.entries.pop_front();
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
            work_offset: 1,
            feed: 100.,
        })
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
        assert_eq!(cache.entries.len(), 64);
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
