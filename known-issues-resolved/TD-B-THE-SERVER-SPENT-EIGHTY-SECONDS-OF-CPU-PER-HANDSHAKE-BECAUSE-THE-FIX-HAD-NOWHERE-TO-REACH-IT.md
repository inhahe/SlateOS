### TD-B-THE-SERVER-SPENT-EIGHTY-SECONDS-OF-CPU-PER-HANDSHAKE-BECAUSE-THE-FIX-HAD-NOWHERE-TO-REACH-IT

**Status: FIXED**, 2026-09-05. `BigUint` is `sshwire`'s; both private copies are
deleted.

**In short:** the key exchange needs arithmetic on 2048-bit numbers, which is
too big for any integer type the machine has, so we wrote it by hand. We wrote
it twice — once in the client, once in the server. A year ago someone measured
the client's version taking **77 seconds of CPU to open one connection**, found
the reason, and rewrote it to be ~350× faster. That fix never reached the
server, because there was no shared place to put it and no reason for anyone
editing the client to look at the server. So the server has been doing the slow
version ever since — and doing it **twice per connection, before the client has
proved who it is**. Anyone who can open a TCP socket to it could pin a CPU core
for a minute and a half by connecting, with no password and no account.

### What each end had

| | `ssh` (client) | `sshd` (server) |
|---|---|---|
| representation | little-endian 32-bit limbs | big-endian bytes |
| division | Knuth algorithm D, one *limb* per step | long division, one **bit** per step, three `Vec` allocations per bit |
| measured | ~0.2 s per handshake (after commit `07cfa2521`) | the 77 s the client was rewritten away from |
| modexps per handshake | 2 | 2 |
| authentication first? | n/a | **no** |

The server's was not a variant or a simplification. It was, line for line, the
implementation the client had been rewritten away from — the original, kept
alive by being in a second file.

### Why this one is different from the other six

The other divergences in this stack are *mistakes*: two people wrote the same
thing and one of them wrote it wrong. This one is not a mistake anywhere. The
client's rewrite was correct, deliberate, measured and committed with a note
explaining itself. The server's copy was correct too — just slow. Nothing was
done wrong, and the outcome is still a remotely triggerable denial of service
sitting in the tree for a year.

That is the argument for extraction stated in its strongest form. Sharing a
function is usually justified as "so the two copies cannot disagree", which
sounds like a tidiness argument and invites the reply that careful people can
keep two copies in step. This is the other half, and it is not about care:
**a fix applied to one copy does not reach the other, and nobody involved ever
has to have made an error for that to hurt.** The client's author did not fail
to fix the server; there was no route by which they could have, short of
noticing that a program they were not working on contained a copy of the code
they were working on.

### What the move also removed

- **21 of `sshd`'s 155 suppressed panic sites**, deleted rather than audited.
  The whole hand-rolled bignum was inside the crate-wide
  `#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]`, and
  the shared implementation indexes through `get`/`get_mut` throughout and
  states a bound for each of the four places it does unchecked arithmetic.
- **A second transcription of the group-14 prime.** 512 hex digits, written out
  in both files, agreeing by luck: nothing compared them, and no test in either
  crate would have failed if a digit had differed. Both would still have
  produced a self-consistent shared secret and disagreed about it, surfacing as
  `MAC verification failed`. The worse case is quieter — an altered digit almost
  certainly yields a **composite** modulus, which is not a broken handshake but
  a *weak* one, and nothing anywhere would have said so.
  `two_generates_the_prime_order_subgroup_of_group14` now checks
  `2^((p-1)/2) ≡ 1 (mod p)`, which a mistyped prime would fail.
- **`sshd`'s private hex parser**, which built a two-character `String` per byte
  and read an invalid digit as zero.

### How it was found

Not by a test, and not by profiling either end. By counting `sshd`'s suppressed
panic-lint sites in order to audit them, noticing that a fifth of them were in
one section, opening that section, and recognising it as code the *client* had
deleted. The client's own comment — "this took 77 seconds" — was the whole
diagnosis; it was simply in the wrong file to be read by anyone looking at the
server.

### What did not move, and stays open

The timing leak. Both implementations are square-and-multiply with a
data-dependent branch per exponent bit, so the shared one is too; making one
copy out of two does not make it constant-time. It does mean the eventual fix —
X25519, per `TD-B-SSH-KEY-EXCHANGE-LEAKS-ITS-SECRET-THROUGH-TIMING` — now has a
single place to land instead of two, which is the same property this entry is
about, running the other way.
