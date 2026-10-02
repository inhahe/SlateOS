## §331 — Lane B's ten SHA-256 copies fold into the shared `sha2` crate; the SHA-1 and SHA-512 beside them stay put

**Date:** 2026-08-18
**Decided by:** Claude (autonomous)

**In short:** Ten programs under `userspace/` each contained their own copy of
SHA-256 — the same 64 magic numbers and the same arithmetic, typed out ten
times. A checksum is only useful if two programs computing it agree, so ten
private copies is ten chances to disagree, and a disagreement would be silent:
a wrong SHA-256 still returns 32 normal-looking bytes, so a backup would simply
stop matching the files it describes with no error anywhere. All ten now call
one shared crate that already existed for this purpose. The copies of *other*
hashes sitting next to them — SHA-1 and SHA-512 — were left alone, because the
shared crate deliberately does not offer those.

### What was there

Root `sha2/` was written to be the tree's single SHA-256 and had two consumers.
Meanwhile lane B's tree had ten more implementations, none of them using it:

| Crate | What the digest is for | Cost of a divergence |
|---|---|---|
| `backup` | manifest hashes | a manifest nothing else in the tree can check |
| `rsync` | `-c` file comparison | two identical files declared different, or worse |
| `ssh`, `sshd` | KEX, HMAC, host-key fingerprints | a handshake that fails against every implementation *except its own twin* |
| `cryptsetup` | PBKDF2-SHA256 key derivation | a key that never opens the disk it just encrypted |
| `doas` | password hashing | header claimed it "matches passwd utility"; nothing checked that |
| `fio` | data-integrity verification | the verifier is the thing being verified |
| `sha256sum`, `coreutils/sha256sum` | *the entire output of the program* | the least defensible copy of all |
| `ssh-keygen` | key fingerprints, signature digests | a fingerprint a human compares against another tool's |

Together they were ~1250 lines of round constants and compression function.
Every one was correct. That is the point: correctness by ten repetitions of a
64-constant table is luck, not design, and the failure mode gives no signal.

### What stays, and why

*`posix/src/sha2.rs` stays.* It backs SHA-crypt (`$5$`/`$6$`), which needs
SHA-512 and MD5 as well; root `sha2/` is SHA-256 only. Folding just the
SHA-256 half out of a file whose other half must remain would leave that file
depending on a crate for one of its three primitives — more seams, not fewer.
§329 already reached the same conclusion from the other direction.

*The SHA-1 in `sha256sum` and the SHA-512 in `sha256sum` and `ssh-keygen`
stay,* for the same reason: the shared crate does not provide them. Ed25519
needs SHA-512 by definition, so `ssh-keygen` cannot drop it.

The result is that root `sha2/` is now the SHA-256 for all of lane B's
userland, and the remaining duplication in this lane is of *other* algorithms —
which is a smaller and much more visible problem than ten copies of one
algorithm scattered across ten programs that all have to agree.

### The alternative that was rejected

*Extend `sha2` to cover SHA-1 and SHA-512 too, then fold everything.* It is the
tidier end state and should probably happen eventually. It was not done here
because it would have mixed two changes: a mechanical consolidation whose
correctness is checked by the existing FIPS vectors in each crate, and the
authoring of two new primitives, which is not mechanical at all. The vectors
that used to check ten private copies now check the shared one — `fio` and
`coreutils/sha256sum` in particular kept their full FIPS test batteries, so the
shared implementation is now the thing those vectors are pointed at. Whether
this project should hand-write cryptographic primitives at all remains
`open-questions.md` → C-Q5, and is untouched by this.
