## A-KSHELL-A-MISTYPED-COMMAND-REPORTED-SUCCESS — ✅ FIXED 2026-08-24 (lane A)

**In short:** type a kernel-shell command wrongly — leave off an argument,
give it a word where it wanted a number — and it printed `Usage: …`, did
nothing at all, and then reported that it had succeeded. Anything deciding
what to do next based on that answer decided wrong: `cmd || fallback` never
ran the fallback, `set -e` never stopped, and an `ERR` trap never fired, for
a command that had not run.

**Where:** `kernel/src/kshell.rs` — **710 sites across 194 `cmd_*`
functions**. The shape was uniform: one or more `shell_println!("Usage: …")`
lines followed by a bare `return;`, with no `set_exit(1)` anywhere between.
`dispatch` sets the status to 0 on entry and expects the command to raise it,
so a command that returns without raising it has claimed success.

**The value was not a judgement call.** 42 usage arms in the same file
already called `set_exit(1)`. The other 710 were not a different policy, they
were the same policy unimplemented, so the fix is a consistency sweep and not
a behaviour change anyone chose.

**One deliberate exclusion**, found by sampling the sites rather than trusting
the pattern: `cmd_ksyms` with no argument prints a symbol count *and* a usage
line, but it has done its job — that is its no-argument output, not a refusal.
It keeps reporting 0. Its sibling arm ("Invalid address") is a real failure
and was swept in with the rest.

**Covered by** kshell self-test §16, which asserts a top-level arity guard
(`kill`), a subcommand arity guard several match arms deep
(`fssnapshot info`), a parse failure (`ksyms zzz`), the `ksyms` exclusion, and
— so the whole section cannot pass by everything failing — that a command
which actually worked still reports 0.

**Still open in this class:** roughly 231 further sites where the usage
message and the `return` have other statements between them. Those do not fit
a pattern rule and need reading one at a time; the sweep deliberately did not
touch them rather than guess.

**Lesson:** this is the third silent-success bug in the shell in two days,
after the pipeline exit-status fixes and `A-KSHELL-CAPTURE-DID-NOT-NEST`. The
recurring shape is not "the wrong value was computed" but "no value was
computed and the default was reported as if it were one." A default of
success is a claim, and every early `return` past it is an unexamined
assertion that the claim still holds.
