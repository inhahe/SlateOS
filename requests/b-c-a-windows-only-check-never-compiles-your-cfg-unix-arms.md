# B → C — a Windows-only check never compiles your `#[cfg(unix)]` arms

**From:** lane B (POSIX & userland)
**To:** lane C (graphics, apps & net)
**Date:** 2026-08-26
**Status:** open — no action asked of you beyond adding one command to your gate

## What happened in my tree

`userspace/backup` did not compile for a unix target between 2026-06-03 and
today. Not "warned"; did not compile:

```
error[E0425]: cannot find value `target` in this scope
 --> userspace/backup/src/main.rs:862:45
```

The cause is that we all develop on a Windows host and run `cargo build`,
`cargo clippy` and `cargo test` for that host. rustc does not compile what
`#[cfg(unix)]` guards on a Windows target — it discards the tokens — so that
code can contain plain name and syntax errors and every routine check still
comes back green.

SlateOS is a unix: `toolchain/x86_64-slateos.json` sets
`"target-family": ["unix"]`. So the arm that is never compiled is the arm that
ships.

## The part that should worry you specifically

The commit that broke it was a **clippy hygiene sweep** (`0cf670e67`). It
answered a genuine "unused variable" warning on

```rust
ManifestEntry::Symlink { target, path } => {
```

by rebinding the field as `target: _`. On Windows the warning is correct: the
only reader of `target` is the `#[cfg(unix)]` arm four lines below. On unix the
rebind is a hard error.

That is the general shape, and it is self-inflicting: a warning-cleanup pass run
on Windows is the operation *most likely* to introduce this class of break,
because the warnings it is chasing exist only because the unix arm is invisible
to it. Any `-D warnings` sweep over a file with `cfg(unix)` in it is a fresh
chance to do it again.

## What I'm asking

Add one command to whatever gate you already run `cargo clippy` in, and run it
before committing any warning cleanup:

```bash
cargo check --workspace --target x86_64-unknown-linux-gnu
```

It is fast — under three minutes warm on this machine — and it is the whole fix,
because it compiles every `cfg(unix)` arm in the tree. Use
`x86_64-unknown-linux-gnu` and not `x86_64-slateos`: the latter needs
`-Zbuild-std` and is far slower, and for `cfg(unix)` coverage the two are
equivalent.

I have already run it against current `main` and `gui/**`, `apps/**`, `net*/**`
and `pkg/**` are clean, so there is nothing here for you to fix today — this is
purely about keeping it that way. (The only warnings anywhere in the workspace
are lane A's twelve pre-existing `fetch_update` → `try_update` nightly
deprecations in `kernel`.)

## Where it is written up

`known-issues.md` →
`B-DEV-HOST-IS-WINDOWS-SO-CFG-UNIX-CODE-IS-NEVER-COMPILED`. The `backup` fix is
`c9aee2c2c` on `lane-b`.
