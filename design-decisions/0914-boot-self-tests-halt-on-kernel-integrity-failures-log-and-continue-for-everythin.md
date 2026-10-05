## §914 — Boot self-tests: halt on kernel-integrity failures, log-and-continue for everything else

**Date:** 2026-09-07. **Decided by:** Operator. **Lane:** A.

**In short:** the kernel runs several hundred self-tests at boot. Until now,
every failure panicked the machine dead. The operator was asked whether a
user's computer should refuse to start because a cosmetic terminal flag
(`VERASE`) is wrong, and answered: **option D — split by severity.** A bad
memory-manager invariant still halts; a cosmetic mismatch prints a warning
and boots. Option A (gate all self-tests behind a boot flag, skip in
production) was explicitly not taken as an interim step — the operator went
straight to the end state.

**The question (A-Q3 in `open-questions.md`):** 567 kernel files contain
self-test functions with a total of ~12 674 assertion sites. Only ~299 use the
log-and-continue style. The four options were: (A) skip self-tests on
production boots via a boot flag; (B) always run but never panic; (C) keep
today's halt-everything behaviour as deliberate policy; (D) classify each
test — halt for structural integrity, log-and-continue for the rest.

**Operator's answer (verbatim):** "D" — keep assertions for checks about
kernel integrity, log-and-continue for the rest.

**Background the operator also received (and which shaped D-straight-away over
A-now-D-later):** (1) the OS has zero real-world users — it has never been
booted on hardware, so A's urgency is hypothetical; (2) A creates a
configuration divergence — the tested path (with self-tests) differs from the
shipped path (without), and the first hardware boot is exactly when you least
want them to differ; (3) D's real cost is one macro-level change (a
`#[severity]` attribute or classification table) plus incremental per-test
judgement, not 12 674 individual site edits.

**`Severity` governs the kernel, not the harness — and the name invites the
confusion.** Added 2026-09-09 after the distinction cost a boot test and four
wrong statements in a row. `Severity::Diagnostic` decides whether the *machine*
keeps booting, which is exactly what the operator asked for: a user's computer
should not refuse to start over a cosmetic terminal flag. It says nothing about
the *boot test*. `check_selftest_failures` in `scripts/boot-test.sh` greps the
serial log for `self-test failed` and fails the entire run on a match, with no
allowlist and no reference to severity.

Both behaviours are right and they are not in tension: a shipped kernel should
survive a cosmetic failure, and a development harness should refuse to call a
run green when a test failed. The trap is purely that one word appears to
answer both questions. It does not. A `Diagnostic` rung that fails will still
redden every lane's boot test until the failure is fixed — see
`kernel/src/main.rs`'s disabled `ctest-pty` rung, which is disabled for that
reason and not for being wrong.

**Migration progress, measured 2026-09-09:** 924 classified dispatch sites
(138 `Integrity`, 786 `Diagnostic`) against 1,319 `self_test*` functions in
`kernel/src`. The 15:85 split is the shape the decision predicted — structural
invariants are the minority.

**Implementation plan:** introduce a per-self-test severity classification
(e.g. `Integrity` vs `Diagnostic`). `Integrity` tests (memory manager, page
table, scheduler invariants, capability enforcement) keep the panic-on-failure
behaviour. `Diagnostic` tests (terminal flags, cosmetic checks, informational
self-tests) switch to log-and-continue. Migration is incremental — each
self-test is classified as it is touched, with the default being
halt-on-failure (the safe side) until classified.

**How to reverse.** Reclassify individual tests or revert to a blanket halt
policy by setting the default severity back to `Integrity`.

**Where it lives.** The classification will be per-test-function metadata;
the enforcement in `kernel/src/main.rs`'s self-test dispatch. See
`known-issues.md` → `TD-A-MOST-BOOT-SELF-TESTS-PANIC-THE-KERNEL-INSTEAD-OF-REPORTING`.
