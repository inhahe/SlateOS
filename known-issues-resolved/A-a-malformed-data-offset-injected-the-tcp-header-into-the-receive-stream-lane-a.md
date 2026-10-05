## A malformed Data Offset injected the TCP header into the receive stream (lane A)

**Status:** FIXED 2026-08-22 — `d5ca795fc`, boot-tested on `lane-a`.

Found by reading, during the `net/tcp.rs` lint sweep, not by a lint: clippy
flagged `data[12]` as `indexing_slicing` and the *value* it read turned out to
be the actual bug. Worth recording because the sweep's stated purpose is
removing panics, and the most serious thing it surfaced was not a panic.

**What was wrong.** `process_tcp_common` took the segment's Data Offset field
and used it as the payload's start offset without checking it:

```rust
let data_offset = ((data[12] >> 4) as usize) * 4;
let payload = if data_offset < data.len() { &data[data_offset..] } else { &[] };
```

Data Offset is four bits. A peer may send 0. When it did, `payload` was
`&data[0..]` — the whole segment, starting with its own 20-byte TCP header —
and that was appended to `rx_buffer` and returned to the application as
ordinary stream data. Twenty bytes of attacker-chosen content (ports,
sequence numbers, flags, window) injected into a stream, from one segment,
requiring only an established connection. Values 1..4 inject proportionally
less. RFC 793 §3.1 sets the minimum at 5 words; nothing enforced it.

The mirror case was also wrong but harmless: Data Offset past the end of the
segment claims options that were never transmitted, and was silently treated
as "no options", so a 20-byte segment could pose as an option-bearing one.
Linux drops both in `tcp_v4_rcv`
(`th->doff < sizeof(struct tcphdr) / 4 || skb->len < th->doff * 4`).

**Why it lasted.** The check had nowhere to live. The header parse was
sixteen index expressions at the top of an 800-line function, so the only way
to exercise the kernel's first contact with peer-chosen bytes was to stand up
a connection and inject a frame — which no test did. The fix is therefore a
split as much as a check: `parse_tcp_header` returns a `TcpHeaderView` or
`None`, holds options and payload as *slices* rather than the offsets used to
cut them, and destructures the fixed header in a single refutable slice
pattern so the length check and the field reads are one operation. It has a
direct self-test pinning doff 0–4, doff past the end, short segments, the
bare 20-byte header, and the doff 5 vs 6 split of one 24-byte segment.

**Generalisation worth acting on.** Every other wire parser in the tree reads
a length or offset field out of the header and slices with it. The three on
the remaining sweep list — `net/ssh.rs`, `net/tls.rs`, `net/firewall.rs` —
should be read for this specific shape (a header field used as an offset
without a lower bound), not merely swept for `indexing_slicing`. A `.get()`
conversion silences the lint on such a site while leaving the injection
intact, which is the failure mode to avoid: the panic is the loud symptom,
the unchecked offset is the bug.

**Sweep status for `net/tcp.rs`:** 182 → 110 → 56 distinct sites, all now
`indexing_slicing`; all 22 blanket `arithmetic_side_effects` allows removed
(`953f828c0`), of which only seven functions had any arithmetic at all.
