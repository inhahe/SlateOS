## TD-B-SSH-KEY-EXCHANGE-LEAKS-ITS-SECRET-THROUGH-TIMING (lane B, 2026-08-21)

**In short:** When `ssh` connects, it and the server each pick a secret random
number and combine them to agree on a session key -- this is Diffie-Hellman key
exchange. Our client computes its half with an algorithm whose *running time
depends on the secret*: bits of the secret that are 1 take measurably longer
than bits that are 0. Anyone who can time the handshake precisely -- a process
on the same machine, or in principle a well-placed network observer -- can read
the secret back out of those timings and then decrypt the whole session. The fix
is to stop using this key exchange and use X25519 instead, which is designed so
that every secret takes exactly the same time.

**Where.** `userspace/sshwire/src/lib.rs`, the `BigUint` section. Two
data-dependent branches, both on the security-critical path:

> **Moved and widened, 2026-09-05.** This was written against
> `userspace/ssh/src/main.rs`, where the type used to live. It is now
> `sshwire`'s, and the entry applies to **both** ends rather than to the client
> only: `sshd` had its own copy with the same square-and-multiply `mod_pow`, so
> the server has been leaking its half of the exponent by the same channel the
> whole time — and a server is the more attractive target, being the end an
> attacker can make handshake on demand. See
> `TD-B-THE-SERVER-SPENT-EIGHTY-SECONDS-OF-CPU-PER-HANDSHAKE-BECAUSE-THE-FIX-HAD-NOWHERE-TO-REACH-IT`.
> Switching to X25519 fixes both at once, which it would not have before.

- `mod_pow` is square-and-multiply: `for i in (0..bits).rev() { result =
  result.mod_mul(&result, modulus); if exp.bit(i) { result =
  result.mod_mul(&base, modulus); } }`. The extra multiply happens only for a
  1 bit, so total time is (roughly) linear in the popcount of the exponent, and
  the *pattern* of fast/slow rounds is the exponent itself.
- `div_rem`'s Knuth add-back step (`if t < 0 { ... }`) runs an extra pass over
  the divisor only when the trial quotient digit was one too large. That is
  operand-dependent, so even the "constant" squarings are not constant-time.

The exponent in question is the DH private exponent generated in
`key_exchange`; the modulus is the 2048-bit group 14 prime.

**How it was found.** While rewriting `BigUint` onto 32-bit limbs to fix the
unusable 77-second handshake (commit `07cfa2521`). The rewrite made the
arithmetic ~350x faster; it did not make it constant-time, and it was never
written to be.

**How bad it is, honestly.** Bounded, but not negligible. The exponent is
ephemeral: a fresh one is drawn per connection, so an attacker gets exactly one
timing trace per secret rather than the thousands that classic RSA/DSA timing
attacks accumulate against a long-lived key. A single trace of a 2048-bit
modexp is not obviously enough to recover the exponent end-to-end. But "not
obviously enough" is not a security argument, the leak is a per-bit one rather
than a statistical aggregate, and a co-resident process can time far more
precisely than a network attacker. Treat it as a real weakness that has not yet
been demonstrated, not as a theoretical one.

**The proper fix, and why it is a replacement rather than a repair.** Making
this code constant-time means rewriting `mod_pow` as a fixed-window ladder with
constant-time table selection, and rewriting `div_rem` to do the add-back
unconditionally under a mask -- i.e. writing a constant-time bignum library by
hand, unreviewed, which is the exact category of code that is famously got
wrong. The better answer is to delete the requirement:

1. Get an **X25519 (RFC 7748)** into the tree, beside the existing `ed25519`.
   The Montgomery ladder is naturally constant-time -- it performs the same
   operations for every scalar bit and selects between them with arithmetic
   rather than a branch -- the scalar is a fixed 32 bytes so there is no
   variable-length loop, and RFC 7748 s5.2 ships known-answer vectors to test
   against.
2. Switch ssh's key exchange to **`curve25519-sha256`** (RFC 8731), which is
   what OpenSSH prefers by default anyway, keeping
   `diffie-hellman-group14-sha256` only as a fallback for servers that lack it.

That removes the hand-rolled 2048-bit modexp from the handshake entirely. The
`BigUint` code would remain only for the fallback path (and could then be
dropped altogether if the fallback is dropped).

**Step 1 is a *port*, not an implementation -- decided 2026-08-21.** This entry
was first written saying "implement X25519 in `posix/`". The operator answered
C-Q5 the same day with **option C: vendor the cryptographic primitives, keep our
own glue** (`design-decisions.md`, lane C's band). X25519 is a primitive by any
reading, and it is a primitive whose *entire* value here is a property no test
can check -- so hand-writing it would reproduce, in new code, precisely the
defect this entry is about. Vendor a vetted implementation (RustCrypto's
`curve25519-dalek`, or BearSSL's `c25519` if a C port is preferred) and keep the
SSH-side plumbing ours. That also means step 1 is no longer lane B's to write
alone: which implementation gets vendored, and where it lands, is the same
decision C-Q5 set in motion for the vault and the password hash, and should be
made once for all three consumers rather than three times.

**Checked 2026-08-21:** `posix/src` has no X25519 today. Grepping
`x25519|curve25519` there matches only Linux uapi *type definitions*
(`linux_wireguard*.rs`, `linux_crypto_kpp*.rs`) -- names in an ABI, not an
implementation. So there is nothing in the tree to wire up yet either way.

**If never fixed:** no regression -- this is how the client has behaved since it
was written, and the 350x speedup neither introduced nor worsened it. It does
not get worse with time on its own. But it is the one remaining defect in ssh
that a rewrite cannot be argued out of: every other finding from the lint sweep
was fixed in place, and this one was left because the correct fix is a new
primitive rather than an edit.

**Status 2026-10-02: waiting on lane E for the primitive.** The port C-Q5 set in
motion now exists -- `rustcrypto/` (§539, 2026-09-27: XChaCha20-Poly1305 and
Argon2id, lane E's) -- and has no X25519. Asked of lane E in
`requests/b-e-vendor-x25519-for-ssh.md` (`x25519-dalek` 2.x); lane B writes the
`curve25519-sha256` exchange in `sshwire` once it lands.
