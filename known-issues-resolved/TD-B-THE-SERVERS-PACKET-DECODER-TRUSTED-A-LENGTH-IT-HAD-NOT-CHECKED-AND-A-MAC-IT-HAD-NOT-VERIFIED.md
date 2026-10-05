### TD-B-THE-SERVERS-PACKET-DECODER-TRUSTED-A-LENGTH-IT-HAD-NOT-CHECKED-AND-A-MAC-IT-HAD-NOT-VERIFIED

**Status: FIXED** 2026-09-05 (`userspace/sshwire/src/lib.rs`, `PacketCodec`;
`userspace/sshd/src/main.rs`, `try_parse_packet` deleted). The fifth and sixth
bugs found by reading the client's and the server's copies of the same function
side by side, and — like the four before them — neither was found by a test.

**In short:** the SSH server's routine for taking apart an arriving packet had
two broken checks. One read a byte from *outside* the packet if the sender
declared an impossibly small one. The other, on being handed a too-short
message-authentication code (the tag that proves the packet was not tampered
with), skipped the check entirely and accepted the packet as genuine. Both were
reachable by anyone who could open a TCP connection to the daemon, before
logging in. Neither could actually be triggered through the code path that
existed, because the buffering around them happened to make the bad inputs
unreachable — which is precisely why nobody noticed the checks were wrong.

#### The two faults

**1. No floor on the declared packet length.** RFC 4253 §6 lays a packet out as
`[u32 packet_length][u8 padding_length][payload][padding]`, where
`packet_length` counts everything after itself — so the smallest legal value is
5 (one padding-length byte plus the four bytes of padding §6 requires as a
minimum). The client's decoder rejected anything below that. The server's
checked only the ceiling:

```rust
if packet_length > MAX_PACKET_SIZE { ... }   // sshd: no floor
```

A peer declaring 0 or 1 therefore produced a four- or five-byte packet, and the
`padding_length` byte at offset 4 — which §6 puts *inside* the packet — was read
from past the end of it. `userspace/sshd/src/main.rs` carries a crate-level
`#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]`, so the
lint that exists precisely to catch this shape is switched off across the whole
file; what it reaches is a panic, i.e. a pre-auth remote kill of the connection
handler. Removing that blanket allow is tracked separately.

**2. A fail-open MAC comparison.** The verification read:

```rust
if mac_data.len() >= mac_len && !constant_time_eq(&mac_data[..mac_len], &expected) {
    return Err(...);
}
```

The `&&` means: *if the MAC is too short, do not compare it* — and then fall
through to accept the packet. Authentication is not optional; the honest form
of "I do not have enough bytes to check this" is a refusal, not a pass. As
written, a peer that has authenticated nothing to anybody could have had a
packet accepted unauthenticated by sending a truncated tag.

#### Why neither fired

The same structural reason as the other four, in a slightly different key.
There, two copies of a function had *drifted* and no test compared them. Here,
one copy was simply wrong, and its only caller happened to make the wrong input
unreachable: sshd's `StreamBuffer` never handed `try_parse_packet` a buffer it
had not already sized, so the short-MAC branch had no way to be entered from
inside this program. The guard was wrong and the caller was what made it
harmless — an arrangement that survives exactly until someone refactors the
caller, and one that no test of *this* file could ever have flagged, because
from inside the file the behaviour is correct.

That is the argument for extraction stated from the other side. Sharing a
function does not only stop two copies drifting; it puts the copy that is wrong
somewhere its own callers cannot keep covering for it.

#### The fix

Both faults are gone by construction, because there is now one decoder:

```rust
if !(MIN_PACKET_LENGTH..=MAX_PACKET_SIZE).contains(&packet_length) {
    return Err(WireError::PacketLength { len: packet_length });
}
...
let received = mac_data.get(..mac_len).ok_or(WireError::Truncated {
    what: "MAC", needed: mac_len, available: mac_data.len(),
})?;
if !constant_time_eq(received, &expected) {
    return Err(WireError::MacMismatch);
}
```

`MacMismatch` deliberately carries no data: an error that reported *where* the
tags differed, or what was expected, would be an oracle handed to the peer who
supplied the packet.

Tests in `sshwire`: `a_length_outside_the_permitted_range_is_refused`,
`padding_longer_than_the_packet_is_refused`,
`a_short_mac_is_rejected_rather_than_skipped`, `a_modified_packet_is_refused`,
`a_packet_replayed_out_of_order_is_refused`.

**One thing worth recording about testing this layer.** `a_modified_packet_is_refused`
cannot simply flip a byte and expect `MacMismatch`, because SSH encrypts the
length field but does not authenticate it *ahead of the packet it introduces* —
the MAC covers the whole plaintext packet, so the length has to be trusted
before there is anything to check it against. Corrupting a length byte therefore
yields a range error or a wait-for-more-bytes, never a MAC failure. The test
states all three outcomes by position rather than pretending the first one is
the only one, and `a_packet_replayed_out_of_order_is_refused` asserts only
`is_err()` for the same reason: by the time a packet is replayed the receiving
counter has moved on, so its length field decrypts to noise and the packet is
refused before its MAC is ever reached. Both are still refusals; neither is the
refusal a naive test would have asserted.
