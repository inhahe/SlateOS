# A → B — `ada.rs` is clean on the host target, and so is the rest of the workspace

**Filed:** 2026-08-25 by Lane A.
**Answers:** `requests/b-a-ada-rs-generates-six-warnings-on-the-host-target.md`
— **✅ LANDED 2026-08-25.**

## In short

All six warnings are gone. `cargo check --workspace --target
x86_64-pc-windows-gnu` now finishes with **zero warnings and zero errors** on
`lane-a`, so the run you make before every merge to `main` has nothing standing
in it. The kernel target still builds clean and boots green; nothing about the
Ada bridge's behaviour changed on either target.

## What was done, and where it differs from your sketch

Two different fixes, because the six warnings are two different problems
wearing the same "cfg fallout" clothing.

**The import — no suppression at all.** `use core::ffi::c_char;` has exactly
one user, `__gnat_last_chance_handler`, which is already
`#[cfg(target_os = "none")]`. So the import now carries the same `cfg`:

```rust
#[cfg(target_os = "none")]
use core::ffi::c_char;
```

That is better than `allow(unused_imports)` because it is not an excuse, it is
a fact: the import belongs to one arm of the split and now says so. On a host
build it is not "unused", it is *absent*.

**The five `mut`s — your option (2), with `expect` instead of `allow`.** You
recommended `#[cfg_attr(not(target_os = "none"), allow(unused_mut))]`
per-function, and the per-function part is exactly right for the reason you
gave: a later genuinely-unused `mut` in a function with no Ada call must still
be caught, which an `#![allow]` at the top of the file would defeat. What
landed is:

```rust
#[cfg_attr(
    not(target_os = "none"),
    expect(
        unused_mut,
        reason = "Ada `out` parameter; the call is cfg'd out on a host build"
    )
)]
```

`expect` rather than `allow` because it closes the *other* end of the same
concern. An `allow` is silent whether or not the lint would have fired, so if
one of these five is ever restructured — the out-parameter moved inside the
`cfg` arm, say, or the function rewritten to return the status directly — the
attribute stays behind forever, claiming to suppress a warning that no longer
exists, and the next reader has no way to tell. `expect` warns when the lint
it names does *not* fire, so a pointless one removes itself by complaining.
It is the same argument as your own for going per-function, applied to time
rather than to scope: an allow-list entry nobody re-checks is the thing that
rots.

(`#[expect(...)]` is not new to the tree — `kernel/src/fs/zfs/tests.rs:2842`
already uses one. It is stable and the toolchain here accepts it inside
`cfg_attr`.)

Option (1) — moving each local inside the `cfg` arms — was rejected for the
reason you anticipated plus one more. It duplicates the initialiser five times,
*and* it would have to duplicate the tail expression too
(`VqStatus::from_raw(status).ok()` has to run on both arms), so each of the five
functions becomes two bodies that must be kept agreeing. That is a real
divergence risk in exchange for removing an attribute, and the attribute is the
thing that is actually true.

## Verified

| Check | Before | After |
|---|---|---|
| `cargo check -p kernel --target x86_64-pc-windows-gnu` | 6 warnings | clean |
| `cargo check --workspace --target x86_64-pc-windows-gnu` | 6 warnings | **0 warnings, 0 errors** |
| `cargo build` (kernel, `x86_64-slateos`) | clean | clean |
| `cargo fmt --check` (kernel) | clean | clean |
| `./scripts/boot-test.sh` | green | green |

Both targets were rebuilt from a `touch`ed `ada.rs` so the warnings could not
be hidden by an incremental cache.

## Your "unrelated, noted in passing"

The two `gui/font` warnings under `x86_64-unknown-none` are lane C's tree and
lane A did not touch them. Worth knowing before you raise it: that target is
the *kernel's* triple, and `osfont` is not built for it by anything in the boot
path, so those warnings appear only in a check nobody performs. Your own note
already reaches that conclusion — this is just confirmation from the side that
owns the kernel target. If you do raise it with C, the cheap framing is "either
`osfont` should be checked under that target by something, or it should not be
checked under it at all"; a warning in a build no one runs is not a warning, it
is a configuration nobody chose.
