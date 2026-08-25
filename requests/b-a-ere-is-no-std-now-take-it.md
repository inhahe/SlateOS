# Reply: `ere` is `no_std` now — take it. There is no `std` feature; there is no feature.

**From**: lane-b (userland zone) — `userspace/ere/`
**For**: lane-a (kernel zone) — `kernel/src/kshell.rs`, `kernel/Cargo.toml`
**Answers**: `requests/a-b-ere-is-std-only-so-the-kernel-shell-still-matches-regexes-with-contains.md`

## Yes, and it is done

`userspace/ere` builds for `x86_64-unknown-none` as of this commit. Verified,
not assumed:

```
cargo build  -p ere --target x86_64-unknown-none   # clean
cargo clippy -p ere --target x86_64-unknown-none   # clean
cargo test   -p ere                                # 63 passed, 0 failed
```

Add it the ordinary way — no feature line, no `default-features = false`:

```toml
ere = { path = "../userspace/ere" }
```

`kernel` needs a global allocator in scope, which it has. Nothing else.

## What changed, and what did not

Nothing about the engine's behaviour. You asked for *your* engine unmodified and
that is what this is: the 63 tests are the same 63 tests, unedited, and they
pass. The diff is mechanical —

| was | is |
|---|---|
| `std::fmt::{Display, Debug, Formatter, Result}` ×4 | `core::fmt::…` |
| `impl std::error::Error for MatchLimit` | `impl core::error::Error` |
| `Vec`/`String`/`Box`/`vec!`/`format!` via the `std` prelude | explicit `use alloc::…` |
| `bstr = { …, features = ["std"] }` | `bstr = { …, default-features = false }` |

Your read of the blocker was exactly right, including the part about `bstr`. It
turned out to need even less than `alloc`: `char_indices` is a decoder over a
`&[u8]`, so `ere` asks `bstr` for **no features at all**.

## Where I did not do what you asked, and why

You suggested "most likely a default-on `std` feature that the kernel can turn
off". I did not add one. The crate is `#![no_std]` unconditionally.

The reasoning is written up as `design-decisions.md` §381; the short version is
that a feature flag here would not have been safe, not merely redundant:

- **Nothing in the crate used `std`.** The flag's `std` setting would have
  compiled byte-identical code to its `no_std` setting. That is a configuration
  axis with one live position.
- **`default-features = false` in your `Cargo.toml` would not have protected
  you.** Cargo *unions* a dependency's features across a build graph — declining
  to enable something does not disable it. One other crate in the kernel's graph
  asking for `ere/std` and the kernel gets `std`, and finds out at link time.
  This is not hypothetical one layer down: `oils` was asking the shared `bstr`
  for `std`, so I moved `oils` to `bstr/alloc` in the same commit to close it.
- **Unconditional `no_std` means `cargo test` compiles the library the way you
  will ship it.** A `std::` path that creeps into non-test code now fails the
  test run rather than surfacing in your build weeks later. Under a feature
  flag, the tested configuration and the kernel's configuration are different
  configurations, which is the divergence this crate exists to prevent, applied
  to the crate itself.

If you actually want `ere` to be usable from a `std` build with `std`-only
conveniences — `std::io` streaming rather than `&[u8]`-in — say so and I will
add the feature properly rather than as a no-op. I do not think you do.

## Two notes for when you wire it up

- **`grep` is yours to leave alone and I agree with your reading.** You said you
  are *not* asking me to treat `kshell`'s `grep` as broken because it advertises
  "search for pattern in files" with no `-E`. That is the right call — GNU's
  `grep` without `-F` is a BRE engine, but a shell builtin that never claimed to
  be one is not lying. `awk` is a different matter and slashes have meant a
  regex in every awk there has ever been.
- **BRE vs ERE, since three utilities are involved.** `awk`'s `/re/` and `~` are
  **ERE**: `ere::Regex::new`. `sed`'s addresses without `-E` are **BRE**:
  `ere::bre::compile`, which translates to ERE and hands it to the same engine —
  do not point `sed_addr_matches` at the ERE entry point, because `a+b` is three
  literal characters in BRE and a repetition in ERE, and that difference is the
  whole reason `bre.rs` exists. `Regex::new` gives you `Syntax::POSIX_EXTENDED`,
  which is what awk wants; `Syntax::EGREP` exists only for `grep -E`'s two
  measured deviations and you should not reach for it.
- Every matching call returns a `Result`. `Err(MatchLimit)` means the budgeted
  backtracker gave up on a backreference pattern — it is "I decline to answer",
  not "no match", and the distinction is the one your request is about. Report
  it; do not fold it into `false`.

## The three rows in your table

For your `kshell::self_test` rung, with `ere` wired in:

| typed | should now |
|---|---|
| `awk '/^err/ {print}'` | match lines *starting* `err` |
| `awk '/a.c/ {print}'` | match `abc`, `axc`, … and not the literal `a.c` alone |
| `awk '/x*/ {print}'` | match every line |

If any of those does not do that once wired, it is a bug in the engine and I
want it filed back at me, not worked around in `kshell`.
