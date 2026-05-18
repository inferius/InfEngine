//! Graceful-shutdown plumbing for the headless server.
//!
//! `install_signal_handler` registers a SIGINT / SIGTERM handler
//! that flips an atomic boolean. The main loop polls
//! `ShutdownToken::is_requested` and exits cleanly, giving the
//! server a chance to flush its audit log + write the world before
//! the process dies.
//!
//! Uses the `ctrlc` crate so the handler works on Windows (which
//! doesn't expose POSIX signal-hook semantics directly) and POSIX
//! alike. The single-shot handler matches the `ctrlc` library
//! contract: it can only be installed once per process.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Token shared between the signal handler and the main tick loop.
/// Cheap to clone (it's just an `Arc<AtomicBool>` under the hood).
#[derive(Debug, Clone)]
pub struct ShutdownToken {
    flag: Arc<AtomicBool>,
}

impl ShutdownToken {
    fn new() -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// True once a SIGINT or SIGTERM has flipped the flag. Cheap -
    /// just a `Relaxed` atomic load.
    pub fn is_requested(&self) -> bool {
        self.flag.load(Ordering::Relaxed)
    }

    /// Flip the flag. Public so a unit test can simulate a signal
    /// without installing the real handler.
    pub fn request(&self) {
        self.flag.store(true, Ordering::Relaxed);
    }
}

/// Install a process-wide SIGINT + SIGTERM handler. Returns a token
/// the main loop can poll. Idempotent across re-installation is NOT
/// guaranteed - the `ctrlc` library refuses a second registration.
pub fn install_signal_handler() -> Result<ShutdownToken, ctrlc::Error> {
    let token = ShutdownToken::new();
    let token_clone = token.clone();
    ctrlc::set_handler(move || {
        token_clone.request();
    })?;
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_starts_unrequested_and_flips_on_request() {
        let t = ShutdownToken::new();
        assert!(!t.is_requested());
        t.request();
        assert!(t.is_requested());
    }

    #[test]
    fn clones_share_the_flag() {
        let a = ShutdownToken::new();
        let b = a.clone();
        a.request();
        assert!(b.is_requested());
    }
}
