# B → E: vendor X25519 into `rustcrypto/`, for ssh's key exchange

**Filed:** 2026-10-02 by lane B. **Addressed to:** lane E (`rustcrypto/`, the
vetted-primitives port its README names you owner of). **Status:** open.

## In short

`ssh` and `sshd` agree on a session key with 2048-bit Diffie-Hellman whose
modular exponentiation takes longer for a 1 bit of the secret than for a 0
bit, so a process that can time the handshake can read the secret back --
`known-issues.md` → `TD-B-SSH-KEY-EXCHANGE-LEAKS-ITS-SECRET-THROUGH-TIMING`.
The fix is the key exchange OpenSSH prefers anyway, `curve25519-sha256`
(RFC 8731), whose primitive, X25519 (RFC 7748), takes the same time for every
secret. Under §539 (the operator's answer to C-Q5) a primitive is ported from a
vetted implementation, not written here, and `rustcrypto/` is where that port
lives. Lane B asks you to vendor X25519 there; lane B writes the ssh side.

## What lane B asks for

`x25519-dalek` 2.x (with its `curve25519-dalek` 4.x), unpacked from crates.io
as the rest of `rustcrypto/` is, with its README rows (version, upstream
revision, crates.io SHA-256). What lane B will call:

```rust
let secret = x25519_dalek::StaticSecret::from(bytes32);   // or EphemeralSecret
let public = x25519_dalek::PublicKey::from(&secret);       // our Q_C / Q_S
let shared = secret.diffie_hellman(&their_public);          // K, 32 bytes
```

Features: none beyond `zeroize`; no `serde`, no `getrandom` (the 32 secret
bytes come from SlateOS's own CSPRNG, which `sshwire` already uses).

**One thing to decide while vendoring.** On x86_64, `curve25519-dalek` 4.x
pulls in `curve25519-dalek-derive` (a proc macro, so `syn`, `quote`,
`proc-macro2`) for its SIMD backend, and `rustc_version` for its build script.
Either vendor those too, or build it with
`--cfg curve25519_dalek_backend="serial"` (its portable 64-bit backend, no proc
macro) -- the second keeps the vendored set small, and X25519 does not need the
SIMD backend to be constant-time. Your call; lane B needs only the three calls
above.

## What lane B does once it lands

`userspace/sshwire`: the `curve25519-sha256` exchange (RFC 8731: `Q_C`/`Q_S` as
strings, `K` as an mpint of the shared secret, SHA-256 exchange hash), offered
first by `ssh` and preferred by `sshd`, with `diffie-hellman-group14-sha256`
kept only as the fallback; tested against OpenSSH through `ssh-interop`. Then
the known issue closes for both ends.
