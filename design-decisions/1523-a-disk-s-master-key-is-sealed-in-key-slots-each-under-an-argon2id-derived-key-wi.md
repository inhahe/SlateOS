## 1523. A disk's master key is sealed in key slots, each under an Argon2id-derived key, with the vendored XChaCha20-Poly1305

**Date:** 2026-10-02 · **Decided by:** Claude (operator-approved scope: §978's "real key derivation with a ported, vetted password hash") · **Lane:** A

**In short:** `diskencrypt::unlock_volume` used to accept any passphrase that
was not empty. Now a volume has a random master key that is never kept in the
clear while it is locked. Each key slot holds that key sealed under a key
derived from one passphrase -- or from a recovery key -- and unlocking works
only if a slot's seal opens. This is how LUKS2, the format Linux's
`cryptsetup` writes, does it. The derivation is Argon2id and the seal is
XChaCha20-Poly1305, both from the vendored RustCrypto crates through `seal`
(§539: ported, not written here).

**What changed:**
- A slot is: a random salt, the Argon2id cost it was made with, a random
  nonce, and the 64-byte master key sealed with the derived key. The volume's
  random identity and the slot's number are the seal's associated data, so a
  slot copied to another volume or position does not open.
- A wrong secret opens no slot: the cipher's tag is the check, and nothing
  derived from a secret is stored to compare against.
- Adding a slot or a recovery key needs the volume unlocked, as `cryptsetup
  luksAddKey` needs an existing passphrase; the last slot cannot be removed.
- A recovery key is 256 random bits shown once as eight groups of eight hex
  digits; its slot costs the least Argon2 allows, since a random 256-bit
  secret gains nothing from stretching.
- The derivation runs with no lock held; the master key is wiped from memory
  when the volume is locked; keys are drawn only once the random-number
  generator has been seeded.
- The made-up "System, /dev/sda1, 512 GiB" volume the table used to start
  with is gone: it starts empty.

| | What changes | For | Against |
|---|---|---|---|
| **Key slots sealing one master key (chosen, as LUKS)** | several passphrases and a recovery key open one volume; changing a passphrase rewrites one slot, not the disk | the design every Linux tool expects; the disk's key never depends on a passphrase | more to store |
| The passphrase's derived key is the disk key | one secret | simplest | a passphrase change re-encrypts the disk; no recovery key |

| | What changes | For | Against |
|---|---|---|---|
| **Passphrase slots at 19 MiB, two passes, one lane (chosen)** | an unlock costs a fraction of a second on hardware | OWASP's floor for Argon2id (and the `argon2` crate's default); kernel memory is the whole machine's | weaker against a GPU attacker than `seal::KdfParams::RECOMMENDED`'s 64 MiB in four lanes |
| 64 MiB, three passes, four lanes | stronger | RFC 9106's second recommendation | 64 MiB of kernel memory per unlock attempt |

The cost is stored in each slot, so it can rise later without breaking
existing slots.

**Not done** (`known-issues.md` `A-DISK-ENCRYPTION-HAS-NO-BLOCK-LAYER`):
nothing encrypts a disk's data with the key yet, and the slots live only in
memory.

**Revisit** when the block layer is built: the slots' on-disk form should be
LUKS2's if existing tools are to read SlateOS disks, which would fix this
format's details (it is LUKS-like now, not LUKS).
