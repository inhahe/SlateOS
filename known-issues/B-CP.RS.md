## B-CP.RS'S-UNIX-ONLY-TESTS-NAME-A-MODULE-THE-FILE-NEVER-IMPORTS — OPEN 2026-09-03 (lane B)

**In short:** `cp`'s test suite has four helper functions that would not
compile. Nobody has noticed because they are marked "Unix only" and this
project's routine `cargo test` runs on the Windows host, where the compiler
skips them without ever reading them. On Linux — which is where the
differential harness runs, and where this code is actually meant to run — they
are a hard error, not a warning. Nothing is *broken* today; a whole slice of
`cp`'s test suite has simply never been compiled by anyone.

**Where:** `userspace/coreutils/src/bin/cp.rs`. The file's import block names
`use coreutils::fsattr::{Link, On};` — the two *types* — and never the module,
nor a free function beside them. Four `#[cfg(unix)]` test helpers then name
what was not imported:

| line | call |
|---|---|
| ~5676 | `fsattr::set_times(On::Path(p, Link::NoFollow), fsattr::Times::both(t))` in `stamp` |
| ~5813 | `fsattr::set_xattr(On::Path(path, Link::NoFollow), name, value)` in `seed_xattr` |
| ~5819 | `fsattr::get_xattr(On::Path(path, Link::NoFollow), name)` in `xattr_of` |
| ~5920 | `chown_privileges()` in `how_loudly_a_failed_attribute_is_reported_is_the_option_that_asked` |

The fourth is the one that makes the point. The first three were found by
reading, when a stage-4 import rewrite made the block worth checking; the
fourth was found only by *running* the Linux check below, and it is a different
name in a different place — which is the evidence that reading is not a
substitute for compiling here.

`use super::*;` at the top of `mod tests` re-exports whatever `cp.rs` imported,
so it brings in `Link` and `On` and nothing named `fsattr`. Two module-level
doc links, `[`On`](fsattr::On)` (~line 397) and `[`fsattr::Xattrs`]` (~line
421), are unresolvable for the same reason and are *not* `cfg`-gated — they are
a `rustdoc` complaint on every platform, which is the one part of this that has
been visible all along and was read as noise.

**Why it was invisible, which is the part worth keeping.** `#[cfg(unix)]` does
not merely disable a function — it deletes it before name resolution, so a
Windows build never learns that `fsattr` is unresolved there. The host runs
Windows; the harness runs Linux but runs the *binaries*, never `cargo test`.
So the only configuration that would have caught this is `cargo test --no-run
--target x86_64-unknown-linux-gnu`, which nothing routinely does. Every
`#[cfg(unix)]` test in this crate is in the same position: they are written,
reviewed and committed, and no machine has ever type-checked them.

**The source fix is done** (2026-09-03, with stage 4 of the copy-engine
extraction) and was one line and one retarget:

1. `#[cfg(unix)] use coreutils::fsattr::{self, Link, On, chown_privileges};`
   inside `mod tests`, next to the other test-only imports. Not at the top of
   the file — on Windows the module would then be an unused import, and a
   `#[allow]` to silence that is a worse trade than a `cfg` that states the
   actual condition.
2. Retarget the two doc links so they resolve without the import: `[`On`]`
   (already imported) and `[`coreutils::fsattr::Xattrs`]`.

**What is still OPEN, and matters more than the two lines — and it is not what
it looks like.** A `cfg(unix)` gate *does* exist, and it was this lane that
asked for it:
`requests/b-a-a-windows-only-check-never-compiles-your-cfg-unix-arms.md`,
landed by lane A on 2026-08-29 in `54b80cc1f`. `scripts/pre-boot.py` runs
`cargo check --workspace --target x86_64-unknown-linux-gnu` and
`scripts/boot-test.sh`'s `check_cfg_unix` runs the clippy equivalent. Both
work. Both ran green over this very tree while all four errors above were in
it.

The reason is one missing word: **neither passes `--all-targets`**, so neither
compiles a single `#[cfg(test)]` module — and `#[cfg(unix)]` is *concentrated*
in test code, because production code here is mostly written against `std`
while the tests are full of `set_permissions(0o4741)`, `symlink`, `nlink`,
`chown` and xattr fixtures that exist on unix and nowhere else. In
`userspace/coreutils` the majority of `#[cfg(unix)]` items in `src/bin/*.rs`
are in `mod tests`. The gate compiles the smaller half of the thing it was
built for and reports OK.

The command that does find them needs no Linux machine, no Linux linker and no
WSL — the host toolchain already has the target installed:

```
cargo check -p coreutils --all-targets --target x86_64-unknown-linux-gnu
```

`check`, not `test --no-run`: name resolution is the whole of what is missing
here, and `check` skips codegen and linking, which is most of the cost. Run
cold on a machine with two other lanes building it, it took ~1,380 s; the
incremental re-run after the one-line fix took **46 s**. For scale, the
workspace-wide gate *without* `--all-targets` took 698 s in the same pre-push
run that certified stage 4. Note also that it writes a
`target/x86_64-unknown-linux-gnu/` subtree that nothing else uses — a real
cost, but disk, not time.

Both scripts are lane A's, so the one-word change is asked for rather than
made: `requests/b-a-the-cfg-unix-gate-skips-every-test-module.md`, filed
2026-09-03, with the four errors as its evidence and both costs — wall clock,
and test modules meeting `#![deny(clippy::all, clippy::pedantic)]` for the
unix target for the first time — left for the gate's owner to weigh.

Until that word is added, every `#[cfg(unix)]` test in `userspace/coreutils` is
unverified source, and the count is not small.

**If it is never fixed:** nothing regresses on its own, because nothing
compiles the affected code. It bites the first person who runs the coreutils
test suite on Linux — they get a compile error in `cp` and, quite reasonably,
assume their own change caused it. The exposure grows slowly: every new
`#[cfg(unix)]` test written on this host is another line nobody has checked —
and the fourth call site above is proof that "we looked carefully" does not
find them.
