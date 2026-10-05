## C-THE-DISK-IMAGER-VERIFIES-NOTHING-AND-SAYS-SHA-256 (lane C, 2026-08-17)

**In short.** `apps/diskimager` offers to checksum an image with MD5, SHA-1 or
SHA-256 and shows you a digest of exactly the right length. None of the three
is the algorithm it claims. All three are the same made-up mixing function, so
the "SHA-256" it prints for a downloaded `.iso` will never match the SHA-256
the publisher printed — and its "verify after write" tick box is checking the
disk against a number that means nothing outside this program.

**Where.** `apps/diskimager/src/main.rs`, `HashState` (~lines 205-290).

**What it actually computes.** `new()` seeds `state` with the genuine
published initial values for whichever algorithm you picked — the real
SHA-256 IV (`0x6a09e667, 0xbb67ae85, …`), the real MD5 and SHA-1 ones. That
is the entire resemblance. `update()` then ignores all of it:

```rust
for (idx, &byte) in data.iter().enumerate() {
    let slot = idx % 8;
    if let Some(s) = self.state.get_mut(slot) {
        *s = s.wrapping_mul(31).wrapping_add(byte as u64);
    }
}
```

That is eight interleaved `u64` polynomial accumulators — a Rabin-style
rolling fingerprint with base 31 — and it is identical for all three
algorithms. `finalize()` then stirs the eight words together and truncates the
hex to 32, 40 or 64 characters depending on which name you asked for. The
choice of algorithm affects **only the length of the output string** and the
eight seed constants.

**How it survived.** Two ways, both worth noting because they generalise.

1. *The tests check shape, not value.* There are eight tests over `HashState`.
   They assert the digest is 64 characters, that it is all hex digits, that
   the same input twice gives the same output, and that two different inputs
   give different outputs. Every one of those passes for `*s = s*31 + byte`.
   Not one test compares against a known answer — and a known-answer vector is
   the *only* test that can distinguish a hash from a plausible-looking
   function, which is precisely why FIPS publishes them.

2. *It has already been "fixed" once, at the wrong level.* There is a
   nine-line comment in `finalize()` explaining that the previous version
   emitted the raw state words and truncated, so that bytes landing in
   discarded words produced identical digests — "a real collision" — and that
   folding all eight words in fixes it. That diagnosis is correct and the fix
   works. But it treats the stub as the thing to repair rather than the thing
   to replace, which is the band-aid accumulation `CLAUDE.md` warns about: the
   collision was a symptom, and the disease is that this is not a hash.

**Impact.** Two distinct failures, one much worse than the other.

- **Comparing against a published checksum is broken outright, and silently.**
  This is the headline use of a disk imager: download an install image, paste
  in the checksum from the download page, confirm it matches. It never will.
  The user sees `Mismatch`, concludes their download is corrupt, and
  re-downloads forever. Worse in the other direction: the Verify tab will
  happily *display* a 64-character "SHA-256" that a user may copy and publish
  as if it were one.
- **Verify-after-write is weaker than it looks but not worthless.** It
  compares the source against the written-back data using the same function on
  both sides, so it is a self-consistency check, and a base-31 polynomial over
  `u64` does catch random corruption with high probability. It will not catch
  deliberate tampering, and it is not what the UI implies.

**Severity.** High. It is a correctness bug in the feature the application
exists for, it is invisible to the user (the output is well-formed and
stable), and the tests are green.

**Proper fix.** Not "write the missing rounds into `update()`" — delegate.
`sha2/` already exists at the workspace root (`d8ad84f54`): `no_std`, no
`alloc`, checked against all four FIPS 180-4 vectors. SHA-1 and MD5 need the
same treatment — they are obsolete *for security* but a disk imager needs them
precisely because publishers still post them, so they should be shared crates
in the same shape, each with the known-answer vectors from FIPS 180-4 and RFC
1321 respectively. Then `HashState` becomes a thin enum over three real
implementations.

**And add value-checking tests**, in `diskimager` as well as in the crates, so
the next stub cannot pass. The minimum bar for any hash in this tree: the
digest of the empty input, and the digest of `"abc"`. Both are published for
all three algorithms.

### FIXED, 2026-08-17 (`cf5ebb13f`, and the commit that follows it)

Done as written above — delegated, not patched.

`blockbuf`, `sha1` and `md5` now sit at the workspace root beside `sha2`.
Three crates rather than two because SHA-1 and MD5 written standalone would
have meant a third and fourth copy of Merkle–Damgård partial-block and padding
logic, which is the half that actually hides bugs: a wrong compression
function fails the first known-answer vector, whereas a wrong buffer only
misbehaves in the seam between two `update` calls — so it passes every
published vector, all of which arrive in a single call. `blockbuf` is that
logic once, tested over every length up to three blocks at every possible
split point. MD5's `T` table is generated from its definition
`floor(2^32 · |sin(i+1)|)` rather than transcribed, which removes that error
class rather than testing for it.

`HashState` is now a three-variant enum over `md5::Md5` / `sha1::Sha1` /
`sha2::Sha256`, and `diskimager` carries four new tests:

| Test | What the stub would have failed |
|---|---|
| `hashes_match_their_published_vectors` | everything — six vectors, empty and `"abc"`, all three algorithms |
| `each_algorithm_produces_its_own_length_and_value` | nothing; it guards the picker wiring, not the maths |
| `splitting_the_input_does_not_change_the_digest` | nothing; it guards the *new* risk, that a file read in chunks hashes differently from a file read whole |
| `finalize_is_repeatable` | nothing; the real hashers consume themselves on finalize, so `HashState` finalizes a clone |

117 tests pass in `diskimager`; 26 tests and 5 doctests in the three crates.
The crate's 29 remaining clippy warnings are pre-existing and unchanged —
measured before and after, identical counts by lint — and are tracked
separately under the `apps/**` half of the lint debt.

**The generalisable lesson, restated because it is the only one that
matters:** none of the eight original tests was wrong. They were all true of
`state[i % 8] = state[i % 8] * 31 + byte`. Shape tests cannot fail on a stub,
so a subsystem with only shape tests is untested no matter how many it has.
