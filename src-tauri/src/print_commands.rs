use std::{collections::VecDeque, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, time::{Duration, Instant}};
use serde::Serialize;
use tauri::{Manager, State};
use crate::{printing, service::PdfService};

#[derive(Clone, Default)]
pub struct PrintJobs(Arc<Mutex<JobState>>);
#[derive(Default)]
struct JobState {
    active: Option<(String, Arc<AtomicBool>)>,
    pending_cancellations: VecDeque<(String, Instant)>,
}
const MAX_PENDING_CANCELLATIONS: usize = 64;
const CANCELLATION_LIFETIME: Duration = Duration::from_secs(300);
fn validate_request(request_id: &str) -> Result<(), String> {
    if request_id.is_empty() || request_id.len() > 128 { return Err("Invalid print request.".into()); }
    Ok(())
}
impl JobState {
    fn expire(&mut self, now: Instant) {
        self.pending_cancellations.retain(|(_, created)| now.saturating_duration_since(*created) < CANCELLATION_LIFETIME);
    }
    fn cancel(&mut self, request_id: String, now: Instant) -> Result<(), String> {
        validate_request(&request_id)?;
        self.expire(now);
        if let Some((id, cancel)) = self.active.as_ref() {
            if id == &request_id { cancel.store(true, Ordering::Relaxed); return Ok(()); }
        }
        if self.pending_cancellations.iter().any(|(id, _)| id == &request_id) { return Ok(()); }
        if self.pending_cancellations.len() == MAX_PENDING_CANCELLATIONS { return Err("Too many pending print cancellations. Try again shortly.".into()); }
        self.pending_cancellations.push_back((request_id, now));
        Ok(())
    }
}
impl PrintJobs {
    fn reserve(&self, request_id: String) -> Result<Reservation, String> {
        validate_request(&request_id)?;
        let mut jobs = self.0.lock().map_err(|_| "Print state is unavailable")?;
        jobs.expire(Instant::now());
        if jobs.active.is_some() { return Err("Another print job is still active.".into()); }
        let cancelled = jobs.pending_cancellations.iter().position(|(id, _)| id == &request_id)
            .and_then(|index| jobs.pending_cancellations.remove(index)).is_some();
        let cancel = Arc::new(AtomicBool::new(cancelled));
        jobs.active = Some((request_id, cancel.clone()));
        Ok(Reservation { jobs: self.clone(), cancel })
    }
}
struct Reservation { jobs: PrintJobs, cancel: Arc<AtomicBool> }
impl Drop for Reservation {
    fn drop(&mut self) {
        if let Ok(mut jobs) = self.jobs.0.lock() {
            if jobs.active.as_ref().is_some_and(|(_, cancel)| Arc::ptr_eq(cancel, &self.cancel)) { jobs.active = None; }
        }
    }
}
struct CancelOnDrop(Arc<AtomicBool>);
impl Drop for CancelOnDrop { fn drop(&mut self) { self.0.store(true, Ordering::Relaxed); } }
#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PrintResult { Cancelled, Submitted { pages: usize } }

#[tauri::command]
pub async fn cancel_print(jobs: State<'_, PrintJobs>, request_id: String) -> Result<(), String> {
    jobs.0.lock().map_err(|_| "Print state is unavailable")?.cancel(request_id, Instant::now())
}

