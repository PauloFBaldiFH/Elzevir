// SPDX-License-Identifier: GPL-3.0-or-later
//! Graceful completion followed by cooperative cancellation, without a timer thread.
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
pub(crate) type Control = Arc<dyn Fn() -> bool + Send + Sync>;
#[derive(Clone, Default)]
pub(crate) struct Cancellation(Arc<Mutex<Option<Instant>>>);
impl Cancellation {
    pub(crate) fn stop(&self) {
        if let Ok(mut at) = self.0.lock()
            && at.is_none()
        {
            *at = Some(Instant::now());
        }
    }
    pub(crate) fn control(&self) -> Control {
        let at = Arc::clone(&self.0);
        Arc::new(move || {
            at.lock().map_or(true, |at| {
                at.is_some_and(|at| at.elapsed() >= Duration::from_secs(20))
            })
        })
    }
}
pub(crate) fn check(control: &Control) -> Result<(), String> {
    if control() {
        Err("operation interrupted during service shutdown".to_owned())
    } else {
        Ok(())
    }
}
pub(crate) struct Reader<R> {
    pub source: R,
    pub control: Control,
}
impl<R: std::io::Read> std::io::Read for Reader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if (self.control)() {
            return Err(std::io::Error::other("operation interrupted"));
        }
        self.source.read(buffer)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grace_allows_small_jobs_to_finish_and_then_interrupts_reads() {
        use std::io::Read as _;
        let cancel = Cancellation::default();
        let control = cancel.control();
        cancel.stop();
        assert!(!control());
        *cancel.0.lock().unwrap() =
            Some(Instant::now().checked_sub(Duration::from_secs(21)).unwrap());
        assert!(control());
        let mut reader = Reader {
            source: b"fictional".as_slice(),
            control,
        };
        assert!(reader.read(&mut [0; 16]).is_err());
    }
}
