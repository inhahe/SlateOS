### TD-B-THE-AES-CTR-COUNTER-IS-REUSED-BY-THE-CLIENT-AND-INVENTED-BY-THE-SERVER

**Status: FIXED** (`userspace/sshwire/src/lib.rs`, `Aes128Ctr`). Found
2026-09-05 while surveying the two crates' remaining duplicated functions —
the fourth two-programs disagreement found by reading, and the first that is a
confidentiality bug rather than an interoperability one.

**In short:** SSH encrypts with a stream cipher, which works by generating a
long pseudo-random "keystream" and XOR-ing your data with it. The one absolute
rule of such a cipher is that a given stretch of keystream is used **once**.
Our client used the *same* stretch for every single packet it sent. Anyone
recording the connection could XOR two of our packets together, cancel the
keystream out entirely, and read the traffic without needing any key. The
server, separately, computed its counter by a formula it had invented, which
matched neither the client's nor the RFC's — so the two ends could not have
decrypted each other in any case.

#### What each end did

AES-CTR (RFC 4344 §4) keeps one 16-byte counter block per direction. It starts
at the derived IV and is incremented by one **for every 16-byte block
encrypted, continuously for the life of the key**. It is never reset and never
derived from anything else. The block cipher is applied to the counter, and the
result is the keystream.

| | Counter for block *b* of packet *n* | Consequence |
|---|---|---|
| `ssh` | `IV + b` — restarted from `IV` at every packet | Every packet in a direction is XOR-ed with the **same** keystream |
| `sshd` | `IV + n*256 + b` — recomputed per block | Differs from the client, so neither can decrypt the other; and packets over 4 KiB (256 blocks) run into the next packet's keystream |
| RFC 4344 | `IV + (total blocks previously encrypted) + b` | — |

The client carried a doc comment asserting the correct behaviour —
*"Across packets, we track the IV globally (the EncryptionState's IV is
incremented after each packet)"* — and nothing in the crate ever wrote to
`enc.iv_c2s` or `enc.iv_s2c` after key derivation. The comment described a
design that was never implemented, which is why reading the function alone did
not reveal the bug; it took grepping for assignments to the field.

The server's `build_ctr` carried its own explanation — *"We add seq \*
(large_blocks) + block_idx to get the correct counter"* — with `large_blocks`
hard-coded to 256 and no citation. There is no reading of RFC 4344 under which
the sequence number enters the counter.

#### Why it matters

Keystream reuse is not a degradation of a stream cipher, it is the removal of
it. Given ciphertexts `C1 = P1 ^ K` and `C2 = P2 ^ K`, an observer computes
`C1 ^ C2 = P1 ^ P2` with no key material at all, and SSH payloads are
structured enough (fixed message numbers, known field layouts, terminal echo)
that separating the two plaintexts from their XOR is routine. The affected
traffic is everything after `NEWKEYS`: the password in
`SSH_MSG_USERAUTH_REQUEST`, and every keystroke and byte of output thereafter.

It had not yet been *exploitable* only because the two ends could not complete a
handshake at all (see
`TD-B-SSHD-SIGNS-AN-EXCHANGE-HASH-OVER-A-CLIENT-VERSION-THE-CLIENT-NEVER-SENT`
and the two identification-line bugs). Those are now fixed, so this one had
become live.

#### Why it never fired in a test

The same reason as the other three. `ssh` tested its cipher against its own
expectations and passed; `sshd` tested its cipher against its own expectations
and passed. Both suites contained AES-CTR roundtrip tests, and a roundtrip test
cannot see this class of bug at all: encrypt-then-decrypt with the same wrong
counter returns the plaintext perfectly. What neither suite contained was a test
that encrypted **two** packets and asserted the keystreams differed, or one that
compared one end's counter against the other's.

#### The fix

The counter is state, so the fix is a type that owns it:
`sshwire::Aes128Ctr::new(key, iv)` holds the key schedule and the running
counter, and `apply(&mut self, data)` advances it by exactly the number of
blocks consumed. There is one per direction, created at `NEWKEYS` and carried in
each crate's `EncryptionState`. `peek_block` exists for the
length-field peek that must not consume, and takes `&self`.

*(Updated 2026-09-05: `EncryptionState` no longer exists in either crate. The
two ciphers now live in `sshwire::PacketCodec` alongside the two sequence
numbers, and `NEWKEYS` is a single `codec.activate(role, k, h, session_id)`
call. Nothing about the cipher itself changed; there is simply one fewer
per-crate struct wrapping it.)*

Because the counter is now inside the cipher, `seq` is no longer a parameter of
anything in this path — there is nowhere for a `seq * 256` to be reintroduced.
And because both ends construct the same type from the same crate, the two
counters cannot drift apart again without the shared tests failing.

Tests added in `sshwire`, all stated against **published** vectors rather than
against our own output, which is the point — the previous tests each checked one
crate's cipher against that same cipher:

| Test | Source | What it would catch |
|---|---|---|
| `the_fips_197_aes_128_vectors_encrypt_as_published` | FIPS-197 App. B, §C.1 | a wrong S-box, `mix_columns`, or round count |
| `the_aes_128_key_schedule_matches_the_published_one` | FIPS-197 App. A.1 | an expansion that goes wrong partway and stays wrong |
| `the_rfc_3686_aes_ctr_vectors_encrypt_as_published` | RFC 3686 §6, vectors 1 and 3 | the counter rule, incl. a 36-byte message's short final block |
| `the_nist_sp_800_38a_ctr_vector_encrypts_as_published` | NIST SP 800-38A §F.5.1 | the counter *walk* — four blocks in one call |
| `the_rfc_4231_hmac_sha256_cases_come_out_as_published` | RFC 4231 §4.2–4.4 | the MAC, against the standard instead of itself |
| `a_key_longer_than_the_hash_block_is_hashed_down_first` | RFC 4231 §4.7 | the one HMAC branch SSH's own key lengths never reach |

and four that no published vector covers, because they are properties of the
type rather than of the algorithm:

- **`the_keystream_never_repeats_across_packets`** — the bug itself. Two `apply`
  calls on identical plaintext must produce different ciphertext, and XOR-ing the
  two results must not cancel the key out. This is the assertion the roundtrip
  tests were structurally unable to make.
- `a_peek_does_not_consume_the_block_the_next_apply_needs` — peeking twice, then
  applying, must decrypt the packet whole.
- `a_partial_final_block_still_advances_the_counter_by_one` — otherwise two ends
  that disagree about a short tail part company on the *next* packet, which is
  the hardest divergence to trace back.
- `the_counter_carries_across_all_sixteen_bytes` — reachable in practice only
  after 2^8k blocks, so here is the only place it is ever exercised.

Deleted rather than moved: sshd's `test_aes_encrypt_decrypt_roundtrip` and ssh's
`aes_ctr_round_trips_a_length_that_is_not_a_block_multiple`, for the reason
above; and ssh's `aes_ctr_declines_a_short_key_or_iv_rather_than_encrypting_with_padding`,
because `Aes128Ctr::new` takes `&[u8; 16]` and a short key no longer compiles.
