## Unauthenticated peers could grow the in-kernel SSH receive buffer without bound (lane A)

**Status:** FIXED 2026-08-23 — boot-tested on `lane-a`.

**In short:** the SSH server runs inside the kernel. Its packet reader could
not tell "I have not received the whole packet yet" from "these bytes can
never *be* a packet", and reported both as "wait for more". So a peer that
sent four bytes of garbage length and then streamed filler was never
disconnected, and its data piled up in kernel memory forever. Enough of it
exhausts the kernel heap and takes the whole machine down — not just that
connection, and without ever logging in.

### What was wrong

`try_read_packet_plain` returned `Option<Vec<u8>>`:

```rust
if packet_length > MAX_PACKET_SIZE || packet_length < 2 {
    return None; // Invalid.
}
let total = 4 + packet_length;
if recv_buf.len() < total {
    return None; // Incomplete.
}
```

Two different situations, one return value. The caller is a loop:

```rust
while let Some(payload) = try_read_packet_plain(&mut session.recv_buf) {
```

`None` ends the loop, and the buffer is only ever drained by a *successful*
parse (`*recv_buf = recv_buf.split_off(total)`). So an unframeable length
field parks the bytes permanently: every poll re-reads the same four bytes,
re-rejects them, and returns to a caller that appends still more.

Nothing capped the accumulation either — `session.recv_buf.extend_from_slice(&data)`
ran unconditionally on every poll.

### Why it survived review

Three things hid it:

- **It produces no lint.** It is not an index, not an unwrap, not an
  arithmetic op. A clippy sweep of this file walks straight past it. This is
  the concrete vindication of the note above: the file was on the sweep list
  with 104 sites, and *none of them was this bug*.
- **The correct shape was already in the file, twelve lines below.**
  `try_read_packet_encrypted` returns `KernelResult<Option<Vec<u8>>>` and does
  return `Err` for a bad length. The two functions are read as a pair and
  look alike; only the return types differ, which is exactly the sort of
  difference the eye completes rather than notices.
- **The comments were correct and still misleading.** `return None; // Invalid.`
  and `return None; // Incomplete.` each accurately describe their own line.
  Nothing in the function says those two must be *distinguishable*, because
  the type had already decided they were not.

### The fix

`try_read_packet_plain` now returns `KernelResult<Option<Vec<u8>>>`, matching
its encrypted twin: `Ok(Some)` consumed a packet, `Ok(None)` is a legal
prefix, `Err` is unframeable and closes the session. The caller became a
three-arm `loop`/`match`. Also tightened while there: `packet_length` must be
at least `MIN_PADDING + 1`, and `padding_length` must meet the RFC 4253
minimum of four — neither was checked, though our own builder emits both.

Separately, a backstop at the append site drops any session whose buffer
passes `MAX_RECV_BUF` (`4 + MAX_PACKET_SIZE + POLY1305_TAG_LEN`, the largest
thing that can legitimately be in flight). A `const _: () = assert!(...)`
pins that it stays above `MAX_PACKET_SIZE`, so a later edit to either
constant fails the build rather than silently truncating legal traffic.
`Session::reset` does `*self = Self::new()`, so the buffer is genuinely
freed on close — checked, because a cap that only defers the exhaustion
would be worse than none.

Self-test `1b` covers what was broken rather than what was written: legal
prefixes of 0/1/4/5/8 bytes must be *held* and not consumed, oversize and
undersize lengths and out-of-range padding must all be `Err`.

### What this says about the remaining sweep

The generalisation in the entry above was right about *where* to look and
understated *what* to look for. "A header field used as an offset without a
lower bound" was the TCP shape. The SSH shape is a level up: **a parser whose
return type cannot express the difference between a recoverable and an
unrecoverable failure.** Both are invisible to lints, and both are found by
reading the framing layer and asking what a peer gains by lying.

For `net/tls.rs` and `net/firewall.rs`, the questions to carry over:

1. Can a length or offset field be rejected in a way that leaves the bytes
   buffered and the connection open?
2. Is there any accumulating buffer fed from the wire with no absolute cap?
3. Where two sibling parsers exist (plain/encrypted, v4/v6), do their return
   types agree? A divergence is where the weaker one is wrong.
