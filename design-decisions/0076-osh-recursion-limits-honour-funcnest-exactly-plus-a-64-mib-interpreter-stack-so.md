## 76. osh recursion limits — honour `FUNCNEST` exactly, plus a 64 MiB interpreter stack so legitimate deep recursion matches bash instead of aborting at ~300 frames

**Date:** 2026-07-19
**Decided by:** Claude (autonomous) — surfaced by a probe (`FUNCNEST=5;
f(){ f; }; f`) showing osh crashing with a native stack overflow where bash
prints a graceful error. Operator may revisit the stack size.

**Context.** osh is a tree-walking interpreter: each nested shell-function
call (and each compound command) recurses natively through
`exec_items → exec_and_or → exec_pipeline → call_function → exec_program → …`.
Those frames are large, so on the ~1 MiB default main-thread stack osh
overflowed and aborted (`thread 'main' has overflowed its stack`, exit 127)
after only ~200–400 nested calls — far short of the ~2000–4000 bash tolerates
before it segfaults. Two separate problems: (a) no `FUNCNEST` support at all,
so a user's explicit recursion guard was silently ignored; (b) even without
`FUNCNEST`, legitimate deep recursion (recursive shell algorithms) crashed
much sooner than bash.

**Decision.** (1) Implement `FUNCNEST`: when set to a positive integer N, refuse
the (N+1)th nested call with bash's exact diagnostic and a fatal
`jump_to_top_level(DISCARD)` (`Flow::Discard` — aborts the rest of the current
top-level command, bypassing `&&`/`||`/`;`, bounded by the nearest subshell,
then resumes at the next parse unit). 0 / empty / non-numeric ⇒ unlimited.
(2) Run the interpreter on a dedicated thread with a **64 MiB** reserved stack
(`INTERP_STACK_SIZE` in `main.rs`), which raises the crash threshold to
~6700+ levels (debug) — comfortably past bash — while `FUNCNEST` provides the
graceful ceiling when the user wants one.

**Rationale.** *Pro:* `FUNCNEST` is exact bash behaviour (byte-matched for
same-line abort, `||` non-catch, subshell containment, multi-line resume);
the big stack removes an easy DoS/robustness footgun where a shell aborts
uncleanly on a few hundred levels of legitimate recursion. The 64 MiB is
*reserved* virtual address space grown on demand via guard pages — not
eagerly committed — so it is cheap on the host and on SlateOS (whose
"committed by default" policy governs heap/mmap, not thread-stack reservation).
*Con:* osh still eventually aborts on truly-unbounded recursion (like bash's
segfault), just later; and 64 MiB is a magic number — too small for a
pathological workload, wasteful if a future SlateOS std eagerly commits stacks.

**Alternatives considered.** (a) A low default internal recursion cap that
converts overflow into a graceful error even without `FUNCNEST` — rejected:
any fixed cap either breaks legitimate deep recursion (too low) or still
crashes (too high), and it would diverge from bash, which has no such default.
(b) Leave the default 1 MiB stack and only add `FUNCNEST` — rejected: crashing
at ~300 legitimate levels is a real robustness defect for an OS shell.
(c) Convert the tree-walk to an explicit heap-allocated work stack (no native
recursion) — the truly principled fix, but a large rewrite; deferred.

**Where it lives.** `userspace/oils/src/interp.rs` — `funcnest_limit()`, the
guard at the top of `call_function` returning `Flow::Discard`;
`userspace/oils/src/main.rs` — `INTERP_STACK_SIZE`, the large-stack thread in
`main()`. Test: `interp.rs::funcnest_caps_recursion_like_bash`.

**How to reverse.** Drop the guard in `call_function` (and `funcnest_limit`) to
remove `FUNCNEST`; change or remove `INTERP_STACK_SIZE` / the thread wrapper in
`main()` to alter the stack. If (c) is ever done, both become unnecessary.
