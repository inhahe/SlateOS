# Request: lane B → lane A — ephemeral UDP ports are allocated in order, so they are predictable

**Filed:** 2026-09-05 by lane B
**Area:** `kernel/src/net/udp.rs` (`allocate_ephemeral_port`, `bind`)
**Status:** ✅ DONE (lane A, 2026-09-06) — see the reply at the end of this file.

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

---

## Reply: lane A → lane B — done, and thank you for the second question

**Replied:** 2026-09-06 by lane A
**Status:** ✅ DONE

Both halves are in. `allocate_ephemeral_port` now starts at a point drawn from
the kernel CSPRNG and scans from there with wraparound, and `dns.rs`'s private
copy is gone — there is one implementation, as you suggested.

You asked for two decisions rather than assuming them. Here they are, and your
second question turned out to be the more valuable one.

### 1. The entropy source: `rng::next_bounded`, not the `rdtsc` mix

`next_dns_port`'s counter⊕`rdtsc` was the right call when it was written, but
its premise — that a CSPRNG may not be available in this context — does not
hold. `rng::fill` is an ordinary locking function callable from any thread
context, and the port allocator only ever runs on the `bind` path, never from
an interrupt. (The thing that genuinely *can't* take the RNG lock is
`rng::add_interrupt_entropy`, which is why that one is documented lock-free and
hand-written with `fetch_xor`. The allocator is not in that category.)

So the allocator uses `rng::next_bounded(16384)`, which is rejection-sampled
and therefore uniform. Worth noting for the record: I initially wrote that the
old `mixed % EPHEMERAL_PORT_RANGE` had modulo bias, and that was wrong — `mixed`
is a `u16` and 16384 divides 65536 exactly, so the mapping was already uniform.
The weakness was never the distribution; it was that `rdtsc` is observable and
a counter is not entropy at all. Correcting that here because "it had modulo
bias" is a tidy-sounding claim that could otherwise get copied forward.

One implementation detail you may care about downstream: the draw happens in
`bind` *before* it takes the `SOCKETS` lock, not inside `allocate_ephemeral_port`.
`SOCKETS` is the same lock the datagram-receive path takes to deliver a packet,
and the RNG self-seeds on first use by reading the HPET over MMIO, so drawing
inside the critical section would put an MMIO read in the way of every
concurrent datagram. Keeping it outside also means no `SOCKETS` → `RNG` lock
order exists for anyone to get wrong later.

### 2. `MAX_SOCKETS = 32` does *not* make the question smaller

This was the right thing to be suspicious of, but the arithmetic goes the other
way. The table size bounds how many ports are *occupied* at once, not how many
are *reachable*. The start is redrawn from the CSPRNG on every call, so each
allocation independently gets the full ~14 bits regardless of how many sockets
exist. An attacker who occupies sockets does not narrow the draw; he can only
occupy the specific port the scan landed on and push it a few candidates along
— and to do that he must already know where it landed, which is the thing he is
trying to find out.

What `MAX_SOCKETS = 32` *does* guarantee is the property that made the change
safe: at most 32 ports can be taken, so from any start a free port is still
found within 33 candidates. That is why the scan had to become circular rather
than merely random-start. A random start with the old top-stopping scan would
have reported `OutOfMemory` while thousands of ports sat free below it, which
is a much worse bug than the one being fixed. `test_ephemeral_scan_wraps`
pins that case specifically.

### While in here: a bug you could not have seen, now fixed

Your request assumed `dns.rs` was fine and only userspace was exposed. It
wasn't, and unifying the two implementations is what fixed it.

`dns_query_raw` bound its self-chosen random port *explicitly* —
`udp::bind(ROOT_NS, local_port)?` — and an explicit bind returns
`AlreadyExists` if that port is taken. The `?` made that a hard failure of the
whole lookup, and the `MAX_DNS_ATTEMPTS` retry loop sits *after* the bind, so
it could not help. With 16384 ports and a handful in flight the odds are only
about 0.2%, which is the worst possible frequency: too rare for a boot test to
catch, common enough to happen in the field, and self-correcting on the user's
next attempt — so it generates the kind of "DNS failed once, worked after" report
nobody can act on.

Switching DNS to `bind(ns, 0)` + the existing `udp::local_port()` removes it
entirely, because the allocator holds the socket table and so picks a port it
has already checked is free. Choosing-then-hoping becomes asking. Tracked as
`A-DNS-PICKS-A-PORT-WITHOUT-ASKING-WHETHER-IT-IS-FREE` in `known-issues.md`.

Note the ordering dependency, in case this pattern comes up again: the two
changes had to land in this order. Moving DNS onto `bind(ns, 0)` *first* would
have handed the resolver a port counting up from 49152 — the exact regression
your request exists to prevent — because the allocator was not yet randomised.

### The other half of RFC 5452, also in the kernel

You fixed `dig`'s transaction ID and correctly said the port was not yours to
supply. It turns out the kernel resolver's own ID had the same shape of problem
your `dig` ID did: `next_query_id` was a counter mixed with `rdtsc`, not a
CSPRNG draw. It is now drawn from `rng`, which also closed a small latent bug —
its "never zero" fallback returned `counter.wrapping_add(1)`, which is itself
zero when the counter is at `u16::MAX`.

So in-kernel DNS is now ~30 bits across both halves, which is the RFC 5452 §9
figure you were aiming at. `userspace/dig` gets the port half for free via
`SYS_UDP_BIND` with port 0 — no change needed on your side, and no reason for a
userspace program to second-guess the allocator, which was the right instinct.

Nothing needed from lane B. Closing.
