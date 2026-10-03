## §102 — osh's last-resort recursion ceiling is a measured stack budget, not a nesting count

**Date:** 2026-08-03

**Decided by:** Claude (autonomous)

**Context.** osh's evaluator is a recursive tree-walker, so shell-level
recursion is native recursion. With `FUNCNEST` unset — the default — nothing
bounded it, and `f() { f; }; f` ran until the thread's stack was exhausted. A
Rust stack overflow is an immediate `abort()`: no `trap`, no `EXIT` handler, no
flushed output, and nothing an embedder could survive. The reference bash is no
better here (it segfaults, status 139), so there was no parity answer to copy —
the ceiling and its wording are ours to choose.

**Decision.** Guard on *measured stack consumption*. `Shell::new` records the
stack address it was built at; the binary, which is the only party that knows
how big the thread's stack is, calls `Shell::set_stack_budget` with three
quarters of it; `Shell::exec_command` refuses to descend once the current frame
is that far below the origin, reporting `maximum nesting level exceeded (out of
stack)` with status 1 and letting the recursion unwind. The budget defaults to
`None` — unguarded — so an embedder that never states its stack size keeps the
old behaviour. `FUNCNEST` is untouched and remains the lower, explicit,
bash-compatible ceiling.

**Rationale.** A shell level costs a different number of Rust frames depending
on how it was reached — a function call, an `eval`, a nested compound command —
so no single depth number is right for all of them: it would be too low for the
cheap path and too high (i.e. still an overflow) for the expensive one. The
measured budget is correct for every path by construction, scales on its own
with a release build or a bigger stack, and needs no re-tuning when the
evaluator's frames change size. Placing it in `exec_command` covers every nested
construct at one point, so no re-entrant path can be forgotten.

**Alternatives considered.**
- *A fixed depth counter.* Simpler and cheaper, but wrong for the reason above,
  and it would need re-calibrating whenever a frame grew.
- *A default `FUNCNEST`.* Would change a documented, bash-compatible variable's
  meaning, and would not catch `eval` or compound-command recursion at all.
- *Growing the stack further.* Only moves the cliff; the process still aborts.
- *Unwinding to the top level (bash's `jump_to_top_level(DISCARD)`, which is
  what `FUNCNEST` does).* Rejected because the guard is a resource limit rather
  than a user-stated one: failing the single command keeps `&&`/`||` and the
  enclosing script working, which is the more conservative behaviour for
  something the script never asked for.

**Where it lives.** `userspace/oils/src/interp.rs` — `stack_mark`,
`Shell::stack_base`/`stack_budget`, `Shell::set_stack_budget`,
`Shell::stack_exhausted`, the guard at the top of `Shell::exec_command`;
`userspace/oils/src/main.rs` — `INTERP_STACK_SIZE`, `FALLBACK_STACK_SIZE`,
`stack_budget`, `run`.

**How to reverse.** Stop calling `set_stack_budget` in `main.rs`; the guard is
inert without it. To change the ceiling instead, adjust `stack_budget`'s
fraction (or `INTERP_STACK_SIZE`).
