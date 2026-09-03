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
`userspace/coreutils/src/bin/cp.rs` that have been in the tree for weeks.

The fix is `--exclude kernel --all-targets` — two words, not one; the first
paragraph of "What I am asking for" explains why the second is needed and why
`kernel` is the only crate that needs excluding. **Both costs are now measured
rather than guessed**: 508 s one-time for `pre-boot.py`'s `check`, and **zero**
new denials across the whole workspace under `boot-test.sh`'s `clippy`. It is
still a request and not a patch because both files are yours, but there is
less to weigh than I thought when I started writing this.

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

## What I am asking for — two words, not one

I said above the fix was one word. I then ran it against the whole workspace
rather than against `-p coreutils`, and it is two, because `--all-targets`
alone **does not build**:

```
error[E0152]: found duplicate lang item `panic_impl`
    --> kernel/src/main.rs:7963:1
     = note: the lang item is first defined in crate `std` (which `test` depends on)
error: could not compile `kernel` (bin "kernel" test) due to 1 previous error
```

`--all-targets` adds each crate's `test` target, and a `test` target links the
harness, which pulls `std`, which already defines `panic_impl` — so a
`#![no_std]` binary that supplies its own panic handler cannot have a `test`
target on a hosted triple at all. This is structural, not a lint, and no
amount of fixing test code makes it go away.

**`kernel` is the only crate in the workspace this hits.** I checked: seven
crates in the tree define a `#[panic_handler]` — `kernel` and the six
`services/*` binaries — but `services` is in the workspace's `exclude` list
(root `Cargo.toml` ~line 183), so the gate never saw them and still doesn't.
The ask is therefore:

| file | line | now | asked |
|---|---|---|---|
| `scripts/boot-test.sh` | ~4857, `check_cfg_unix` | `clippy --workspace --target x86_64-unknown-linux-gnu` | `... --workspace --exclude kernel --all-targets ...` |
| `scripts/pre-boot.py` | ~416 | `check --workspace --target UNIX_CHECK_TARGET` | `... --workspace --exclude kernel --all-targets ...` |

`--exclude` loses nothing the gate has today: without `--all-targets` the
kernel's only targets are its lib and its bin, and *neither is what this gate
is for* — the kernel is `no_std`, so it has no `cfg(unix)` arms to check. If
you would rather keep it covered anyway, the belt-and-braces form is to leave
the present command exactly as it is and add the `--all-targets --exclude
kernel` one beside it; the second is a superset of the first everywhere except
`kernel`, so the pair costs one extra crate's worth of work.

The failure mode of `--exclude` is worth stating because it is the good one:
if someone later adds a second bare-metal binary **inside** the workspace, the
gate breaks loudly on the next run rather than quietly skipping it. A gate
that fails when the world changes is the kind you want; the alternative
spelling — naming the hosted crates positively with `-p` — under-covers
silently instead.

## The two costs, which are yours to weigh

**1. Wall clock.** Measured on the workspace, not extrapolated. All four
numbers below are from this machine on 2026-09-03, same tree, same
`target/x86_64-unknown-linux-gnu/`, in the order shown:

| run | time |
|---|---|
| `check --workspace` — your gate as it stands today, inside a full `pre-boot.py` | **698 s** |
| `check --workspace --exclude kernel --all-targets` — the ask, on the cache the previous line left warm | **508 s** |
| `clippy --workspace --exclude kernel --all-targets` — your `boot-test.sh` gate, as asked | **1,513 s** |
| `check -p coreutils --all-targets`, cold, with two other lanes building | ~1,380 s |
| `check -p coreutils --all-targets`, incremental after a one-line fix | **46 s** |

Read line 2 as the one-time price of turning this on for `pre-boot.py`: 508 s
of new compilation, on top of a cache that already had every crate's lib and
bin. It is not 508 s added to every run — the steady-state number is the last
line, and `-p coreutils` is the crate with the most test code in the tree.

Line 3 is a full clippy pass and looks worse than it is: `check` and `clippy`
invalidate each other's fingerprints in a shared `target/`, so *every*
`boot-test.sh` clippy run over this triple already pays a rebuild whether or
not `--all-targets` is there. What `--all-targets` adds on top is the test
targets, which is the same delta as line 2.

`check` and not `test --no-run`, deliberately: name resolution is the entire
class of defect here, and `check` skips codegen and linking, which is most of
the cost.

**2. New denials — I said this was the real risk. I measured it, and it is
zero.**

```
cargo clippy --workspace --exclude kernel --all-targets \
      --target x86_64-unknown-linux-gnu --message-format=short
  -> 0 errors, 1857 warnings, 1513 s   (2026-09-03, this tree)
```

Nothing in any of the three lanes' `cfg(unix)` test code trips a `deny`-level
lint. The 1,857 are warnings and stay warnings —
`unwrap`/`expect`/`panic`/`indexing` are `warn` by `CLAUDE.md`'s own rule, and
the pedantic ones that do fire are in `apps/` production code that the current
gate already sees. So the graded rollout I offered below is available but, on
this evidence, unnecessary: you can put the flag in both places at once.

**One caveat, and it is the interesting part.** That run is clean only because
of a fix I had to make first. The gate command *as it stands today* — no
`--all-targets` involved — already failed:

```
userspace/notimpl/src/lib.rs:49:5: error: needless `fn main` in doctest
error: could not compile `notimpl` (lib) due to 1 previous error
```

`notimpl` is mine, it landed in `534e4d63f`, and it is on `lane-b` and not on
`main`, so it blocked nobody but me; it is fixed in `570d9375a`. The point is
not the lint. The point is that a crate can sit in the tree failing
`check_cfg_unix`'s exact command and nobody finds out, because `boot-test.sh`
is expensive enough that it is not run on every change, and the gate that *is*
run on every change — `pre-boot.py` — lints only `kernel`. Whatever you decide
about `--all-targets`, a cheap workspace-wide clippy over this triple in
`pre-boot.py` would have caught it the day it landed.

If you would still rather stage it: put `--all-targets` in `pre-boot.py`
first (where a non-lane-A failure is already advisory and non-blocking, by
your own design), and only then add it to `boot-test.sh`'s hard gate. It is
your gate and your call.

## If nothing changes

Nothing regresses on its own — the affected code is not compiled by anything,
so it cannot rot faster than it is written. It bites the first person who runs
`cargo test` on Linux, who gets a compile error in someone else's crate and
reasonably assumes their own change caused it. And the surface grows with every
`#[cfg(unix)]` test any of the three lanes writes, which is a steady trickle in
mine.

I have left the whole workspace green under the exact command I am asking for
— `clippy --workspace --exclude kernel --all-targets`, 0 errors — so whenever
you turn this on, it turns on green. If you leave it a while, that stops being
true, and the thing that breaks it will be somebody's `cfg(unix)` test that
nothing ever compiled.
