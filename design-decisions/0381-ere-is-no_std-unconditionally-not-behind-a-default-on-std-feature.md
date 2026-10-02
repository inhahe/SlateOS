## §381 — `ere` is `no_std` unconditionally, not behind a default-on `std` feature

**Date:** 2026-08-24
**Decided by:** Claude (autonomous) — in response to lane A's request, and
choosing a different mechanism from the one the request proposed

**In short:** The kernel has its own little shell that runs before the real
system starts, and its `awk` and `sed` were not really matching patterns — asked
for lines starting with `err`, they returned lines that merely *contained* the
four characters `^err`, and reported success. The real pattern engine already
exists in userspace but could not be used from the kernel, because it was built
against the ordinary Rust standard library and the kernel has no standard
library. Lane A asked for a switch that turns the standard library off. Instead
the standard library was removed altogether, because nothing in the engine was
using it, and a switch with two settings is a thing that can be flipped the
wrong way.

### What was decided

`userspace/ere` is now `#![no_std]` plus `extern crate alloc`, with no feature
flags at all. `std` is linked for the test build only (`#[cfg(test)] extern
crate std;`), because two tests need a thread and a clock to bound the
backtracker's running time with.

The whole of the change to the engine was:

| was | is |
|---|---|
| `std::fmt::{Display, Debug, Formatter, Result}` ×4 | `core::fmt::…` |
| `impl std::error::Error for MatchLimit` | `impl core::error::Error` — the same trait since Rust 1.81 |
| implicit `Vec`/`String`/`Box`/`vec!`/`format!` from the `std` prelude | explicit `use alloc::…` |
| `bstr = { …, features = ["std"] }` | `bstr = { …, default-features = false }` |

Nothing else moved. No behaviour changed, and the crate's 63 tests pass
unmodified.

### Why not the `std` feature lane A asked for

A default-on `std` feature is the conventional answer, and it is the wrong one
here for three reasons that are specific to this crate.

- **Nothing was using `std`.** A feature exists to name a real difference
  between two builds. Here there is none: after the four `core::fmt` impls and
  the one `core::error::Error`, the `std` build and the `no_std` build would
  have compiled the identical code. The flag would have had one setting nobody
  ever selects — dead configuration, and dead configuration is the kind that
  rots because nothing exercises it.
- **Cargo unions features across a build graph, so the flag would not even have
  worked reliably.** `default-features = false` in `kernel/Cargo.toml` does not
  subtract anything; it only declines to add. Any other crate in the kernel's
  graph asking for `ere/std` would switch it back on for the kernel too, and
  the failure would be a kernel that will not link, arriving at whoever next
  edits an unrelated `Cargo.toml`. `oils` already asks the shared `bstr` for
  `std`, which is exactly that shape one layer down — and is why `oils` was
  moved to `bstr/alloc` in the same change.
- **Two configurations that can disagree is the failure this crate exists to
  prevent.** The crate's own module doc says so about *engines*: four programs
  each had their own idea of what `[a-z]` matched. A crate that can be compiled
  two ways is a smaller version of the same thing — `cargo test` exercises one
  of them and the kernel ships the other. Under unconditional `no_std`, `cargo
  test` compiles the library exactly as the kernel will, so a `std::` path that
  crept into non-test code fails the *test run*, not just the kernel build six
  weeks later.

### What is given up

- **`std`-only conveniences are now closed to the engine.** If `ere` ever wants
  `std::io` (streaming a subject rather than taking a `&[u8]`), or a
  `HashMap`-backed cache, it cannot simply reach for them. `alloc` has
  `BTreeMap`, and the engine's public shape is `&[u8]`-in, spans-out, so this
  looks cheap — but it is a real door closed, and reopening it means adding the
  feature after all.
- **The test build and the shipped build differ by `extern crate std`.** That is
  the narrowest form of the very split objected to above. It is accepted
  because it runs in the safe direction: the *library* is `no_std` in both, and
  only the `#[cfg(test)]` module gains anything.

### Alternatives rejected

- **A default-on `std` feature** — lane A's proposal. Rejected above.
- **`#![cfg_attr(not(test), no_std)]`** — makes the test build a `std` build, so
  the test modules need no import changes at all. Rejected because it hides the
  thing worth catching: under it, a `std::` path added to *production* code
  still compiles under `cargo test`, and only the kernel build would notice.
  The three added `use alloc::…` lines are the price of that check, and they are
  three lines.
- **A second engine in `kernel/`** — what happens if nothing is done. Rejected
  by lane A in the request itself, and by this crate's reason for existing.
