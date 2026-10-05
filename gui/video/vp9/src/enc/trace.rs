//! A test-only log of the realtime decisions, line for line in the format
//! the instrumented libvpx the port is checked against writes
//! (`tools/trace/libvpx_trace_patch.py` adds `VP9T` lines to its
//! `vp9_encodeframe.c`, `vp9_pickmode.c` and `vp9_encoder.c`), so that the
//! two logs can be compared and the first decision that differs found:
//! `tools/trace/README.md`.
//!
//! The log is per thread and off until [`start`]: encoding writes nothing
//! unless a test asks for it.

use std::cell::RefCell;

thread_local! {
    static TRACE: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
}

/// Start logging on this thread.
pub(crate) fn start() {
    TRACE.with(|t| *t.borrow_mut() = Some(Vec::new()));
}

/// The lines logged since [`start`], and logging off.
pub(crate) fn take() -> Vec<String> {
    TRACE.with(|t| t.borrow_mut().take().unwrap_or_default())
}

/// Log a line, if logging is on: `f` is only called then.
pub(crate) fn line(f: impl FnOnce() -> String) {
    TRACE.with(|t| {
        if let Some(v) = t.borrow_mut().as_mut() {
            v.push(f());
        }
    });
}