#[tauri::command]
pub async fn print_document(app: tauri::AppHandle, service: State<'_, PdfService>, jobs: State<'_, PrintJobs>, id: u64, revision: u64, current_page: usize, request_id: String) -> Result<PrintResult, String> {
    let reservation = jobs.reserve(request_id)?;
    let cancel = reservation.cancel.clone();
    let _cancel_on_drop = CancelOnDrop(cancel.clone());
    if cancel.load(Ordering::Relaxed) { return Ok(PrintResult::Cancelled); }
    let window = app.get_webview_window("main").ok_or("Application window is unavailable")?;
    let hwnd = window.hwnd().map_err(|error| error.to_string())?.0 as isize;
    let snapshot = service.begin_print(id, revision).await?;
    let token = snapshot.token;
    let worker = service.inner().clone();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let spawn = std::thread::Builder::new().name("pdf-print".into()).spawn(move || {
        let snapshot = snapshot;
        let _reservation = reservation;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| printing::print(hwnd, &snapshot.name, snapshot.pages, current_page, cancel, |page, width, height| worker.print_render_blocking(token, page, width, height))))
            .unwrap_or_else(|_| Err("The print worker stopped unexpectedly.".into()));
        let cleanup = tauri::async_runtime::block_on(worker.end_print(token));
        let result = result.and_then(|outcome| cleanup.map(|_| outcome));
        drop(_reservation);
        let _ = sender.send(result);
    });
    let result = match spawn {
        Ok(_) => receiver.await.map_err(|_| "The print worker stopped unexpectedly.".to_string()).and_then(|result| result),
        Err(error) => { let _ = service.end_print(token).await; Err(format!("Unable to start printing: {error}")) },
    };
    let result = result?;
    Ok(match result { printing::PrintOutcome::Cancelled => PrintResult::Cancelled, printing::PrintOutcome::Submitted { pages } => PrintResult::Submitted { pages } })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_before_reservation_is_consumed_without_cancelling_another_job() {
        let jobs = PrintJobs::default();
        jobs.0.lock().unwrap().cancel("later".into(), Instant::now()).unwrap();
        let first = jobs.reserve("first".into()).unwrap();
        assert!(!first.cancel.load(Ordering::Relaxed));
        assert!(jobs.reserve("later".into()).is_err());
        drop(first);
        let later = jobs.reserve("later".into()).unwrap();
        assert!(later.cancel.load(Ordering::Relaxed));
        assert!(jobs.0.lock().unwrap().pending_cancellations.is_empty());
        drop(later);
        assert!(jobs.0.lock().unwrap().active.is_none());
    }

    #[test]
    fn active_cancellation_is_idempotent_and_pending_ids_are_bounded_and_expire() {
        let jobs = PrintJobs::default();
        let active = jobs.reserve("active".into()).unwrap();
        let now = Instant::now();
        let mut state = jobs.0.lock().unwrap();
        for _ in 0..2 { state.cancel("active".into(), now).unwrap(); }
        assert!(active.cancel.load(Ordering::Relaxed));
        assert!(state.pending_cancellations.is_empty());
        for index in 0..MAX_PENDING_CANCELLATIONS { state.cancel(format!("request-{index}"), now).unwrap(); }
        state.cancel("request-0".into(), now).unwrap();
        assert_eq!(state.pending_cancellations.len(), MAX_PENDING_CANCELLATIONS);
        assert!(state.cancel("overflow".into(), now).is_err());
        state.cancel("fresh".into(), now + CANCELLATION_LIFETIME).unwrap();
        assert_eq!(state.pending_cancellations.len(), 1);
        assert_eq!(state.pending_cancellations[0].0, "fresh");
        drop(state);
        drop(active);
    }

    #[test]
    fn invalid_ids_never_consume_bookkeeping_and_reservations_release_on_unwind() {
        let jobs = PrintJobs::default();
        for invalid in [String::new(), "x".repeat(129)] {
            assert!(jobs.reserve(invalid.clone()).is_err());
            assert!(jobs.0.lock().unwrap().cancel(invalid, Instant::now()).is_err());
        }
        assert!(jobs.0.lock().unwrap().pending_cancellations.is_empty());
        let failure = std::panic::catch_unwind(|| {
            let _reservation = jobs.reserve("panic".into()).unwrap();
            panic!("simulated native print failure");
        });
        assert!(failure.is_err());
        assert!(jobs.reserve("after-error".into()).is_ok());
    }

    #[test]
    fn dropping_async_request_cancels_but_keeps_worker_reservation_until_cleanup() {
        let jobs = PrintJobs::default();
        let reservation = jobs.reserve("worker".into()).unwrap();
        let cancellation = CancelOnDrop(reservation.cancel.clone());
        let flag = reservation.cancel.clone();
        let (finish, wait) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || { wait.recv().unwrap(); drop(reservation); });
        drop(cancellation);
        assert!(flag.load(Ordering::Relaxed));
        assert!(jobs.reserve("too-early".into()).is_err());
        finish.send(()).unwrap();
        thread.join().unwrap();
        assert!(jobs.reserve("next".into()).is_ok());
    }
}
