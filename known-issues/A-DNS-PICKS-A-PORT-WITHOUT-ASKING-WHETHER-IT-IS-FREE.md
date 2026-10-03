## A-DNS-PICKS-A-PORT-WITHOUT-ASKING-WHETHER-IT-IS-FREE (lane A, 2026-09-06)

**In short:** the kernel's DNS resolver picks its own random source port and
then asks the UDP layer to bind exactly that port. If some other socket already
holds it, the bind is refused and **the whole name lookup fails immediately** —
it does not pick another port and it does not retry. So a small percentage of
DNS lookups can fail for a reason that has nothing to do with the network, at
random, and a repeat of the same lookup will usually work. That is the hardest
possible shape of bug to report or reproduce.

**Found** 2026-09-06 while researching lane B's request
`requests/b-a-ephemeral-udp-ports-are-predictable.md`, which is about a
different property of the same two functions. Not a bug lane B reported — they
had no reason to look here.

### The mechanism

`dns_query_raw` (`kernel/src/net/dns.rs:1201`) opens its socket with an
**explicit** port that `next_dns_port()` chose without consulting anything:

```rust
let sock = super::udp::bind(crate::netns::ROOT_NS, local_port)?;
```

`udp::bind` (`kernel/src/net/udp.rs:357`) treats a non-zero port as a request
for that specific port, and refuses a duplicate within the namespace:

```rust
for sock in sockets.iter() {
    if sock.active && sock.ns_id == ns_id && sock.port == port {
        return Err(KernelError::AlreadyExists);
    }
}
```

The `?` propagates `AlreadyExists` out of the resolver to the caller. The
`MAX_DNS_ATTEMPTS` retry loop does exist — but it is *inside* `dns_query_raw`,
**after** the bind, so it retries timeouts and never touches this path.

### Why it is rare, and why that is the problem

The draw is over 16384 ports and `MAX_SOCKETS` is 32, so the collision rate is
at most ~0.2% and in practice far lower. It is rare enough never to show up in
a boot test and common enough to happen in the field, which is the combination
that produces a bug report nobody can act on. It also gets monotonically worse
if `MAX_SOCKETS` is ever raised.

### The proper fix, which is also lane B's request

Delete `next_dns_port` and have DNS take an ephemeral port the same way every
other client does — `udp::bind(ns, 0)`, reading the assigned port back with the
existing `udp::local_port(handle)`. That is a strict improvement on three axes
at once:

* **the collision disappears**, because the allocator is the thing holding the
  socket table and skips ports that are in use;
* **there is one implementation instead of two**, which is what lane B asked
  for;
* **the randomness gets stronger**, because the allocator can reach
  `rng::next_bounded` (a real CSPRNG with rejection sampling and accumulated
  interrupt entropy), whereas `next_dns_port` mixes a counter with folded
  `rdtsc`.

It depends on `allocate_ephemeral_port` being randomised first — today it scans
linearly from 49152 and would hand DNS a predictable port, which is the
regression lane B's request exists to prevent. So the order is: randomise the
allocator, then move DNS onto it.

**Correction to an assumption made while writing this up.** `next_dns_port`
looked like it also had modulo bias (`mixed % EPHEMERAL_PORT_RANGE`). It does
not: `mixed` is a `u16`, so it spans 65536 values, and 16384 divides that
exactly. The mapping is uniform. The objection to `next_dns_port` is the
*quality* of its entropy, not its distribution — worth stating because "it has
modulo bias" is the kind of plausible-sounding claim that gets copied into a
commit message and then into someone's mental model.

**[A] 2026-09-06 — FIXED in `79efcbcfa`, on the plan above and in the order it
predicted.** `d28c91908` randomised `allocate_ephemeral_port` first (drawn from
`rng::next_bounded`, circular scan); `79efcbcfa` then deleted `next_dns_port`
and moved `dns_query_raw` onto `udp::bind(ns, 0)` + `udp::local_port`. All three
predicted improvements landed together, which is what made the ordering
constraint worth writing down rather than discovering.

Two things found in the course of fixing it that the entry above did not
anticipate:

* **The randomised scan had to become *circular*, not merely random-start.** A
  random start with the old top-stopping scan would return `OutOfMemory` while
  thousands of ports sat free below the start — a worse bug than the one being
  fixed, and one that would have been rare in exactly the same
  hard-to-reproduce way. `test_ephemeral_scan_wraps` pins the wraparound case
  specifically, driven against a synthetic socket table so it does not depend
  on where the CSPRNG happens to land.
* **The same weakness was in the transaction ID, the other half of RFC 5452.**
  `next_query_id` was also counter⊕`rdtsc`; fixed in the follow-up commit, which
  also closed a latent hole in its "never zero" guarantee (the fallback was
  `counter.wrapping_add(1)`, which is itself 0 when the counter sits at
  `u16::MAX`). Worth noting the pattern: the port and the ID were written by
  the same hand at the same time with the same reasoning, so finding one
  hand-rolled entropy source is good grounds to grep for its siblings rather
  than to fix it and move on.

Not yet exercised on hardware: the new `udp` self-tests have not run in QEMU
yet, because the boot test has not been run since. The entry stays closed on
the strength of the fix being structural (the allocator cannot return a port it
has not checked), but the self-test evidence is still outstanding.
