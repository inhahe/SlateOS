## `A-FRAG-REJECTED-FRAGMENTS-STILL-CLAIM-A-REASSEMBLY-SLOT` — found 2026-08-22 (lane A) — FIXED 2026-08-22

**In short:** When a machine receives a large network message split into pieces
("fragments"), the kernel holds the pieces in a small table until the last one
arrives and it can glue them back together. That table has **eight** slots. A
piece that the kernel *rejects as invalid* still takes a slot and never gives it
back for 30 seconds — so **eight junk packets from anywhere on the network
occupy the whole table**, and every real split-up message arriving during those
30 seconds gets thrown away to make room. Roughly one junk packet every four
seconds keeps it that way forever. The attacker needs no credentials, no
handshake, and no connection: these are unsolicited packets. Fix is a few lines
— free the slot when the fragment that created it was rejected.

### Where

`kernel/src/net/frag.rs`:

| Item | Line | Note |
|---|---|---|
| `MAX_REASSEMBLY_ENTRIES` | 66 | `= 8` — the whole table |
| `add_fragment` (IPv4) | 293 | claims the slot at 354-357 |
| `FragEntry::add_fragment` | 155 | rejects at 157 / 164 |
| `add_fragment_v6` | 638 | same shape, `ENTRIES_V6` |
| `tick_expire` | 396 | the only thing that frees a stuck slot, after 30 s |

### The sequence

The module-level `add_fragment` marks the slot live **before** it knows whether
the fragment is any good:

```rust
let now = crate::hrtimer::now_ns();
entries[slot].active = true;          // <-- committed here
entries[slot].key = key;
entries[slot].created_ns = now;
...
let complete = entry.add_fragment(byte_offset, data, more_fragments);
```

and `FragEntry::add_fragment` returns `false` from the top on two paths, having
touched nothing at all:

```rust
if data.is_empty() { return false; }                 // empty payload
let end = byte_offset.saturating_add(data.len());
if end > MAX_PAYLOAD_V4 { return false; }            // offset+len past 65535
```

Nothing undoes the `active = true`. The slot now holds a key, a timestamp, an
empty buffer and an empty bitmap. It cannot complete — no data will ever match
it unless the attacker chooses to send more — so it sits until `tick_expire`
reaps it at the 30 s mark.

Because the key includes the 16-bit `identification` field, the attacker gets
65 536 distinct keys for free; eight of them fill the table. From then on the
allocator's fallback path runs on every legitimate fragment:

```
[frag] Evicting reassembly entry (id=...) to make room
```

which discards a real, partially-reassembled datagram. IPv6 is identical
(`FragKeyV6`, 32-bit ID, so 4 billion keys).

### Why the existing tests do not catch it

Two of the dead `#[test]`s point straight at these paths —
`test_oversized_fragment_rejected` and `test_empty_fragment_ignored` — and both
assert only `result.is_none()`, which is true. The leak is in state they never
look at. They also never ran (`A-KERNEL-UNIT-TESTS-NEVER-RUN`), so even that
much was hypothetical.

The `self_test()` at line 765 misses it for a different and more interesting
reason: it is documented as exercising `FragEntry`/`FragEntryV6` methods
"directly without touching global state or the timer". That is a reasonable way
to keep a boot self-test hermetic, but it means **the entire slot-allocation
layer — the table, the keying, the eviction policy — has no coverage at all**.
The bug lives exactly in the layer that was skipped.

### Fix

After the inner call, release a slot that holds nothing:

```rust
// A rejected fragment must not leave a live entry behind: the slot was
// claimed before we knew the fragment was any good, and an entry with an
// empty buffer can never complete.  Leaving it costs a reassembly slot for
// the full timeout, which is a remote DoS at eight packets.
if !complete && entry.buffer.is_empty() {
    entry.clear();
}
```

`entry.buffer.is_empty()` is an exact test for "contributed nothing": an
accepted fragment has non-empty `data`, so `end >= 1` and the buffer is resized
to at least 1 byte. It is therefore a no-op for an existing entry that is
legitimately still waiting. Apply to both `add_fragment` and `add_fragment_v6`.

Then give the allocation layer coverage, since that is the real gap — the seven
dead `#[test]`s in this module all drive the module-level `add_fragment`, which
is precisely the untested layer, so converting them is the fix for the coverage
hole as well as the vehicle for a regression test on this bug.

### Provenance

Found while converting this module's dead `#[test]`s to boot self-tests under
`A-KERNEL-UNIT-TESTS-NEVER-RUN`. Reading the two rejection tests closely enough
to port them is what exposed the slot leak they were sitting next to.

### Fixed 2026-08-22

Both `add_fragment` and `add_fragment_v6` now release a slot whose entry
contributed nothing:

```rust
if entry.buffer.is_empty() {
    entry.clear();
}
```

on the `else` (not-complete) arm. `buffer.is_empty()` is an exact test for
"contributed nothing" — an accepted fragment has non-empty `data`, so `end >= 1`
and the buffer is grown to at least one byte — which makes it a no-op for an
entry legitimately still waiting for more fragments.

The regression test drives the actual attack rather than asserting on internals:
`test_table_rejected_fragments_free_their_slot` pushes `MAX_REASSEMBLY_ENTRIES`
empty fragments with distinct ids, then the same number of oversized ones,
checking the table is empty after each sweep, and finally sends a real datagram
and requires it to reassemble. Before the fix that last step could only succeed
by evicting something.

Boot test: PASS, 1656 s, 11th consecutive clean boot, 6 WARNING / 3 FAIL
baseline unchanged. The serial log shows `[frag] table: rejected fragments free
their slot (8 empty + 8 oversized): OK` and **zero** occurrences of `[frag]
Evicting reassembly entry`, which is the observable the fix is about — the
sixteen junk fragments forced no eviction at all.
