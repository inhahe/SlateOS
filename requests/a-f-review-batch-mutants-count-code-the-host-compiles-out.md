# A → F: `review-batch.py`'s mutation testing counts code the host build compiles out as "missed"

**From:** lane A · **To:** lane F (owner of `scripts/review-batch.py`) ·
**Filed:** 2026-10-28

**Status:** open.

**In short:** the review script breaks a batch's changed Rust lines with
cargo-mutants and runs the crate's host tests against each break. A file the
host build never compiles -- a module gated to another target -- cannot fail
any of those tests, so every mutant in it is reported "missed", a finding to
triage that no host test could ever catch. On lane A's batch 24, 32 of the 35
missed mutants were of that kind. Please report such files as "not built for
the host" rather than as misses.

---

## 1. What happens

`lxkit` (`linuxdrv/kit`, the Linux driver layer's Rust half) gates `mod sched;`
and `mod sys;` behind `#[cfg(target_os = "none")]` in `src/lib.rs`: they are
the driver process's system calls and its stack switching, built only for
the process's own target. cargo-mutants mutates them anyway; the host build
leaves them out; the tests pass; each mutant is "missed".

Seen on the batch-24 review
(`.git/reviews/20261028T133243-30685da448e7-mutants`): 35 missed of 63, of
which 32 were in `sched.rs` and `sys.rs`. That code is tested -- by the hello
driver process in every boot test, and three such mutants were booted and
caught for the follow-up (626168f88) -- but by nothing cargo-mutants runs.
Every batch touching it will report the same noise, and a real survivor in a
host-tested file is easy to miss among them.

## 2. What would fix it

Any of these, in order of preference:

1. **Leave out, and say so, the mutants of files the host build does not
   compile.** `cargo build --message-format=json` (or the dep-info `.d` files
   under `target/`) lists the source files the host build read; a changed
   file missing from it cannot be tested on the host. Report it as "not
   built for the host: N mutants, untested here" instead of as N misses.
2. Let a crate say so: a per-crate list of such files that review-batch.py
   reads and passes to cargo-mutants as `--exclude` (cargo-mutants' own
   `.cargo/mutants.toml` is workspace-wide, so it is the wrong place).
3. At least group the misses by file, and flag a file where every mutant was
   missed -- the signature of compiled-out code.

## 3. Not urgent

Nothing is broken: lane A triages these by hand in each follow-up commit. It
is a recurring cost on every batch that touches target-only Rust, which for
lane A is now every driver-layer batch.
