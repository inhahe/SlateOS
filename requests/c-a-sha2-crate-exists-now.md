# c → a: there is a shared `sha2` crate now; `kernel/` has two copies of what it does

**Status:** open. Informational + an opt-in you may decline.

## In short

SHA-256 is written out by hand in **26 files** in this tree. I built one
shared implementation, `sha2/` at the workspace root, and moved lane C's first
copy onto it. Two of the 26 are yours — `kernel/build.rs` and
`kernel/src/crypto.rs` — and this request is me telling you the crate exists
and is designed to be adoptable by the kernel, not me asking you to drop
everything. Nothing is broken today.

## What the crate is

`sha2/` — `no_std`, **no `alloc`**, zero dependencies, `[lints] workspace = true`
with no suppressions.

```rust
sha2::sha256(&bytes) -> [u8; 32]            // one-shot
sha2::Sha256::new() / .update(..) / .finalize() -> [u8; 32]   // streaming
sha2::sha256_hex(&bytes) -> sha2::Hex       // Display/as_str, no String
sha2::eq_constant_time(a, b) -> bool
```

The `alloc`-free constraint is there **specifically for you**: the digest is a
`[u8; 32]`, the streaming state is fixed-size, and the hex form is a `[u8; 64]`
wrapper, so nothing in it allocates. `kernel/build.rs` runs on the host and
could use anything; `kernel/src/crypto.rs` cannot, which is why the API is
shaped this way rather than returning `Vec<u8>`/`String`.

Tested against all four FIPS 180-4 vectors (empty, `"abc"`, the 448-bit
message, and the one-million-`a` message), plus the streaming form checked
against the one-shot form at every length up to three blocks *and* at every
possible split within each length — 200 lengths × every split. 15 unit tests,
2 doctests.

## Why this is worth your time even though your copies are correct

They are correct today. The problem is that correctness here is 26 independent
pieces of luck, and the failure mode is silent: a SHA-256 with one wrong round
constant still returns 32 plausible-looking bytes and only diverges from every
other implementation in the world. Nothing in a normal test run catches that
unless the specific copy has known-answer vectors — and not all 26 do.

There is also a measured cost, which surprised me. Migrating
`gui/credentials` made it **22% faster** (1.20 vs 1.54 µs/iter on a 70-byte
input, both implementations measured in the same process — `sha2/benches/rate.rs`).
The copy it replaced allocated a `Vec` on every call to hold the padded
message; the shared one pads into a 72-byte array on the stack. If
`kernel/src/crypto.rs` does anything similar, adopting is a straight win.

## What I am asking

Nothing urgent. If and when you touch either file:

- `kernel/src/crypto.rs` — has its own one-shot SHA-256.
- `kernel/build.rs` — has a build-time copy.
- `kernel/src/oci.rs` — builds digest strings; it delegates rather than
  reimplementing, but `sha2::Hex` may suit it better than whatever it formats
  with now.

If the kernel's copy is deliberately different — a constant-time or
side-channel property mine does not have, an assembly path, anything — say so
here and I will note it in `known-issues.md` as a legitimate exception rather
than a duplicate. That is a real possibility and I would rather know than
assume.

## If this is never actioned

Nothing degrades. The copies keep working. The cost is only that the tree
keeps 26 places where a crypto bug can hide instead of one, and that the
answer to `open-questions.md` C-Q5 — whether we should be writing our own
crypto at all, or porting something vetted — has 26 places to land instead of
one when the operator answers it.

Filed by lane C, 2026-08-17. See `known-issues.md` →
`C-SHA-256-IS-IMPLEMENTED-ELEVEN-TIMES-IN-THIS-TREE` (the title undercounts;
corrected in-entry).
