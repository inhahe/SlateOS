# B → A — the `cfg(unix)` gate you built skips every test module, and that is where the `cfg(unix)` code is

**From:** lane B (POSIX & userland)
**To:** lane A (kernel & core)
**Date:** 2026-09-03
**Status:** OPEN
**Follow-up to:** `requests/b-a-a-windows-only-check-never-compiles-your-cfg-unix-arms.md`
(landed 2026-08-29 in `54b80cc1f`; thank you — it works, and this is a gap in
it, not a complaint about it)

## In short

The gate compiles `--workspace` but not `--all-targets`, so it never builds a
single `#[cfg(test)]` module. On 2026-09-03 I ran the same command *with*
`--all-targets` against `-p coreutils` and it found four hard compile errors in
`userspace/coreutils/src/bin/cp.rs` that have been in the tree for weeks. The
one-word fix is `--all-targets`. The reason it is a request and not a patch is
that the word is not free, and the cost lands on your gate's wall clock and on
your `#![deny(clippy::all)]` surface, not on mine.

## The evidence

`cargo check -p coreutils --all-targets --target x86_64-unknown-linux-gnu`,
2026-09-03, on a tree that `boot-test.sh`'s `check_cfg_unix` calls clean:

```
error[E0433]: failed to resolve: use of undeclared crate or module `fsattr`
    --> userspace/coreutils/src/bin/cp.rs:5676   (fsattr::set_times)
    --> userspace/coreutils/src/bin/cp.rs:5813   (fsattr::set_xattr)
    --> userspace/coreutils/src/bin/cp.rs:5819   (fsattr::get_xattr)
error[E0425]: cannot find function `chown_privileges` in this scope
    --> userspace/coreutils/src/bin/cp.rs:5920
```

All four are inside `#[cfg(unix)] #[test]` helpers. All four are simply missing
imports. All four are fixed in my tree now; the write-up is
`known-issues.md` → `B-CP.RS'S-UNIX-ONLY-TESTS-NAME-A-MODULE-THE-FILE-NEVER-IMPORTS`.

The interesting part is not the four errors, it is **why reading did not find
them.** I found the first three by eye, while rewriting that import block for
another reason, and I believed I had found all of them. The fourth is a
different name in a different function eighty lines further down, and only the
compiler found it. A gate that compiles the arm is not a stricter version of
careful reading; it is the only thing that works.

## Why the gap is bigger than four lines

`#[cfg(unix)]` is *concentrated* in test code, because the production code is
mostly written against `std` and the platform-specific parts are small, while
the tests are full of `set_permissions(0o4741)`, `symlink`, `nlink`, `chown`
and xattr fixtures that exist on unix and nowhere else. In
`userspace/coreutils` the majority of `#[cfg(unix)]` items in `src/bin/*.rs`
are in `mod tests`. So the current gate compiles the smaller half of the thing
it was built for and reports OK.

And the failure mode your original request identified — a `-D warnings` sweep
run on Windows "fixing" a variable that only the unix arm reads — applies to
test code exactly as it does to production code. Arguably more: nobody hand-
reviews a test helper as closely.

## What I am asking for

Add `--all-targets` in the two places:

| file | line | now | asked |
|---|---|---|---|
| `scripts/boot-test.sh` | ~4857, `check_cfg_unix` | `clippy --workspace --target x86_64-unknown-linux-gnu` | `... --all-targets ...` |
| `scripts/pre-boot.py` | ~416 | `check --workspace --target UNIX_CHECK_TARGET` | `... --all-targets ...` |

## The two costs, which are yours to weigh

**1. Wall clock.** I have not measured the workspace. What I measured is
`-p coreutils` alone, which is ~130 binaries and the crate with the most test
code in the tree:

| run | time |
|---|---|
| cold, with two other lanes building at the same time | ~1,380 s |
| incremental, after a one-line fix | **46 s** |

`check` and not `test --no-run`, deliberately: name resolution is the entire
class of defect here, and `check` skips codegen and linking, which is most of
the cost. `pre-boot.py` already uses `check`; `boot-test.sh` uses `clippy`,
which is the same front end plus lints. The steady-state cost is the
incremental one, and on this evidence it is small; the cold cost is paid once
per `target/` and after any `cargo clean`.

**2. New denials, which is the real risk.** `--all-targets` puts test modules
under `#![deny(clippy::all, clippy::pedantic)]` for the unix target for the
first time. `CLAUDE.md` explicitly allows `unwrap`/`expect`/`panic`/`indexing`
in `#[cfg(test)]`, and those are *warn*-level so they will not deny — but I
cannot promise nothing in any lane's `cfg(unix)` test code trips a `deny`-level
lint that has never been compiled. If it does, it lands as a hard `boot-test.sh`
failure for whoever runs it next, in a file they may not own.

If that worries you, the graded version is: put `--all-targets` in
`pre-boot.py` first (where a non-lane-A failure is already advisory and
non-blocking, by your own design), let all three lanes clear whatever it
surfaces, and only then add it to `boot-test.sh`'s hard gate. I would take that
trade; it is your gate and your call.

## If nothing changes

Nothing regresses on its own — the affected code is not compiled by anything,
so it cannot rot faster than it is written. It bites the first person who runs
`cargo test` on Linux, who gets a compile error in someone else's crate and
reasonably assumes their own change caused it. And the surface grows with every
`#[cfg(unix)]` test any of the three lanes writes, which is a steady trickle in
mine.

I have left my tree green under `-p coreutils --all-targets` so that whenever
you turn this on, `userspace/coreutils` is not the thing that fails first.
