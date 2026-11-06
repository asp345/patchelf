//! Debug output, enabled by `--debug` or the `PATCHELF_DEBUG` environment variable.

use std::sync::atomic::{AtomicBool, Ordering};

static ENABLED: AtomicBool = AtomicBool::new(false);

pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => { if $crate::log::enabled() { eprintln!($($arg)*); } };
}

/// Render bytes for messages; ELF strings are not guaranteed to be UTF-8.
pub fn show(bytes: &[u8]) -> std::borrow::Cow<'_, str> {
    String::from_utf8_lossy(bytes)
}
