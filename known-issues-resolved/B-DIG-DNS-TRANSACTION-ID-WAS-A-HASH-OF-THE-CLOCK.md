## B-DIG-DNS-TRANSACTION-ID-WAS-A-HASH-OF-THE-CLOCK (lane B) — FIXED 2026-09-05

**In short:** DNS over UDP has no handshake and no signature. When `dig` asks a
question, the only thing that lets it recognise the *server's* answer — rather
than one that some other host on the network chose to send — is a 16-bit
"transaction ID" it puts in the question and expects to see echoed back, plus
the port it asked from. `dig` was generating that ID by hashing the current
time. The time is not a secret; anyone in a position to send a forged answer is
in a position to know, to within a round trip, when the question went out. So
the check `dig` performed on every answer — "does this ID match the one I sent?"
— was a check the forger could pass, and `dig` would print the forged answer as
fact. The tool people reach for *because* they suspect DNS is lying to them
could itself be lied to.

### Where

`userspace/dig/src/main.rs`:

```rust
/// Simple non-cryptographic hash for generating transaction IDs.
fn make_txn_id() -> u16 {
    // Use the monotonic clock as a source of entropy.
    let t = clock_monotonic_us();
    let mut h: u32 = 5381;
    for b in t.to_le_bytes() {
        h = h.wrapping_mul(33).wrapping_add(u32::from(b));
    }
    (h & 0xFFFF) as u16
}
```

The result is compared against the response in `perform_query`, so the field was
load-bearing rather than decorative — the code took the defence seriously and
then built it on a public value.

RFC 5452 §9 is explicit that the ID must be unpredictable and drawn from a
cryptographically secure source. A hash is not encryption: djb2 over eight known
bytes is invertible by search in negligible time, and two IDs drawn microseconds
apart differ only in the low bits of a hash of two adjacent integers.

### The unit bug next door

`clock_monotonic_us` returned the syscall's value unconverted, and
`SYS_CLOCK_MONOTONIC` returns nanoseconds — the comment on the constant three
lines above the wrapper *said* "boot-relative nanoseconds" while the wrapper
below it said `_us`. This was the third instance found in one sitting
(`B-SSHD-LOGIN-GRACE-TIMER-…`, `B-INETD-RATE-LIMIT-WINDOW-…`). Here it was
arithmetically harmless, because the only consumer hashed the value — which is
its own kind of warning: the wrong unit was invisible because nothing downstream
cared what the number meant, and nothing downstream caring what the number meant
is exactly why it was the wrong number to use.

### The fix

`make_txn_id` draws two bytes from `randrange::fill_secret` and returns
`Result<u16, DigError>`. There is no fallback: `randrange`'s documentation is
explicit that a caller must never substitute a guessable value for a secret, and
a predictable transaction ID is that. `clock_monotonic_us` and the now-unused
`SYS_CLOCK_MONOTONIC` constant are deleted, since the txn ID was their only
consumer.

Failing closed is the point, and it is what the regression test asserts:
`a_transaction_id_is_drawn_or_refused_and_never_guessed` probes the CSPRNG and
then requires that an ID exists **if and only if** entropy did. The old code
could not fail — it always produced a number — which is why the defect was
invisible: nothing looked wrong at the call site or in the output.

Note this changes nothing about `dig` on a host build. `randrange` declines on
non-Unix hosts by design, so `dig` there now refuses to send a query — but on a
host build every network syscall in `dig` is already a stub returning `-ENOSYS`,
so no query could have been sent regardless.

### Not fixed here: the source port

RFC 5452's other half is source-port randomisation, and `dig` gets its port from
`SYS_UDP_BIND`'s ephemeral allocation, i.e. from the kernel. Whether that
allocation is randomised was checked, and it is not:
`udp::allocate_ephemeral_port` scans linearly from 49152, so the first socket a
process opens always gets exactly that port. The transaction ID is therefore
doing all the work alone -- 16 bits where RFC 5452 asks for about 30. The
kernel's own in-tree resolver (`kernel/src/net/dns.rs`) randomises its port and
says in a comment why, so the requirement is understood there; it is simply not
reachable through the userspace socket API. Not lane B's to fix, and filed as
`requests/b-a-ephemeral-udp-ports-are-predictable.md`.
