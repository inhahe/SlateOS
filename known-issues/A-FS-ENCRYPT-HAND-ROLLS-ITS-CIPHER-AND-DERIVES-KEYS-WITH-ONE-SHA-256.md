## A-FS-ENCRYPT-HAND-ROLLS-ITS-CIPHER-AND-DERIVES-KEYS-WITH-ONE-SHA-256 (lane A, 2026-10-15) — OPEN

**Status:** OPEN (lane A). Found while evicting the kernel's TLS and SSH
cryptography with the in-kernel network applications (decision 934).

**In short:** the kernel shell's `encrypt` command (`kernel/src/fs/encrypt.rs`)
encrypts a file under a passphrase, but the protection is much weaker than it
looks. The passphrase becomes a key through a single SHA-256, so a stolen file
can be attacked at billions of guesses per second; the cipher is a
hand-written ChaCha20 kept in the kernel; and until 2026-10-16 each file's nonce (the number that
must never repeat under one key) was built from the CPU's cycle counter and the
clock rather than from the random-number generator. Nothing else in the system
uses this format, and `design.txt` does not ask for per-file encryption at all.

### What is wrong

| Part | Now | What it should be |
|---|---|---|
| Key derivation (`derive_key`) | `SHA-256(passphrase ‖ key name ‖ "FENC_KDF_V1")`: one fast hash, salted by the key's *name*, so the same passphrase and name give the same key on every machine | a slow, memory-hard function with a random salt -- Argon2id, as `fs::diskencrypt` already does through the vendored `seal` crate |
| Cipher | ChaCha20 and HMAC-SHA256 (encrypt-then-MAC), hand-written in `kernel/src/crypto.rs` -- since 2026-10-16 their only users; the module had its own copies until then | `seal`'s XChaCha20-Poly1305: design-decisions §539 says primitives are ported, never written here |
| Nonce (`generate_nonce`) | TSC and the low bytes of the real-time clock -- fixed 2026-10-16 to draw from `crate::rng`, as `diskencrypt` does | done |

The cipher itself is correct (the RFC 8439 and RFC 4231 vectors pass in
`crypto::self_test_hmac_chacha20`); the defect is that it is a copy kept
outside the vetted crates.

### Why it is not simply ported to `seal` now

The salt has nowhere to live. The keystore is in memory and holds *derived*
keys, not passphrases; files outlive the boot. With a random salt, the key a
user adds after a reboot differs from the one their files were sealed under,
and they can no longer be opened. The working designs each change what the
feature is:

- **Salt in each file, derive per file** (`age`'s passphrase mode): the
  keystore keeps the passphrase rather than a key, and every encrypt or
  decrypt pays one Argon2id run -- slow by design.
- **A wrapped file key** (`fscrypt`'s v2 policies): the passphrase unlocks a
  master key stored on disk once, and files carry keys derived from it. This
  is a filesystem feature (per directory, policy in the inode), not a shell
  command.
- **Delete it**: the feature is not in `design.txt`, it is reachable only from
  the kernel shell, and file encryption for users belongs to a userspace tool
  or to the filesystem, not to a debugging shell.

Lane A's recommendation is the last, with `fscrypt`-style encryption as a
future roadmap item if the operator wants per-file encryption at all. That is
a product decision and is not taken here.

### Where

`kernel/src/fs/encrypt.rs`: `derive_key`, `chacha20_crypt` (now on
`crate::crypto::chacha20_xor`), `hmac_sha256` (now `crate::crypto`'s).
