//! Revision-stamped import workers. Cancellation discards results even when a
//! native translator cannot interrupt its current call.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};

pub(crate) struct ImportJobs<T> {
    serial: u64,
    active: Option<Active<T>>,
}
struct Active<T> {
    request: u64,
    epoch: u64,
    cancelled: Arc<AtomicBool>,
    receiver: mpsc::Receiver<Result<T, String>>,
}
impl<T: Send + 'static> Default for ImportJobs<T> {
    fn default() -> Self {
        Self {
            serial: 0,
            active: None,
        }
    }
}
impl<T: Send + 'static> ImportJobs<T> {
    pub fn start(
        &mut self,
        epoch: u64,
        work: impl FnOnce() -> Result<T, String> + Send + 'static,
    ) -> u64 {
        self.cancel();
        self.serial = self.serial.wrapping_add(1);
        let request = self.serial;
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancelled.clone();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
                .unwrap_or_else(|_| Err("CAD import worker failed".into()));
            if !worker_cancel.load(Ordering::Acquire) {
                let _ = sender.send(result);
            }
        });
        self.active = Some(Active {
            request,
            epoch,
            cancelled,
            receiver,
        });
        request
    }
    pub fn pending(&self) -> bool {
        self.active.is_some()
    }
    pub fn cancel(&mut self) {
        if let Some(active) = self.active.take() {
            active.cancelled.store(true, Ordering::Release);
        }
    }
    pub fn poll(&mut self, epoch: u64) -> Option<Result<T, String>> {
        let active = self.active.as_ref()?;
        if active.epoch != epoch || active.request != self.serial {
            self.cancel();
            return Some(Err(
                "Import discarded because the project changed; import again".into(),
            ));
        }
        match active.receiver.try_recv() {
            Ok(result) => {
                self.active = None;
                Some(result)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.active = None;
                Some(Err("CAD import worker stopped without a result".into()))
            }
        }
    }
}
impl<T> Drop for ImportJobs<T> {
    fn drop(&mut self) {
        if let Some(active) = &self.active {
            active.cancelled.store(true, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_edits_discard_even_a_completed_import() {
        let mut jobs = ImportJobs::default();
        let (tx, rx) = mpsc::channel();
        jobs.start(7, move || {
            tx.send(()).unwrap();
            Ok(42)
        });
        rx.recv().unwrap();
        assert!(jobs.poll(8).unwrap().is_err());
        assert!(!jobs.pending());
    }
    #[test]
    fn cancellation_and_new_requests_cannot_publish_old_results() {
        let mut jobs = ImportJobs::default();
        let (tx, rx) = mpsc::channel();
        let first = jobs.start(0, move || {
            rx.recv().unwrap();
            Ok(1)
        });
        let second = jobs.start(0, || Ok(2));
        assert!(second > first);
        tx.send(()).unwrap();
        loop {
            if let Some(result) = jobs.poll(0) {
                assert_eq!(result.unwrap(), 2);
                break;
            }
            std::thread::yield_now();
        }
        jobs.start(0, || Ok(3));
        jobs.cancel();
        assert!(jobs.poll(0).is_none());
    }
}
