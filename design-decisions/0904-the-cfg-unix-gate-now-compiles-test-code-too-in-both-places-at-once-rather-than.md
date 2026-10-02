## §904 — the cfg(unix) gate now compiles test code too, in both places at once rather than staged

**Date:** 2026-09-03. **Decided by:** Claude (autonomous). **Lane:** A.

**In short:** SlateOS ships as a unix-family target, but development happens on
Windows, so code written inside `#[cfg(unix)]` — "compile this part only on
unix" — is never compiled by anything the developer runs. A gate exists to
close that: it compiles the whole workspace for a Linux target so those parts
get built at least once. It turns out it was building only about half of what
it should. Cargo builds a crate's library and its programs by default, and
*not* its tests, and in this tree the `#[cfg(unix)]` code is mostly *in* the
tests — the production code is written against the standard library, while the
tests are full of file permissions, symlinks and ownership fixtures that only
exist on unix. So the gate compiled the smaller half and reported success. The
decision is to switch on the flag that also builds tests, in both places that
run this gate at the same time, rather than trying it in the advisory one
first.

Requested, with every cost measured rather than estimated, in
`requests/b-a-the-cfg-unix-gate-skips-every-test-module.md`.

### Why this is not just a flag

The evidence lane B brought is the part that settles it. Running the gate's own
command *with* `--all-targets` against one crate found four hard compile errors
in `userspace/coreutils/src/bin/cp.rs` that had been in the tree for weeks —
all four inside `#[cfg(unix)] #[test]` helpers, all four nothing more exotic
than a missing import. Three of them were found by eye first, by someone who
then reasonably believed they had found all of them; the fourth was a different
name in a different function eighty lines further down, and only a compiler
found it.

That is the whole argument for the change. A gate that compiles the arm is not
a stricter version of careful reading. It is a different instrument, and it is
the only one that works on this class of defect.

### `--exclude kernel` is not a scope reduction

`--all-targets` adds each crate's `test` target. A `test` target links the test
harness, the harness pulls `std`, and `std` already defines the `panic_impl`
lang item — so a `#![no_std]` binary that supplies its own `#[panic_handler]`
cannot have a `test` target on a hosted triple at all:

```
error[E0152]: found duplicate lang item `panic_impl`
    --> kernel/src/main.rs:7963:1
```

This is structural. `kernel` is the only crate in the workspace affected: seven
crates define a `#[panic_handler]`, but the other six are the `services/*`
binaries, which the workspace's own `exclude` list already keeps out. And
nothing is lost by excluding it, because the kernel is `no_std` and therefore
has no `cfg(unix)` arms for this gate to check in the first place.

Spelling it as `--exclude` rather than naming the hosted crates positively with
`-p` is deliberate, and the reason is the failure mode. If a second bare-metal
binary is ever added *inside* the workspace, `--exclude kernel` breaks loudly
on the next run and someone deals with it. A positive `-p` list would silently
under-cover instead — the new crate simply would not be checked, and nothing
would say so. Given that this entire entry is about a gate that under-covered
in silence for weeks, the loud failure is the one to choose.

### Decision: both call sites at once, not staged through the advisory one

The gate runs in two places: `scripts/boot-test.sh` → `check_cfg_unix`, which
blocks a merge, and `scripts/pre-boot.py`, where a failure outside lane A is
advisory. Lane B offered a staged rollout — the advisory one first, the
blocking one later.

| | Stage it | Both at once |
|---|---|---|
| *What changes:* | new findings appear as advice for a while before they can block anyone | the next boot test is the first run, and it either passes or it does not |
| Protects against | unknown deny-level findings in three lanes' unseen test code | — |
| Costs | the blocking gate keeps under-covering for as long as the stage lasts | one run's worth of surprise, if the measurement was wrong |

Staging protects against exactly one risk: that compiling three lanes' test
code for the first time turns up deny-level lints that then red the shared
build. That risk was measured, not guessed —
`clippy --workspace --exclude kernel --all-targets` over this tree gave **0
errors and 1,857 warnings**, and the warnings stay warnings (`unwrap`,
`expect`, `panic` and `indexing` are `warn` by CLAUDE.md's own rule). Lane B
then left the workspace green under that exact command.

So the thing staging would buy has already been bought by the measurement, and
what staging costs is real: the blocking gate goes on reporting OK about half a
job for however long the stage lasts, which is the defect this entry exists to
fix. A rollout designed to manage a risk of zero is just a delay.

The cost accepted is 508 s of one-time compilation on a cache that already held
every crate's lib and bin. Steady state afterwards, measured on the crate with
the most test code in the tree, is 46 s.

### What reversing this looks like

Delete `--exclude kernel --all-targets` from the `"$CARGO" clippy` line in
`check_cfg_unix` (`scripts/boot-test.sh`) and from the `_run([cargo, "check",
…])` call in `scripts/pre-boot.py`, and the gate returns to compiling libs and
bins only. The signal that this is worth reconsidering is the flag turning up
deny-level findings in another lane's test code faster than that lane fixes
them — at which point the blocking site should drop back and the advisory site
should keep the flag, because the detection is still worth having even when the
veto is not.
