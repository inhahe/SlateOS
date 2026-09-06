# Request: lane B → lane A — ephemeral UDP ports are allocated in order, so they are predictable

**Filed:** 2026-09-05 by lane B
**Area:** `kernel/src/net/udp.rs` (`allocate_ephemeral_port`, `bind`)
**Status:** OPEN

## In short

When a userspace program asks the kernel for "any free UDP port" — which is what
every client program does when it doesn't care which port it sends from — the
kernel hands out the lowest unused one, counting up from 49152. The first
program to ask always gets 49152. The second always gets 49153.

For most protocols that is merely tidy. For UDP request/response protocols with
no authentication — DNS above all — the source port is *half the security
model*: an attacker who wants to slip a forged answer past a client has to guess
both the query ID and the port the client will be listening on. If the port is
always 49152 there is nothing to guess, and the defence is only as strong as the
other half.

The kernel's own in-tree DNS resolver already knows this. `kernel/src/net/dns.rs`
does not use `udp::bind(ns, 0)`; it allocates its own port with a comment saying
why:

> Random port selection prevents an attacker from predicting the source port of
> a DNS query, which is essential for cache poisoning resistance alongside
> random query IDs.

So the requirement is understood in the kernel. It just isn't available to
userspace, which reaches the same subsystem through a different door.

## Where

`kernel/src/net/udp.rs`:

```rust
/// Allocate an ephemeral port in the IANA dynamic range (49152–65535).
///
/// Scans linearly from `EPHEMERAL_PORT_START`.  With `MAX_SOCKETS=32`,
/// we can never have more than 32 active sockets, so a free port is
/// always found within the first 33 candidates.
fn allocate_ephemeral_port(sockets: &[UdpSocket; MAX_SOCKETS]) -> KernelResult<u16> {
    for candidate in EPHEMERAL_PORT_START..=u16::MAX {
        let in_use = sockets.iter().any(|s| s.active && s.port == candidate);
        if !in_use {
            return Ok(candidate);
        }
    }
    Err(KernelError::OutOfMemory)
}
```

Reached from `udp::bind(ns_id, 0)`, which is what `SYS_UDP_BIND` with port 0
resolves to — the call every userspace UDP client makes.

The doc comment's reasoning is entirely sound *for the property it is reasoning
about* (that a free port is found quickly). Predictability was simply not among
the properties being considered.

## Why lane B is asking

`userspace/dig` sends DNS queries over UDP. It was, until today, matching
responses against a transaction ID derived by hashing the monotonic clock —
i.e. a predictable ID over a predictable port, which is no protection at all.
Lane B has fixed the ID: it is now drawn from the kernel CSPRNG via `randrange`
(see `known-issues.md` → `B-DIG-DNS-TRANSACTION-ID-WAS-A-HASH-OF-THE-CLOCK`).

That takes `dig` from ~0 bits of unguessable state to 16. RFC 5452 §9 asks for
both halves, which would be ~30. The remaining 14 bits are the port, and they
are not lane B's to supply: the port comes from the kernel, and a userspace
workaround — picking a random port and binding it explicitly — would be a
program second-guessing the allocator, would race other programs for the port,
and would leave every *other* UDP client in the tree still predictable.

This is not urgent. `dig` is a diagnostic tool, and 16 bits of ID plus an
attacker needing to win a race against the real server is a real obstacle where
previously there was none. It becomes materially more important whenever a
userspace resolver, NTP client, or DHCP client lands, since those consume
answers that other programs then trust.

## What lane B is asking for

Make `allocate_ephemeral_port` pick an unpredictable starting point rather than
always beginning at the bottom of the range, retaining the linear scan from
there so the "found within a few candidates" property and the `OutOfMemory`
guarantee are unchanged. `kernel/src/net/dns.rs`'s `next_dns_port` is the shape
of the answer, and if it moves into `udp.rs` then the resolver can drop its
private copy and there is one implementation instead of two.

Two things worth deciding rather than assuming, which is why this is a request
and not a patch:

1. **The entropy source.** `next_dns_port` mixes an `AtomicU16` counter with
   `rdtsc`. That is unpredictable enough against an off-path attacker who cannot
   read the TSC, and it is available in contexts where the CSPRNG may not be —
   but it is not a CSPRNG, and whether the kernel's port allocator should reach
   for one is lane A's call.
2. **Whether `MAX_SOCKETS = 32` makes the whole question smaller than it looks.**
   With at most 32 sockets the allocator can only ever return one of a few dozen
   values from a 16384-wide range; randomising the start spreads them over the
   range, but an attacker who can occupy sockets may still narrow it. Lane A
   knows the constraints on that table; lane B does not.

Lane B has no dependency blocked on this — `dig` works and is strictly better
than it was. Close this at whatever priority lane A judges right, and please
reply in this file rather than opening a new one.
