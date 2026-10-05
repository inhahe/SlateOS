## C-SHA-256-IS-IMPLEMENTED-ELEVEN-TIMES-IN-THIS-TREE (lane C, 2026-08-17)

**In short:** eleven separate copies of the same cryptographic hash function
have been written by hand in this repository. Each one can be independently
wrong, and each one has to be independently reviewed, tested and fixed. Some
of them guard logging in and unlocking the screen.

**Where** (found by `grep -rn "fn sha256" --include=*.rs`):

| Copy | Lane | Shape |
|---|---|---|
| `kernel/src/crypto.rs` | A | one-shot |
| `posix/src/sha2.rs` | B | one-shot, has the FIPS million-`a` vector |
| `posix/src/crypt.rs` | B | via `sha2` |
| `init/loginmgr/src/main.rs` | B | block compression + one-shot |
| `userspace/coreutils/src/bin/sha256sum.rs` | B | one-shot |
| `userspace/backup/src/main.rs` | B | streaming |
| `kernel/src/oci.rs` | A | digest string |
| `kernel/build.rs` | A | build-time |
| `gui/credentials/src/main.rs` | **C** | one-shot |
| `apps/lockscreen/src/main.rs` | **C** | one-shot |
| `apps/backup/src/main.rs` | **C** | streaming `Sha256` struct |

All three lane-C copies do carry the standard known-answer vectors (empty,
`"abc"`, the 448-bit message), so none of them is presently *wrong*. That is
luck holding, not a design.

**Proper fix:** one implementation, one set of test vectors, shared. Lane C
can extract its three into a small top-level crate alongside the existing
`byteread` / `textfind` / `textfmt` utility crates and adopt it; lanes A and B
have to opt in themselves, so the cross-lane half is a request rather than an
edit. Note `kernel/` is `no_std`, so the shared crate must be `no_std` with an
`alloc`-free one-shot API to be adoptable by all three lanes.

### Correction and progress, 2026-08-17

**It is twenty-six, not eleven.** The original count came from
`grep "fn sha256"`, which misses every copy that names its entry point
something else or exposes only a `Sha256` struct. Two greps are needed to see
them all, and neither alone is sufficient:

```
grep -rln "0x6a09e667" --include=*.rs .   # the eight IV words
grep -rln "fn sha256\|struct Sha256" --include=*.rs .
```

The union is 26 files. The original entry also missed one of lane C's own:
**`apps/diskimager`** has a streaming copy, so lane C had four, not three.

**Done.** `sha2/` now exists at the workspace root — `no_std`, no `alloc`, the
four FIPS 180-4 vectors, a streaming form cross-checked against the one-shot
one at every length up to three blocks and every split within each length, and
a `benches/rate.rs` (commit `d8ad84f54`). `gui/credentials` is migrated
(`eb6e77799`), which is also the first evidence for the claim that the copies
cost something: it was **22% faster** afterwards (1.20 vs 1.54 µs/iter on a
70-byte input, both measured in one process), because that copy allocated a
`Vec` per call for the padded message. That matters concretely — the
credential KDF runs 100 000 hashes per unlock.

**Remaining, 25 copies.** Lane C's three: `apps/backup` (streaming +
`sha256_bytes`/`sha256_hex`/`sha256_file`), `apps/diskimager` (streaming),
`apps/lockscreen` (one-shot). Lanes A and B own the other 22 and must opt in
themselves; requests are filed rather than edits made.

**What consolidating does and does not buy.** It does not make the primitive
vetted — a single unvetted SHA-256 is still unvetted, and whether this tree
should be writing its own crypto at all is `open-questions.md` C-Q5. What it
buys is that the answer has one place to land, that a mistake is caught once
rather than needing to be caught 26 times, and — per the measurement above —
that the duplication was costing performance as well as review budget.

### Lane C is done, 2026-08-17

All four of lane C's copies are gone. `gui/credentials` (`eb6e77799`),
`apps/diskimager` (`65883cf92` — which was not a migration but a bug fix; see
`C-THE-DISK-IMAGER-VERIFIES-NOTHING-AND-SAYS-SHA-256` below), and now
`apps/backup` and `apps/lockscreen`. **22 copies remain, all in lanes A and B**
— 20 in B, 3 in A, minus `posix/src/crypt.rs` which correctly delegates to
`posix/src/sha2.rs` rather than carrying its own.

The migrations were not neutral. Deleting the copies removed clippy warnings
in bulk, because a hand-transcribed FIPS table is one long run of exactly the
constructs the workspace lints forbid — `w[i - 15]`, `h[i] = h[i] + a`,
`block_start + 64`:

| Crate | warnings before | after |
|---|---|---|
| `apps/backup` | 368 | 266 |
| `apps/lockscreen` | 85 | **11** |

That is 176 warnings that were never going to be fixed in place, because
fixing them means bounds-checking a loop whose bounds are the specification.
It is worth stating as a general result: **an inlined copy of a published
algorithm is a large, permanent lint-debt liability, and moving it into a
crate that is written once against the vectors is the only way to discharge
it.** The remaining `apps/**` lint debt is now dominated by ordinary
application code, which is fixable.

Two further findings from the audit, both recorded separately:

- The mechanical check that found the disk-imager stub — extract every
  8-hex-digit literal from a file and look for a contiguous run equal to the
  64-word K table and the 8-word IV — also found the **same stub hashing
  system passwords** in `userspace/login` and `userspace/chpasswd`. See
  `C-THE-SAME-STUB-IS-THE-SYSTEM-PASSWORD-HASH` at the end of this file.
- Five further lane-B copies have no known-answer vector at all
  (`userspace/backup`, `pkg`, `rsync`, `ssh`, `useradm`), though all five do
  carry the full constant tables. Listed in
  `requests/c-b-passwd-and-login-disagree-about-etc-shadow.md`.
