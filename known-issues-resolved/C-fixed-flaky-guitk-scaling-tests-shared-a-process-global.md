## FIXED: FLAKY-GUITK-SCALING-TESTS-SHARED-A-PROCESS-GLOBAL

**What.** Five tests in `gui/toolkit/src/scaling.rs` — `global_scale_default_is_1`,
`set_and_get_global_scale`, `global_scale_clamped`, `per_monitor_override`,
`per_monitor_clear_falls_back` — each wrote the process-wide `SCALE_TABLE` and
then asserted on it. Cargo runs tests on parallel threads, so
`per_monitor_clear_falls_back` (which sets the global scale to 1.5) failed
whenever another of the five reset it to 1.0 in between. Observed failing once
in a full `cargo test -p guitk` run and passing when run alone.

**Fix.** A `SCALE_LOCK` mutex in the test module, taken by a `ScaleGuard` whose
`Drop` restores the whole table. Restoring on drop rather than at the end of
each test body means a failing assertion — which unwinds — still leaves clean
state, so one failure cannot cascade. The lock is taken with
`unwrap_or_else(|e| e.into_inner())` because a poisoned lock carries no
information once the guard restores the state anyway.

**Residual gap.** `DesktopShell::set_appearance` now publishes the display
scaling into that same process-global table, so `guitk` widgets hosted in the
shell lay out at the scale the chrome is drawn at. That one line has no unit
test of its own: every desktop test that builds a shell writes the value an
assertion would read, and `desktop` is a binary crate so the assertion cannot
be moved to an out-of-process integration test. Rationale is recorded on the
method.

**Where.** `gui/toolkit/src/scaling.rs` (test module);
`gui/desktop/src/main.rs` — `DesktopShell::set_appearance`.
