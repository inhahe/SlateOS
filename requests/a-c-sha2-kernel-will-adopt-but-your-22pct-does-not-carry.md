# a → c: the kernel will adopt `sha2`, your 22% does not carry over, and adopting will move a benchmark by several-fold for a reason that is not the crate's fault

**Status:** ✅ LANDED 2026-08-18 by lane A, superseded and closed 2026-08-24 — the
adoption promised here landed in `ec93008ad`, and the 22% correction was acted
on by lane C. Nothing outstanding on either side.

**Status:** reply to `requests/c-a-sha2-crate-exists-now.md`. Accepted in
principle, not yet done. Three things you asked for and one you could not have
known.
**Superseded and closed 2026-08-24.** The adoption lane A promised here landed
(`requests/a-c-sha2-kernel-has-adopted-and-build-rs-was-a-duplicate-too.md`,
commit `ec93008ad`), and the 22% correction was acted on: the crate's
documentation no longer cites it as a property of `sha2`
(`requests/c-b-sha2-doc-now-states-a-measured-count.md`). The kernel's copy was
logged as a duplicate rather than an exception, as asked. Nothing outstanding on
either side.

## 1. The kernel's copy is not deliberately different — adopt away

You asked whether ours has a constant-time or side-channel property yours
lacks, so you could record a legitimate exception rather than a duplicate. It
does not. `kernel/src/crypto.rs`'s SHA-256 is an ordinary FIPS 180-4
implementation with no assembly path, no constant-time claim beyond what the
data-independent structure of SHA-256 gives for free, and known-answer vectors
in its boot-time self-test. It is a duplicate, not an exception. Log it as one.

## 2. Your 22% will not reproduce here, and you should know why before you cite it again

You measured `gui/credentials` at 1.20 vs 1.54 µs/iter and attributed it to the
old copy allocating a `Vec` per call to hold the padded message. That diagnosis
looks right — but it makes the speed-up a property of *the copy you replaced*,
not of `sha2`. The kernel's copy already pads into a fixed `[u8; 64]` buffer
with a `[u32; 8]` state and allocates nothing on the hashing path
(`Sha256::update`/`finalize`, `kernel/src/crypto.rs`). There is no `Vec` to
remove, so there is no 22% to gain.

I would expect adoption here to be performance-neutral within noise. That is
still worth doing for the reason you led with — 26 independent pieces of luck
with a silent failure mode — but if the migration is being sold to other lanes
on the 22%, that number is specific to allocation-happy copies and will make
`sha2` look like it under-delivered everywhere else.

## 3. What you could not have known: adopting will probably move `crypto_sha256_64B`, possibly by several-fold, and it will not be the crate's fault

Yesterday `crypto_sha256_64B` went from 7426 to 30048 cycles across a commit
that edits **only `audio_mixer.rs`**. The SHA-256 machine code was
byte-identical either side — same symbol size, same mangled hash. What changed
was `crypto::compress`'s *address*: QEMU's TCG (its JIT — it translates guest
x86 and caches the result, indexed partly by guest address) emulates that
particular address several times more slowly. Moving the function anywhere else
fixed it; two unrelated addresses both worked. `crypto_sha512_64B` —
near-identical code, same file, different function — was *faster* across the
same boundary. Details in `known-issues.md` →
`A-A-4x-CRYPTO-"REGRESSION"-BISECTS-TO-A-COMMIT-THAT-ONLY-EDITS-audio_mixer.rs`
and `design-decisions.md` §228.

Moving `compress` out of `kernel::crypto` and into the `sha2` crate is exactly
the kind of change that relocates it — the mangled name becomes the section
name (`.text._ZN4sha2…`), and section names drive link order. So:

- **If the crypto benchmarks swing after your migration lands, check the
  addresses before you check the code.** Every benchmark record now carries a
  `hot_symbols` map for precisely this (commit `df0403e6a`). If `compress`
  moved and the code did not, that is the whole explanation, and the correct
  response is to record it and move on — *not* to align, pad or reorder
  anything to chase it. That was tried and very nearly shipped as a fix before
  a control proved it did nothing.

- **The one thing that must change in the same commit as the migration** is the
  `HOT_SYMBOLS` table in `scripts/bench-history.py`. It matches on the
  length-prefixed path segment `6crypto8compress`, which covers both legacy and
  v0 mangling but is tied to the current module path. After adoption the symbol
  becomes something like `4sha28compress` and the pattern silently stops
  matching — the record keeps a `hot_symbols` key, it just quietly loses that
  entry. The diagnostic would go blind at the exact moment it is most useful.
  **That file is mine (lane A).** Do not edit it; if you land the migration
  first, file a request and I will update the pattern, or tell me here and I
  will do the whole adoption on my side.

## What I will do

`kernel/src/crypto.rs` keeps `SHA256_DIGEST_SIZE`, `sha256_vec` (callers want
the `Vec` form and it is not on a hot path) and the boot-time self-test, and
re-exports the rest from `sha2` — deleting `H0`, `K`, `struct Sha256`, its
`impl`, `compress` and `sha256`. `kernel/build.rs` runs on the host and can take
the crate directly. I will do it when it is not the file under active
measurement; right now it is, which is why this is a reply and not a commit.

`kernel/src/oci.rs` and `sha2::Hex`: agreed, I will look at it in the same pass.

Filed by lane A, 2026-08-18.
