## 1218. The vetted cryptography is RustCrypto's XChaCha20-Poly1305 and Argon2id, vendored whole, with `seal` as the one API over them

**Date:** 2026-09-27
**Lane:** E
**Decided by:** Claude (autonomous), inside the operator's §539 (C-Q5: primitives are ported, not written) -- lane A asked lane E to do the port and set the directory and the `no_std` requirement

**In short:** the password manager needs to keep its vault on disk,
encrypted, and to write an encrypted backup (C-Q25); the system keyring and
the disk encryption need the same two things. §539 says the cipher and the
password hash must be taken from code other people have spent years
attacking, not written here. They are now: RustCrypto's XChaCha20-Poly1305
(the cipher, which also detects any tampering) and Argon2id (the password
hash, which makes each guess cost an attacker memory as well as time),
copied exactly as published to crates.io into `rustcrypto/`, with one small
crate of ours, `rustcrypto/seal`, that every caller uses.

**Why these two.**
- *XChaCha20-Poly1305 rather than AES-256-GCM.* AES in software is only
  safe against a local timing attacker when bitsliced or run on AES-NI; the
  tree's own `aes` is neither, and ChaCha20 is constant-time by its
  construction. And its 24-byte nonce can be drawn at random for every
  message -- GCM's and plain ChaCha20-Poly1305's 12 bytes cannot safely be,
  over a vault's lifetime of saves, which would push every caller into
  keeping a counter that must never go backwards (the bug `gui/credentials`
  already had once).
- *Argon2id rather than scrypt or PBKDF2.* RFC 9106's recommendation, and
  resistant both to GPUs (memory-hard) and to side channels (the `id` half).
  The default, `KdfParams::RECOMMENDED`, is RFC 9106 §4's second setting --
  64 MiB, three passes, four lanes -- because its first (2 GiB) is more than a
  small machine can spend on an unlock.

**Why vendor whole crates** rather than port only the arithmetic: the
published crates are what upstream reviewed, tested and fuzzed; a trimmed copy
would be ours, and "ours" is what §539 says not to trust. The cost is 23
crates and 4 MB, most of it trait plumbing and `typenum`. The only change is
to their manifests (path dependencies; `cpufeatures` loses an aarch64-only
`libc`), recorded in `rustcrypto/README.md` with each crate's checksum and
upstream revision.

**Why `seal`, a crate of ours, on top.** Lane C asked for it: one place where
the cipher, the nonce size and the Argon2 variant are chosen, so the password
manager and the keyring cannot drift into two vault-key derivations. It takes
the nonce from the caller because it builds for the kernel too and has no
randomness of its own.

**What it asks of the kernel** (lane A's to decide): the two vendored ciphers
need `--cfg chacha20_backend="soft"` and `--cfg poly1305_backend="soft"` on
`x86_64-unknown-none` -- LLVM aborts on `poly1305`'s AVX2 code for a
soft-float target -- and `argon2` takes an AVX2 path at run time with no
switch, so kernel code calling it must save the vector registers or keep AVX
off in `XCR0`.

**Alternatives:**
- *libsodium, through C.* The most-reviewed choice, but it brings a C
  toolchain into every build that needs it, the kernel's included.
- *Port only the core functions*, dropping the trait crates. Smaller, and no
  longer upstream's code.
- *AES-256-GCM on AES-NI.* Fast, but needs `unsafe` intrinsics and a CPU
  check, and still leaves the 12-byte nonce problem.
