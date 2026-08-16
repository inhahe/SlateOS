# ⚠️ This crate is an UNFINISHED, SUPERSEDED rewrite — read before working on it

**Do not "finish" this without asking the operator first.** It is not a
half-done task waiting to be completed. It is work that was superseded *while in
progress*, and completing it would spend hours re-solving a problem that is
already solved.

## Why

This crate is a from-scratch Rust reimplementation of pkgconf, started without
first checking whether upstream pkgconf builds for SlateOS.

It does. Upstream **pkgconf 2.3.0 cross-compiles and links against
`toolchain/sysroot/lib/libc.a` with zero source changes, zero shims and zero
missing symbols, on the first attempt** — all 53 libc symbols it needs were
already present. See `scripts/pkgconf-spike/` (run `run.sh` to reproduce).

Per `design-decisions.md` §307 and `roadmap-detailed.md`'s "Porting vs.
Reimplementing" policy, the port wins.

## State, as measured 2026-08-16

| | |
|---|---|
| Unit tests | **119/119 pass** (`cargo test -p pkgconf --target x86_64-pc-windows-gnu`, from the **repo root**) |
| Builds for `x86_64-slateos` | **yes** — `cargo +nightly build-slateos -p pkgconf` |
| Upstream long options implemented | **34 of 62** — 28 missing |
| Clippy | **clean** (`--all-targets`), rustfmt clean |
| Run on target | **never** — no on-target self-test exists |

**Clippy being clean does not mean this is finished.** It was red (9 errors,
2 warnings) until 2026-08-16, and the two dead-code warnings were being used as
a status marker for "unfinished". That is a bad marker — a reader cannot tell a
warning that means *unfinished* from one that means *defect*, and a parked crate
is still read. So they were resolved on the evidence, in opposite directions:
`Store::dirs()` was scaffolding for nothing and was deleted; `PcFile::path` was
scaffolding for a real gap (`--variable=pcfiledir` returned `""` while
`${pcfiledir}` worked) and was wired up, along with the virtual `pkg-config`
package that makes the field legitimately optional.

**The marker that actually matters is the last row of the table:** it has never
executed under the SlateOS kernel, and 28 upstream options are still missing.
See `known-issues.md` for the full account of what changed and why none of it
moves the §307 decision.

## Two traps

1. **Build it from the repo root, not from `userspace/`.** The zone config turns
   on `build-std`, which collides with a host-target `cargo test` (duplicate
   lang items, `panic_unwind` strategy mismatch). Use the root `build-slateos` /
   `check-slateos` aliases, or `cargo test -p pkgconf --target
   x86_64-pc-windows-gnu` from the root.
2. **The working tree is inconsistent on purpose.** `main.rs` is tracked and
   *modified*; `flags.rs`, `pcfile.rs`, `store.rs` and `version.rs` are
   *untracked*. The committed `main.rs` is the older ~200-line standalone
   version with no `mod` declarations, so **committing `main.rs` alone would
   break the crate**. Commit all five or none. Branch
   `wip/pkgconf-rust-parked` holds a consistent snapshot of all five, advanced
   to the 2026-08-16 state in `a3c5cb306` — **that branch, not this working
   tree, is authoritative** if the two ever drift.

## If you are salvaging

`version.rs` is the piece worth keeping: a complete, well-tested `rpmvercmp`
including the `~` pre-release rule that pkg-config 0.29 itself gets wrong.

Full detail, including the list of the 28 missing options and the recommended
resolution order, is in `known-issues.md` →
`TD-PKGCONF-THE-RUST-REWRITE-IS-UNFINISHED-AND-SUPERSEDED-BY-THE-UPSTREAM-PORT`.
