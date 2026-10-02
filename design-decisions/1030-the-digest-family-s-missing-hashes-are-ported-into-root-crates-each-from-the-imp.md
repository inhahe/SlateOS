## 1030. The digest family's missing hashes are ported into root crates, each from the implementation nearest its program

**Date:** 2026-09-25
**Lane:** B
**Decided by:** Claude (autonomous, within the operator's §539)

**In short:** `sha224sum`, `sha384sum`, `sha512sum`, `b2sum` and `cksum -a`
needed four hash functions the tree had no shared copy of: SHA-224, SHA-384,
SHA-512 and BLAKE2b, plus SM3 for `cksum -a sm3`. §539 already settles
*whether* to write them (no: cryptographic primitives are ported from
implementations others have attacked for years). What was left to decide is
*where* the ports live and *which* implementation each is ported from. They
live in root crates, beside `sha1`, `md5` and `sha2`, so the next program that
needs one links it instead of writing a sixth copy; and each is ported from the
implementation closest to the program that needs it.

### Where they live

| Option | *What changes:* | For | Against |
|---|---|---|---|
| **Root crates (chosen)**: SHA-224/384/512 added to `sha2/`, new `blake2/` and `sm3/` | three crates any lane can link; `coreutils` depends on them | one copy per primitive, the pattern `sha1`/`md5`/`sha2`/`crc32` already follow; BLAKE2b is also Argon2's inner hash, which the password-hash half of C-Q5 will want | root crates are no lane's (A-Q11), so the change must stay additive -- which it does: new types in `sha2`, a new method in `blockbuf`, two new crates, and SHA-256's code untouched |
| Private modules inside `coreutils` | nothing outside `coreutils` can reach them | no shared crate edited | the next consumer writes its own copy, which is how the tree came to have 26 SHA-256s (see `sha2`'s crate docs) |

### Which implementation each is ported from

| Primitive | Ported from | Why that one |
|---|---|---|
| SHA-512, SHA-384 | RustCrypto `sha2` 0.11.0, the portable backend (`soft/compact.rs`) | MIT/Apache, the most-reviewed Rust implementation, and the loop form rather than the unrolled one, so a reader can check it against FIPS 180-4 line by line |
| SHA-224 | none needed: SHA-256 from other initial values, truncated (FIPS 180-4 §6.3) | the existing `Sha256` gained a private constructor taking the initial value; no second compression function |
| BLAKE2b | the BLAKE2 reference implementation, `blake2b-ref.c` | it is the very file GNU `b2sum` is built from, so for the program this was brought in for, the arithmetic is upstream's rather than merely equivalent to it; CC0/OpenSSL/Apache |
| SM3 | RustCrypto `sm3` 0.5.0 | MIT/Apache; gnulib's `sm3.c` is the other candidate, but it is LGPL, and a root crate any lane may link should not carry that choice for them |

Each crate records the upstream version, the git revision and the SHA-256 of
the archive it was read from, which is what §539 asks for so that "keep it
current" has something to compare against. Where the port changes the code's
*shape* -- `last_chunk` windows instead of `w[i - 15]` indexing, SM3's
sixty-four unrolled macro calls written as the standard's loop -- the crate
docs say so, and the arithmetic is unchanged: every crate passes its
standard's vectors and a cross-check against Python's `hashlib`, which is an
independent implementation.

### Two smaller calls inside it

**SHA-512's 128-bit length field went into `blockbuf`, not around it.**
`blockbuf`'s `finalize` writes a 64-bit length; used for SHA-512 it would
write eight zero bytes where the high half goes, which is right for every
message under 2^61 bytes and silently wrong above. `finalize_wide` widens the
count before multiplying, so it is exact for every length the counter can
hold. Additive: the old method is unchanged.

**`cksum --debug` prints nothing.** GNU's x86 build reports whether its PCLMUL
CRC is in use. Ours has only the table CRC, so it answers as GNU built without
`USE_PCLMUL_CRC32` does -- silence -- rather than printing GNU's "not
detected", which would describe hardware it never looked at. Recorded as the
one `xfail` in `scripts/cksum-diff.sh` that is not `--help`/`--version`.

**Revisit when** C-Q5's vendoring decision is made for the vault: if it picks
a crate to vendor wholesale (RustCrypto's, most likely), these three should be
re-pointed at the same vendored copy rather than stay as a second port of the
same code.

**Where:** `sha2/src/sha512.rs`, `sha2/src/lib.rs` (`Sha224`), `blockbuf`
(`finalize_wide`), `blake2/`, `sm3/`; used by `userspace/coreutils/src/digest.rs`.
