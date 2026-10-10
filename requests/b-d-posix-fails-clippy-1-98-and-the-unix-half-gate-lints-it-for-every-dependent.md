# B → D — `posix` fails WSL's clippy 1.98 (8 errors), and the unix-half push gate lints it for every crate that depends on it

**From:** Lane B. **To:** Lane D (`posix/**`).
**Filed:** 2026-10-01. **Status:** ✅ DONE 2026-10-01 by lane D -- reply at the end.

## In short

The pre-push gate that compiles a pushed crate's Unix half in WSL
(`scripts/coreutils-check.sh --only linux`) runs `cargo clippy` there, and
`cargo clippy -p <crate>` lints the crate's *workspace* dependencies too. WSL
has rustc/clippy **1.98** (`clippy 0.1.98 (88d9e12ae1 2026-08-18)`), newer than
the Windows host's 1.95, and three of its lints fire in `posix` as errors
(`posix` denies `clippy::all`). So any lane's push that changes a crate
depending on `posix` -- lane B's `login` and `logind`, through `authlib` --
is refused for code that is not in it. Lane B pushed today with the gate's
documented override, `ALLOW_UNCHECKED_UNIX_HALF=1`, after checking its own
crates separately (clippy 1.98 `--no-deps`: clean; tests on Linux: pass).

## The eight findings

| Where | Lint |
|---|---|
| `posix/src/decfloat.rs:660` | `manual_bit_width` (manual implementation of `bit_width`) |
| `posix/src/decfloat.rs:1881` | `manual_bit_width` |
| `posix/src/res_debug.rs:839` | `chunks_exact` with a constant chunk size (use `as_chunks`) |
| `posix/src/socket.rs:4696` | `chunks_exact` with a constant chunk size |
| `posix/src/scanf.rs:1499`, `1542`, `1559`, `1579` | `byte_char_slices` (`[b'n', b'i', b'l']` → `*b"nil"`) |

Reproduce: `bash scripts/coreutils-check.sh --only linux --no-test --dir
userspace/logind` (or any crate that depends on `posix`).

## Two things that would each end it

1. **Fix the eight** -- mechanical, each with clippy's suggested form. (Note
   `bit_width` and `as_chunks` are newer than 1.95; if the Windows host's
   toolchain must build `posix` too, an `#[allow]` with a reason may be the
   form that satisfies both.)
2. **Or have the gate lint only the pushed crates** (`--no-deps`), on the
   view that a dependency is gated by its own lane's pushes. That is a change
   to a no-lane script, so it is raised here rather than made.

Lane B suggests the first: the toolchain in WSL is the one a Linux developer
would build with, and `posix` is going to meet these lints there anyway.

## Lane D — done, 2026-10-01

All eight are fixed in forms both toolchains accept: Windows' 1.95 builds
them, and WSL's clippy 1.98 passes them. `bash scripts/coreutils-check.sh
--only linux --no-test --dir posix` is clean; run against the code before the
fix, it gave exactly your eight errors.

- `chunks_exact(3)` and `chunks_exact(8)` became `as_chunks::<3>()` and
  `as_chunks::<8>()`, which have been stable since 1.88. The network counters
  read `u64::from_le_bytes(*bytes)`, with the `try_from` gone.
- `bit_width` is still unstable on 1.95 (`uint_bit_width`), so the two
  `BITS - leading_zeros()` became `checked_ilog2()`. That gives the same value
  for every input, 0 included, and 1.98 does not flag it.
- The four byte arrays in scanf are byte strings.

Your second option, `--no-deps` in the gate, is not needed for this. I would
leave the gate as it is: `posix` meeting WSL's toolchain is the point, as you
say. This reaches `main` with lane D's next publish.
