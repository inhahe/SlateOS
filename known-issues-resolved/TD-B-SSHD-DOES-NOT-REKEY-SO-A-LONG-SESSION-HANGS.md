## TD-B-SSHD-DOES-NOT-REKEY-SO-A-LONG-SESSION-HANGS (lane B) — FIXED 2026-09-05

**Status:** FIXED. Found 2026-09-05 while fixing the global-request entry above;
all five numbered steps below are done, on `lane-b`. The code lives in
`userspace/sshd/src/lib.rs` and `userspace/ssh/src/lib.rs` — the pointers to
`main.rs` further down are from when this was written and are wrong; `sshd`'s
`main.rs` is a sixteen-line shim over the library.

**In short:** SSH periodically renegotiates its encryption keys, on a timer and
by volume of data. Our server never does, and — worse — ignores the client when
*it* asks. An OpenSSH client stops sending anything else until the renegotiation
it started finishes, so a session that lasts an hour or moves a gigabyte stops
dead, with no error, forever.

### Where

`userspace/sshd/src/main.rs`, `dispatch_channel_message`: the
`msg::SSH_MSG_KEXINIT` arm logs and returns. There is no rekey path anywhere in
the file — grepping for `rekey` finds nothing.

### Why it matters

Rekeying is not optional politeness. RFC 4253 §9 recommends it after one hour or
one gigabyte, and OpenSSH implements both (`RekeyLimit`, default `default none`,
which still means the *time*-based rekey at 1 h is off but the cipher-specific
data limit is not — for AES-CTR that limit is large, but the client also rekeys
on its own schedule in several builds and distributions patch it).

The failure mode is a hang rather than an error, which is the expensive part. An
OpenSSH client that sends `KEXINIT` will not send further channel data until the
exchange completes; it holds the session open and silent. To the user, a working
session simply stops responding mid-command, with no diagnostic on either end
unless the server is in debug mode.

### How to reproduce

Connect with `ssh -o RekeyLimit=1M` and move more than a megabyte through the
session (`cat` a large file). The session stalls at the limit.

### What the proper fix looks like

Implement server-side rekeying. Concretely:

1. Factor the existing handshake so the KEX exchange (`KEXINIT` → `KEX_DH_INIT`
   → `KEX_DH_REPLY` → `NEWKEYS`) can run against an already-encrypted
   connection, not only against a fresh one. Today it is inline in the
   connection setup path.
2. Preserve the session identifier from the *first* exchange: RFC 4253 §7.2 says
   the exchange hash `H` of the first KEX remains the session ID forever, and
   only the keys are re-derived.
3. Swap the cipher and MAC state at exactly the right packet boundary — new keys
   take effect on the packet *after* `NEWKEYS` in each direction independently.
   Getting this wrong desynchronises the keystream and turns every subsequent
   packet into a MAC failure.
4. Queue or drop non-KEX traffic while an exchange is in flight, per §7.1.
5. Initiate our own rekey on the RFC's thresholds so the server is not relying on
   the client to do it.

The sequence numbers do **not** reset across a rekey, which the existing
`recv_seq`/`send_seq` handling already gets right.

### What was actually done

All five, in two commits.

Steps 1–4 (`userspace/sshd/src/lib.rs`): the handshake was split into
`run_key_exchange`, which runs against an already-encrypted connection as
happily as a fresh one; `do_rekey` is the entry the `SSH_MSG_KEXINIT` dispatch
arm now calls instead of logging. The session id is kept with `get_or_insert`,
so only the first exchange sets it (§7.2). `PacketCodec::activate` swaps each
direction's cipher and MAC independently at the packet after its own `NEWKEYS`,
and deliberately does *not* reset the sequence numbers.

A detail that was not in the plan above and that §11.2 requires: a key exchange
must tolerate `SSH_MSG_IGNORE` and `SSH_MSG_DEBUG` arriving in the middle of it,
and report `SSH_MSG_DISCONNECT` as a disconnection rather than as "expected
`KEX_DH_REPLY`". OpenSSH sends `IGNORE` at arbitrary points under
`ObscureKeystrokeTiming`, so without this the fix would have failed against the
most common client for a reason it would have blamed on that client. Both ends
grew a `recv_during_kex` for this.

Step 5 (the same file, plus `userspace/ssh/src/lib.rs`): `rekey_is_due` is
checked at the top of `handle_channels`, against a gibibyte of traffic under the
current keys (`PacketCodec::bytes_under_current_keys`, the larger of the two
directions — they have separate keys) or an hour since they were installed
(`keys_installed_at`, set in `run_key_exchange` where a key's life begins).
`start_rekey` sends our `KEXINIT` and then has to buffer the client's in-flight
channel data for one round trip, because §7.1 gags a peer once *it* has sent
`KEXINIT`, not once it has received one; that buffer is bounded at 64 packets
and a client exceeding it is disconnected rather than silently truncated.

Shipping step 5 alone would have *introduced* this same hang in the other
direction, against our own client: the `ssh` client had no arm for an
unsolicited `KEXINIT` either. It has one now (`SshSession::do_rekey`), in the
same change. The client needs no in-flight buffer — anything the server sent
before its `KEXINIT` is earlier in the byte stream and already dispatched.

Both directions are exercised end to end in `userspace/ssh-interop`
(`the_client_can_rekey_an_established_session_and_the_daemon_serves_it` and
`the_daemon_can_rekey_an_established_session_and_the_client_serves_it`), each
running two rekeys rather than one: the second travels under the first's keys,
so a rekey that derived different keys at the two ends cannot be read at all
instead of passing quietly and failing later as a MAC error on unrelated
traffic.
