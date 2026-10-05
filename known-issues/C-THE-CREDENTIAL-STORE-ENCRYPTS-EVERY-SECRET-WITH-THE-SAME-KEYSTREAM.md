## C-THE-CREDENTIAL-STORE-ENCRYPTS-EVERY-SECRET-WITH-THE-SAME-KEYSTREAM (lane C, 2026-08-17)

**In short:** the credential manager — the thing that holds the user's saved
passwords — scrambles every secret in the vault with the *same* repeating
pattern. Anyone who can read the vault file can recover its contents without
ever learning the master password, by lining two entries up against each
other and cancelling the pattern out. This is the single worst defect
currently known in lane C's tree.

**Where:** `gui/credentials/src/main.rs`, `encrypt` / `decrypt` /
`generate_keystream`.

`generate_keystream(key, len)` produces `SHA-256(key ‖ 0) ‖ SHA-256(key ‖ 1) ‖
…`. It takes no nonce (a number used once, mixed in so that encrypting the
same thing twice gives different output). So it is a pure function of the
session key, and every credential in a vault is XORed with the identical
keystream. XOR two ciphertexts together and the keystream cancels, leaving the
two plaintexts XORed with each other — the classic "two-time pad", which is
routinely solved by hand for text. No key recovery is needed and no master
password is needed; read access to the stored ciphertexts is enough.

The doc comment says "this is a demonstration cipher; production use would
employ AES-256-GCM", which covers *weak* but not *broken*: a demonstration
cipher is still expected to keep two records from decrypting each other.

**Proper fix (does not need any new primitive):** give every encryption its own
nonce and mix it into the keystream — `SHA-256(key ‖ nonce ‖ counter)` — then
store the nonce beside the ciphertext. A per-record sequence number that is
persisted and never reused under a given key is a sufficient nonce and needs
no randomness, so this is fixable today with the SHA-256 already in the file.
That turns the construction into SHA-256 used as a counter-mode PRF, which is
a defensible stream cipher rather than a broken one.

Still missing after that fix, and requiring things the tree does not yet have:
authentication (the ciphertext can be flipped bit-for-bit undetected — wants an
HMAC or a real AEAD) and a vetted AES-256-GCM. See
`open-questions.md` → "Do we write our own cryptographic primitives?".

**Resolved 2026-08-17, as described.** `encrypt` now takes a nonce and returns
`nonce ‖ ciphertext`; `decrypt` reads the nonce back off the front and returns
`Result`, because a blob shorter than the 8-byte nonce never came from
`encrypt`. `generate_keystream(key, nonce, len)` is
`SHA-256(key ‖ nonce ‖ counter)`. `CredentialStore` supplies nonces from
`next_nonce`, a counter that only ever increases; `take_nonces(n)` hands out a
contiguous base so a caller already holding `&mut credentials` (the re-encrypt
loop in `set_master_password`) can still get fresh ones. Gaps are harmless —
nonces must be unique, not contiguous.

Regression test:
`the_same_plaintext_twice_does_not_produce_the_same_ciphertext` asserts the
*bodies* differ and not merely the nonce prefixes, plus
`a_blob_too_short_to_hold_a_nonce_is_rejected_not_misread`.

**One thing a persistence layer must not get wrong**, and there is no
persistence layer yet: `next_nonce` has to round-trip to disk. A vault
reloaded with the counter reset to zero re-issues nonces it has already used
and reintroduces exactly this bug. The field carries a comment saying so.

The authentication half is *not* fixed and remains open under C-Q5.
