//! A87: the editor in two languages.
//!
//! Every label goes through [`t`], which picks between the English and
//! the Czech spelling of the same thing. The choice is one global read
//! a million times and written once from the View menu - a language is
//! process-wide state in the truest sense, and threading it through
//! every panel signature would be ceremony without safety, since the
//! worst a stale read can do is draw one frame's label in the other
//! language.
//!
//! Only the editor's own chrome is translated here. Project data -
//! room names, script ids, the game's lines - stays whatever the
//! project says; the game's languages are the localisation system's
//! business, not this switch's.

use std::sync::atomic::{AtomicBool, Ordering};

static CZECH: AtomicBool = AtomicBool::new(false);

/// Switch the interface language.
pub fn set_czech(on: bool) {
    CZECH.store(on, Ordering::Relaxed);
}

/// Whether the interface is currently Czech.
pub fn is_czech() -> bool {
    CZECH.load(Ordering::Relaxed)
}

/// The label in the interface language.
pub fn t(en: &'static str, cs: &'static str) -> &'static str {
    if is_czech() { cs } else { en }
}

/// Tests that flip the language hold this, so parallel tests cannot
/// read the flag mid-flip. The lock is test-only: the editor itself
/// writes the flag from one place and a stale frame is harmless.
#[cfg(test)]
pub(crate) fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_picks_the_spelling() {
        let _guard = test_lock();
        // One process-wide flag, so the test puts it back where it
        // found it.
        let before = is_czech();
        set_czech(false);
        assert_eq!(t("Room", "Místnost"), "Room");
        set_czech(true);
        assert_eq!(t("Room", "Místnost"), "Místnost");
        set_czech(before);
    }
}
