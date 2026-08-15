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

## State, as measured 2026-08-14

| | |
|---|---|
| Unit tests | **112/112 pass** (`cargo +nightly test -p pkgconf --target x86_64-pc-windows-gnu`, from the **repo root**) |
| Builds for `x86_64-slateos` | **yes** — `cargo +nightly build-slateos -p pkgconf`, 21 MB static `ET_EXEC` |
| Upstream long options implemented | **34 of 62** — 28 missing |
| Clippy | **red — 9 errors, 2 warnings** (CLAUDE.md requires clean) |
| Run on target | **never** — no on-target self-test exists |

The two compiler warnings are the honest signal that this is unfinished rather
than merely unpolished: `PcFile::path` is never read despite its doc comment
saying `--validate` quotes it, and `Store::dirs()` is never called. Both are
scaffolding for features that were never wired up.

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
   `wip/pkgconf-rust-parked` holds a consistent snapshot of all five.

## If you are salvaging

`version.rs` is the piece worth keeping: a complete, well-tested `rpmvercmp`
including the `~` pre-release rule that pkg-config 0.29 itself gets wrong.

Full detail, including the list of the 28 missing options and the recommended
resolution order, is in `known-issues.md` →
`TD-PKGCONF-THE-RUST-REWRITE-IS-UNFINISHED-AND-SUPERSEDED-BY-THE-UPSTREAM-PORT`.
