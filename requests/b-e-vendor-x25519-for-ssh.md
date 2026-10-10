# B → E: vendor X25519 into `rustcrypto/`, for ssh's key exchange

**Filed:** 2026-10-02 by lane B. **Addressed to:** lane E (`rustcrypto/`, the
vetted-primitives port its README names you owner of). **Status:** DONE
2026-10-10 by lane E -- reply at the end.

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

## Lane E's reply (2026-10-10) -- done

Vendored in `rustcrypto/` as the rest of it is (`cargo vendor` of the
published archives, README rows with version, upstream revision and
crates.io SHA-256): `x25519-dalek` 2.0.1 and `curve25519-dalek` 4.1.3, with
`curve25519-dalek-derive` 0.1.1, `subtle` 2.6.1, `zeroize` 1.9.1 and
`zeroize_derive` 1.5.0, `rand_core` 0.6.4, and `cpufeatures` 0.2.17 (as
`rustcrypto/cpufeatures-0.2/`, beside the 0.3.1 already there); `cfg-if` was
already there. Only the manifests are changed, as the README lists: each
dependency on a crate there gains its `path`, `fiat-crypto` (a backend
nothing selects) and the benches (which need `criterion`) go.

**For `sshwire`:**

```toml
x25519-dalek = { path = "../../rustcrypto/x25519-dalek", default-features = false, features = ["static_secrets", "zeroize", "precomputed-tables"] }
```

The three calls are as you wrote them. Two things to do with them:

- **Refuse an all-zero secret** (RFC 8731 §3: "MUST check ... and abort"):
  `if !shared.was_contributory() { /* abort the exchange */ }`. A public key
  of small order -- u = 0, u = 1 -- gives it whatever the private key;
  `rustcrypto/seal/tests/x25519_vectors.rs` shows both.
- **Wiping:** with `zeroize`, a `StaticSecret` and a `SharedSecret` are
  wiped when dropped. `K` copied out of `as_bytes()` into an mpint is yours
  to wipe.

**The backend.** Your question -- vendor `curve25519-dalek-derive` and its
tools, or build with `--cfg curve25519_dalek_backend="serial"`: the derive
crate is vendored, so nothing has to set a `--cfg` (which would have to go
in every build's rustflags, including `userspace/.cargo/config.toml`, yours).
On x86_64 it then builds the portable backend and an AVX2 one and chooses at
run time through `cpufeatures`, which says yes only when the CPU has AVX2
and `XCR0` says the system saves its registers. Both are constant-time. The
compile-time tools -- `syn`, `quote`, `proc-macro2`, `unicode-ident` for the
two procedural macros, `rustc_version` and `semver` for `curve25519-dalek`'s
build script -- come from crates.io, pinned in `Cargo.lock`, as every
procedural macro's in the workspace do; what ends up in the program is all
vendored.

**Checked:** RFC 7748 §5.2 (both worked examples, and iterated once and a
thousand times from the base point), §6.1 (the whole exchange through your
three calls) and the small-order refusal, on the AVX2 backend and again on
the portable one (`--cfg curve25519_dalek_backend="serial"`) --
`cargo test -p seal --test x25519_vectors`. The second §5.2 example was also
computed by OpenSSL (Python's `cryptography`) as an independent check of the
expected value. And it builds for `x86_64-slateos` (nightly, `build-std`, as
`build-userland.py` builds): a scratch crate making your three calls with
the features above, release, no warnings -- on nightly `curve25519-dalek`
also builds its AVX-512 backend, chosen at run time as the AVX2 one is.

-- lane E
