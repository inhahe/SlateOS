## TD-B-SSHD-SIGNS-AN-EXCHANGE-HASH-OVER-A-CLIENT-VERSION-THE-CLIENT-NEVER-SENT (lane B) — FIXED 2026-09-05

**Status:** FIXED — `userspace/sshd/src/main.rs`, commit on `lane-b` 2026-09-05.

**In short:** the SSH server could not talk to any SSH client at all — not
OpenSSH, not the `ssh` client in this same tree. Every connection died during
the handshake, before anyone could even type a password. The cause was one
line: when the server computed the fingerprint of the handshake that it signs to
prove who it is, it filled in a *made-up* value where the client's own
identification string belonged. The client, computing the same fingerprint from
what really happened, got a different answer, so the server's signature failed
to check out and the client hung up.

### What was wrong

`compute_exchange_hash` opened with:

```rust
// V_C: client version (we use a placeholder since we don't store it).
hash_input.extend_from_slice(&ssh_string(b"SSH-2.0-client"));
```

and the comment was accurate: `do_version_exchange` read the client's version
line, checked its prefix, wrote it to the debug log, and dropped it on the
floor. `ConnectionState` had nowhere to put it.

RFC 4253 §8 defines the exchange hash as

```
H = HASH(V_C || V_S || I_C || I_S || K_S || e || f || K)
```

where `V_C` is the client's identification string. `H` is not an internal
checksum — it is the single value the whole handshake turns on:

- the server **signs `H`** with its host key, and that signature is the only
  evidence the client has that it is talking to the machine whose key it
  trusts;
- the client **recomputes `H` independently** from its own view of the
  handshake and verifies the signature against its own copy;
- `H` of the first exchange becomes the **session ID**, which is in turn part of
  the publickey authentication signed blob (RFC 4252 §7).

So a `V_C` the client never sent does not degrade the handshake, it ends it.
Our client does exactly what it should — `verify_host_key_signature(&k_s, &h,
&sig_blob)?` runs before anything else, deliberately ahead of the `known_hosts`
prompt — and that check could never pass.

| End | `V_C` used to build `H` |
|---|---|
| `userspace/ssh` (and OpenSSH) | the real client identification string, e.g. `SSH-2.0-SlateOS_1.0` |
| `userspace/sshd` | the constant `SSH-2.0-client`, whatever the client sent |

Everything else in the construction already agreed: `I_C`/`I_S` are the full
KEXINIT payloads including the message byte on both sides, `K_S`, `e`, `f` and
`K` match, `V_S` matches (the client uses the server version it actually read),
and `derive_key`/`derive_keys` agree exactly — mpint `K`, then `H`, then the id
byte, then the session ID. `V_C` was the only divergence, and one was enough.

### Why nobody noticed

There is no test anywhere that makes the two ends agree. `userspace/ssh` and
`userspace/sshd` each have a thorough unit-test suite, and each suite tests its
own implementation against its own idea of the protocol. Both compute a hash;
neither ever compares its hash to the other's. A protocol is a contract between
two programs, and every test we had checked one signature of that contract
against itself.

The `Cargo.toml` of *both* crates already contains the argument that would have
caught this. sshd's says, of SHA-256:

> This crate carried its own SHA-256, as does the client in `userspace/ssh` —
> two implementations of one digest on the two ends of a connection, which is
> the arrangement where a divergence hides best.

That reasoning was applied to the digest and stopped there. `compute_exchange_hash`,
`derive_keys`, `ssh_string`, `encode_mpint`, the readers, `build_packet`,
`compute_mac` and AES-CTR are all still written twice, once per crate.

### How it was fixed

- `ConnectionState` gained `client_version: String`, with a doc comment saying
  why it is stored rather than logged.
- `do_version_exchange` keeps the line it already validated.
- `compute_exchange_hash` takes `client_version` as its first parameter and
  hashes it. The placeholder and its comment are gone, replaced by a note
  recording what the substitution cost.

Four tests. The load-bearing one,
`the_exchange_hash_is_the_one_rfc_4253_specifies`, writes the §8 construction
out longhand in the test and compares — deliberately *not* calling the
production function, because a test that asserts a function equals itself
catches nothing and the bug here was a missing input.
`the_clients_own_version_reaches_the_exchange_hash` asserts two different client
versions give two different digests, which is the property that was false.
`the_placeholder_version_is_no_longer_what_gets_hashed` pins the specific
fabricated string out of existence.
`a_fresh_connection_has_not_learned_a_client_version_yet` pins the field's
initial value, so a future refactor that computed the hash before the version
exchange would fail rather than hash an empty string.

**Verified:** 195 passing in `cargo test -p sshd`, clippy clean, rustfmt clean.

### What still needs doing

The fix is correct but the *structure* that produced it is untouched: the wire
and crypto layer is still implemented twice, and nothing forces the two copies
to agree. Filed as
`TD-B-THE-SSH-WIRE-LAYER-IS-WRITTEN-TWICE-AND-NOTHING-MAKES-THE-TWO-COPIES-AGREE`
below.
