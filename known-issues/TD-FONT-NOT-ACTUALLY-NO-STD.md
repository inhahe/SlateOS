### TD-FONT-NOT-ACTUALLY-NO-STD. `osfont` documents itself as `no_std` but links `std` — 2026-08-14 — OPEN

**What.** `gui/font` is written entirely in `alloc` terms (`alloc::vec::Vec`,
`alloc::string::String`, no `std::` paths, `extern crate alloc;` at the top),
and a comment in `cff.rs` asserted outright that "this crate is `no_std`". It
is not: `src/lib.rs` carries no `#![no_std]` attribute, so the crate links the
standard library like any other and the discipline is enforced by nothing but
habit.

**How it was found.** Adding `#![no_std]` to see whether the claim held. It
does not — the build fails with 47 errors, in two groups:

- **Float math (35 errors).** `f32::sqrt`, `floor`, `ceil`, `round` and
  `mul_add` are inherent methods provided by `std`, not by `core`. They are
  used throughout `raster.rs` and `scaled.rs`, which is unavoidable for a
  rasterizer.
- **Prelude items (12 errors).** `String`, `vec!` and `format!` are reached
  through the `std` prelude at a dozen sites instead of being imported from
  `alloc`.

**Why it matters.** The compositor and the toolkit both depend on this crate
and both are meant to run on SlateOS. As long as the attribute is absent, a
`std::`-only construct added here compiles cleanly on the development host and
fails only when someone finally builds for the target — at which point the
offending code is old and its author is a previous session. The false comment
made this worse than a silent omission, because it told the next reader the
invariant was already being checked.

**Proper fix.** Add `libm` to the workspace, replace the inherent float
methods with `libm::{sqrtf, floorf, ceilf, roundf, fmaf}` (or the
`num-traits`/`libm` float shim), import the prelude items from `alloc` at the
dozen sites, then add `#![no_std]` and `#[cfg(test)] extern crate std;`. The
mechanical part is small; what makes it more than mechanical is that `libm`
would be this workspace's first float-math dependency, and whether SlateOS
userspace GUI binaries get a `std` port at all is Lane B's call (`posix/**`) —
if they do, `no_std` here buys much less than it seems to. That question
should be settled before spending the churn.

**Interim.** The false comment in `cff.rs` was corrected and the crate docs in
`lib.rs` now state the real position, so nobody is misled into thinking the
invariant is enforced. Keep writing `alloc::` paths: the point of doing so is
that closing this stays a small change.

**Update, 2026-09-02.** The crate gained one deliberate `std::` user:
`src/phase.rs`, the shaping-phase timer, which needs `std::time::Instant`.
It is behind the `phase-timing` cargo feature, off by default and never on in
a shipped build, and the `std` import sits inside the `#[cfg(feature = …)]`
half of a `sealed` module — the feature-off half is a unit struct with no
imports at all. So it does not enlarge this entry: whoever adds `#![no_std]`
puts it behind `#![cfg_attr(not(feature = "phase-timing"), no_std)]`, or
declines to build the feature for the target, and nothing else changes. It is
recorded here only so the next `grep` for `std::` in this crate finds an
explanation rather than a violation.
