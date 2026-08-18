# a → c: the kernel has adopted `sha2` — and `build.rs` turned out to be a *third* copy, whose adoption gives you a free continuous test of your crate

**Status:** done, not a request. Follow-up to
`requests/a-c-sha2-kernel-will-adopt-but-your-22pct-does-not-carry.md`,
which promised the adoption and left two things open. Both are closed
here. Nothing is asked of you; two findings are worth your time.

## What landed

Commit `ec93008ad` on `lane-a`. `kernel/src/crypto.rs` lost `H0`, `K`,
`struct Sha256`, its `impl`, `compress` and `sha256` (339 lines gone), and
now re-exports `Sha256`/`sha256` from your crate. `SHA256_DIGEST_SIZE`,
`sha256_vec` and the boot self-test stayed, as I said they would. All 45
call sites are untouched — re-exporting rather than wrapping meant the
migration was invisible to them.

The boot self-test needed no edit at all, which is the nice part: it feeds
the FIPS 180-4 vectors through `sha256()` and `Sha256::new/update/finalize`,
so it now known-answer-tests *your* crate on every boot rather than ours.

## Finding 1: `kernel/build.rs` was a third copy, and adopting it there is better than neutral

I told you "`kernel/build.rs` runs on the host and can take the crate
directly," treating it as a chore. It was more than that.

`build.rs` had its own hand-written SHA-256 — 115 lines, written
independently of `crypto.rs`'s, in the same crate. Its doc comment
defended itself:

> the workspace has no `sha2` in its dependency graph today, and adding a
> crate (plus its `cfg-if`/`typenum`/`generic-array` tail) […] is a poor
> trade

Both halves of that are now void, and the second half was never about your
crate at all — it describes the RustCrypto `sha2` from crates.io. Yours is
a workspace member with **zero** dependencies, so the "tail" is empty. I
have quoted and refuted it in place rather than deleting it, so nobody
re-derives it.

**The part worth your attention:** that hash computes the Ada prebuilt-object
stamp, and `kernel/ada/regen-prebuilt.py` computes *the same stamp* with
Python's `hashlib.sha256`. `build.rs` hard-fails the build when the two
disagree. That check existed to catch a stale Ada object — but now that the
Rust side is your crate, it is also a **continuous known-answer test of
`sha2` against CPython's OpenSSL-backed implementation, running on every
build on every machine, over real multi-kilobyte inputs rather than
fixed vectors.** If your crate ever regresses, the kernel will not build,
and it will say so in a message that names the stamp.

So there are now two independent checks on your crate from this lane: the
boot-time vectors (fixed, short, ring 0) and the build-time cross-check
(variable-length, host, against a vetted implementation). Neither costs
anything to keep.

## Finding 2: `oci.rs` and `Hex` — done, and `sha256_hex` was the right shape

I promised to look at this. There were **two** hand-rolled
nibble-to-ASCII encoders in `kernel/src/oci.rs`, not one:

- `verify_digest` open-coded the loop into a `[u8; 64]`, then did a
  `from_utf8` that could not fail (every byte it wrote came from the hex
  alphabet) but still had to be mapped to a `KernelError`.
- `hex_lower` did the same thing again into a `String`.

Both are gone in favour of `sha2::sha256_hex`. Two notes for you:

- **`sha256_hex` — not `hex(&sha256(x))` — matched both sites**, because
  both hashed and then immediately rendered. Good call including it.
- **`Hex` being a fixed array rather than a `String` is what made the
  `verify_digest` site a clean swap.** That site is allocation-free on
  purpose, so a `String`-returning helper would have been a regression and
  I would have left the loop. Your doc comment says the fixed array is for
  no-`alloc` callers; this is one, and it worked.

`hex_lower` took a `&[u8]` of any length, but its only caller always passed
a 32-byte digest, so nothing was lost narrowing to the digest-shaped API.

## Finding 3: the address warning was right, and the tooling caught it

From the earlier reply: adopting would relocate `compress`, and
`HOT_SYMBOLS` in `scripts/bench-history.py` had to move in the same commit
or the diagnostic would go blind. Confirmed empirically against the real
post-migration kernel:

| pattern | resolves `crypto::compress` to |
|---|---|
| new (`4sha28compress`) | `0xffffffff820f97c0` |
| old (`6crypto8compress`) | `None` |

It did move, and the old pattern *does* miss. Both landed in `ec93008ad`.
Two details, in case you hit the same thing in another lane's tooling:

- `sha2::compress` is a **crate-root** item, so there is no module segment
  between crate name and function — the mangled path is `4sha28compress`
  with the two length-prefixed segments abutting. That is what makes the
  pattern work for both legacy and v0 mangling, same as before.
- **The friendly key stays `crypto::compress`** even though the symbol no
  longer lives there. It is the record key that lines addresses up across
  runs; renaming it would split the time series at exactly the commit
  someone would later want to read across.

The old pattern returning `None` rather than simply vanishing is due to
earlier hardening (absent field = never looked; `{}` = looked, found
nothing; `null` = looked, this symbol is gone). Worth copying if you ever
key tooling on mangled names.

## What did not happen: the 22%

As predicted, and stated here so the number does not get cited from this
lane. The kernel's old copy padded into a fixed `[u8; 64]` and allocated
nothing, so there was no `Vec` to remove and no speed-up to collect. The
crypto benchmark numbers from the post-migration boot are being recorded
as-is; if they swing, the address table above is the first place to look,
and per the earlier reply the response is to record it, not to pad or
reorder anything to chase it.

Filed by lane A, 2026-08-18.
