//! One cancellable comparison worker. Repeated positions share classifications
//! across stock frames; only the latest requested frame can reach the UI.
use cam_render::{CadScene, MeshVertex, SurfaceClass};
use cam_toolpath::CancelToken;
use std::{
    collections::HashMap,
    sync::{mpsc, Arc, Mutex},
};

const CACHE_LIMIT: usize = 262_144;
type PointCache = HashMap<[u32; 3], Option<SurfaceClass>>;

struct Request {
    generation: u64,
    points: Vec<[f32; 3]>,
    tolerance: f64,
    cancel: CancelToken,
}
struct Response {
    generation: u64,
    classes: Vec<Option<SurfaceClass>>,
}
struct Worker {
    inbox: Arc<Mutex<Option<Request>>>,
    wake: mpsc::SyncSender<()>,
    results: mpsc::Receiver<Response>,
}

#[derive(Default)]
pub(crate) struct StockComparison {
    worker: Option<Worker>,
    source: Option<Arc<CadScene>>,
    generation: u64,
    submitted: Option<u64>,
    active: Option<CancelToken>,
    tolerance: Option<u64>,
}

impl StockComparison {
    pub fn invalidate(&mut self) {
        if let Some(cancel) = self.active.take() {
            cancel.cancel();
        }
        self.generation = self.generation.wrapping_add(1);
        self.submitted = None;
    }

    /// Empty targets need no worker. Otherwise enqueue one owned position
    /// snapshot and coalesce repeated UI refreshes of the same stock frame.
    pub fn request(
        &mut self,
        target: &Arc<CadScene>,
        vertices: &[MeshVertex],
        tolerance: f64,
    ) -> Option<Vec<Option<SurfaceClass>>> {
        let changed = self
            .source
            .as_ref()
            .is_none_or(|source| !Arc::ptr_eq(source, target));
        if changed || self.tolerance != Some(tolerance.to_bits()) {
            self.invalidate();
            if changed {
                self.worker = None;
            }
            self.source = Some(target.clone());
            self.tolerance = Some(tolerance.to_bits());
        }
        if self.submitted == Some(self.generation) {
            return None;
        }
        self.submitted = Some(self.generation);
        if target.bounds().is_none() || vertices.is_empty() {
            return Some(vec![None; vertices.len()]);
        }
        let worker = self
            .worker
            .get_or_insert_with(|| Worker::new(target.clone()));
        let cancel = CancelToken::new();
        self.active = Some(cancel.clone());
        *worker.inbox.lock().unwrap() = Some(Request {
            generation: self.generation,
            points: vertices.iter().map(|vertex| vertex.position).collect(),
            tolerance,
            cancel,
        });
        let _ = worker.wake.try_send(());
        None
    }

    pub fn poll(&mut self) -> Option<Vec<Option<SurfaceClass>>> {
        let worker = self.worker.as_ref()?;
        loop {
            match worker.results.try_recv() {
                Ok(response) if response.generation == self.generation => {
                    self.active = None;
                    return Some(response.classes);
                }
                Ok(_) => continue,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.active = None;
                    return None;
                }
                Err(mpsc::TryRecvError::Empty) => return None,
            }
        }
    }

    pub fn pending(&self) -> bool {
        self.active.is_some()
    }
}
impl Drop for StockComparison {
    fn drop(&mut self) {
        self.invalidate();
    }
}

impl Worker {
    fn new(target: Arc<CadScene>) -> Self {
        let inbox = Arc::new(Mutex::new(None::<Request>));
        let (wake, requests) = mpsc::sync_channel(1);
        let (results, receiver) = mpsc::channel();
        let worker_inbox = inbox.clone();
        let worker_target = target.clone();
        std::thread::spawn(move || {
            let mut cache = PointCache::new();
            let mut tolerance = None;
            while requests.recv().is_ok() {
                let Some(request) = worker_inbox.lock().unwrap().take() else {
                    continue;
                };
                if tolerance != Some(request.tolerance.to_bits()) || cache.len() >= CACHE_LIMIT {
                    cache.clear();
                    tolerance = Some(request.tolerance.to_bits());
                }
                let Some((classes, _queries)) = classify_points(&request, &mut cache, |point| {
                    worker_target.classify_surface(point, request.tolerance)
                }) else {
                    continue;
                };
                if !request.cancel.is_cancelled()
                    && results
                        .send(Response {
                            generation: request.generation,
                            classes,
                        })
                        .is_err()
                {
                    break;
                }
            }
        });
        Self {
            inbox,
            wake,
            results: receiver,
        }
    }
}

fn classify_points(
    request: &Request,
    cache: &mut PointCache,
    mut classify: impl FnMut([f64; 3]) -> Option<SurfaceClass>,
) -> Option<(Vec<Option<SurfaceClass>>, usize)> {
    let mut classes = Vec::with_capacity(request.points.len());
    let mut queries = 0;
    for point in &request.points {
        if request.cancel.is_cancelled() {
            return None;
        }
        let key = point.map(|v| if v == 0. { 0 } else { v.to_bits() });
        let class = if let Some(class) = cache.get(&key) {
            *class
        } else {
            let class = classify(point.map(f64::from));
            queries += 1;
            if cache.len() < CACHE_LIMIT {
                cache.insert(key, class);
            }
            class
        };
        classes.push(class);
    }
    Some((classes, queries))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicated_vertices_and_unchanged_frames_reuse_exact_point_results() {
        let request = Request {
            generation: 1,
            points: [[0., 0., 0.], [1., 0., 0.], [-0., 0., 0.]].repeat(10_000),
            tolerance: 0.05,
            cancel: CancelToken::new(),
        };
        let mut cache = PointCache::new();
        let (first, queries) = classify_points(&request, &mut cache, |p| {
            Some(if p[0] == 0. {
                SurfaceClass::WithinTolerance
            } else {
                SurfaceClass::Excess
            })
        })
        .unwrap();
        assert_eq!(queries, 2);
        let (next, queries) = classify_points(&request, &mut cache, |_| {
            panic!("cached position recomputed")
        })
        .unwrap();
        assert_eq!(queries, 0);
        assert_eq!(first, next);
        request.cancel.cancel();
        assert!(classify_points(&request, &mut cache, |_| panic!()).is_none());
    }
    #[test]
    fn superseded_comparison_responses_cannot_color_a_newer_stock_frame() {
        let (results, receiver) = mpsc::channel();
        let (wake, _requests) = mpsc::sync_channel(1);
        let mut comparison = StockComparison::default();
        comparison.worker = Some(Worker {
            inbox: Arc::new(Mutex::new(None)),
            wake,
            results: receiver,
        });
        comparison.invalidate();
        results
            .send(Response {
                generation: 0,
                classes: vec![Some(SurfaceClass::Gouge)],
            })
            .unwrap();
        results
            .send(Response {
                generation: 1,
                classes: vec![Some(SurfaceClass::WithinTolerance)],
            })
            .unwrap();
        assert_eq!(
            comparison.poll(),
            Some(vec![Some(SurfaceClass::WithinTolerance)])
        );
        assert!(!comparison.pending());
    }
}
