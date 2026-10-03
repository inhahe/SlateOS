### TD-OILS-SUBSHELL-TRAP-DISPLAY. `osh` subshells drop parent trap *strings*, so `trap -p` inside a subshell shows nothing (bash keeps the strings for display while resetting their firing disposition) — FIXED 2026-07-20 (was already implemented; entry was stale)

**Where:** `userspace/oils/src/interp.rs` — `clone_for_subshell` (the `traps`
field: currently filters to keep only ignored `''` traps and drops the rest).

**What:** in bash a subshell (a `(…)` group, a pipeline stage, or a command
substitution) **displays** the parent's trap command strings via `trap -p`/bare
`trap`, even though the trap's *firing disposition* is reset to default (so an
actual signal runs the default action, not the handler). Measured (bash 5.2):

```
$ trap 'echo x' INT; (trap -p)
trap -- 'echo x' SIGINT          # string shown in the subshell
$ trap 'echo x' INT; trap -p | cat
trap -- 'echo x' SIGINT          # shown across a pipeline stage too
$ trap 'echo H' USR1; (kill -USR1 $BASHPID); …
User defined signal 1            # default action ran — handler did NOT fire
```

osh keeps only ignored (`''`) traps in the subshell clone, so `trap -p | cat`
prints an empty result where bash prints the inherited line. The *firing*
semantics osh already models correctly: it never fires an inherited non-ignored
trap in a subshell (and osh has no async signal delivery at all yet).

**Impact:** low — visible only when a script inspects traps (`trap -p`/`trap`)
from **inside** a subshell (pipeline stage, `( )`, or `$( )`). Handler firing is
already correct.

**Proper fix:** split "trap string for display" from "active disposition." Keep
*all* parent trap strings in `clone_for_subshell` (so `trap -p` reflects them),
but mark the non-ignored ones reset-in-subshell so they do not fire. For the
synchronous pseudo-signals this must honour bash's inheritance rules —
`DEBUG`/`RETURN` fire in a subshell only under `functrace` (`set -T`), `ERR`
only under `errtrace` (`set -E`); by default they display but do not fire.
Naively keeping the strings *without* that guard would wrongly fire
`DEBUG`/`ERR`/`RETURN` handlers inside subshells, so the guard is required, which
is why this is deferred rather than a one-line clone change.

**Fix (verified 2026-07-20 — the code already did this; the entry lagged):**
`clone_for_subshell` already implements the display-vs-disposition split via a
dedicated `trap_shadow: HashMap<String, String>` field. The active `traps` map
keeps only the traps that still *fire* in a subshell — ignored (`''`) traps
always, plus `DEBUG`/`RETURN` under `functrace` and `ERR` under `errtrace` — and
every non-inherited trap string is moved into `trap_shadow` (merged with any the
parent already carried). `trap_print` (`trap -p`) and the bare `trap` listing
merge `trap_shadow` in for any signal not already active, so a `( … )` group, a
pipeline stage, and a command substitution all *display* the inherited strings
while never firing the reset handlers — exactly bash's behaviour. Setting a trap
inside the subshell replaces its shadow; `trap - SIG` clears it. Regression
coverage: `subshell_lists_inherited_traps_but_does_not_fire_them` (now also
asserts the pipeline-stage `trap -p | cat` display case and the default-`DEBUG`
display-but-don't-fire case) and `subshell_exit_trap_fires`. Verified end-to-end
against the built binary: `trap 'echo x' INT; (trap -p)` and `trap -p | cat`
both print `trap -- 'echo x' SIGINT`.
