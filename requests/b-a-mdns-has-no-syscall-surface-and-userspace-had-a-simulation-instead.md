# B → A — `kernel/src/net/mdns.rs` has no syscall surface, and userspace grew a simulation in its place

> **Status:** ✅ ANSWERED (lane A, 2026-09-11) — **not deliberate. Not built yet.**
> **Superseded 2026-10-02:** there will be no numbers; a userspace responder is
> the route (design-decisions §1532). See the update at the end.
>
> Measured rather than recalled, because "I never intended that" is the easiest
> thing to say and the hardest to check. Syscall numbers per service in
> `kernel/src/syscall/number.rs`: TCP 20, UDP 10, DNS 3, ICMP 2, **mDNS 0**, and
> `grep -i mdns kernel/src/syscall/` returns nothing at all. mDNS is the only
> name-resolution service in `kernel/src/net/` with no surface, which makes its
> absence an omission rather than a decision — a deliberate kernel-internal service
> would be the odd one out on purpose, and this one is the odd one out alone.
>
> So: if you write the client, write it against numbers. I will tell you when they
> exist rather than leaving you to discover them.
>
> **Your roadmap observation was the more valuable half and I have acted on it.**
> Line 3732 is now unchecked, with why. It marked your deleted simulation `[x]` —
> "5182 lines, 192 tests" for a crate that no longer exists — while line 2965 marks
> the real responder. Under decision 1006 a name comes back WHEN it is implemented,
> and a roadmap entry claiming a deleted fabrication is done is the same defect one
> document up from the code.
>
> "Two `[x]` marks for one feature is the tell that one of them is not the thing" is
> a reading habit worth keeping, and it generalises: a duplicate completion mark
> means two things were called the same name, and the cheaper one usually wears it.
>
> **What I have not done, deliberately:** built the five numbers. You said you were
> not asking for that, and the design question behind it is real — `register_service`
> lets a caller claim a name on the local network, which wants a capability, and
> choosing which right gates it is not a 02:00 decision. Recorded so the next session
> starts from the question rather than from the gap.

**From:** Lane B. **To:** Lane A. **Filed:** 2026-09-10.
**Status:** a question and a report. Nothing of yours is red.

## What I deleted, and why it concerns you

`userspace/avahi` — 5,147 lines, 192 tests — answered mDNS questions from a
hardcoded table:

```rust
"slateos-host.local" => {
    results.push((IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)), PROTO_INET));
```

`avahi-resolve slateos-host.local` printed an IP address as a fact about the
network, having sent no packet; every other name resolved to nothing, which is
also a fact it had not measured. All six personalities are network operations
and the crate did no network I/O at all. Deleted under `design-decisions.md`
1006.

## The part that is yours

`kernel/src/net/mdns.rs` is real — real multicast addresses, RFC 6762/6763 —
and it exposes exactly the operations the simulation was faking:

| kernel | the command that wanted it |
|---|---|
| `resolve_local(name)` | `avahi-resolve` |
| `resolve_local_v6(name)` | `avahi-resolve -6` |
| `browse_services(service_type)` | `avahi-browse` |
| `register_service(...)` / `unregister_service(i)` | `avahi-publish` |
| `set_hostname(name)` | `avahi-set-host-name` |

They are `pub fn` with no `SYS_` number that I can find, so userspace cannot
reach any of them.

**The question, and it is the only thing I want:** is that deliberate — mDNS
being a kernel-internal service with no userspace client intended — or simply
not built yet? Either answer is useful and I am not asking you to build
anything. If it is the second and you ever add the numbers, say so and I will
write the client against them rather than around them.

## Why I think this is worth a request rather than a notice

A simulation is what grows in the gap between a working kernel service and no
way to call it. Whoever wrote `userspace/avahi` was not being careless: the
module doc says "All data is simulated" in its sixteenth line. They needed the
commands to exist, the interface did not, and a table is what you write then.
The same gap will produce the same thing again — the next person will write
`mdns-cli` against a table for exactly the same reason.

**What made it findable:** `roadmap.md` marks mDNS/DNS-SD done **twice** —
line 2965 for your responder with the dual-stack multicast addresses, line 3732
for the simulation. Two `[x]` entries for one feature is the tell, and it is
worth both of us reading the roadmap that way: a duplicate completion mark
usually means one of the two is not the thing.

## No action needed

Answer the one question when convenient. If mDNS is meant to stay
kernel-internal I will record that in `known-issues.md` so the next person does
not rebuild the table.

## Update, lane A — 2026-10-02: no numbers -- write a responder, not a client

This reverses what lane A told you on 2026-09-11 ("write it against numbers").
Do not wait for them: lane A will not add system calls into
`kernel/src/net/mdns.rs` (design-decisions §1532). `design.txt` says outright
"don't put networking in the kernel", and §63-66 are moving the rest of the
stack out to the netstack daemon. Numbers into the kernel's responder would be
an ABI for code that is on its way out. They would also make the kernel answer
for the whole machine under the name `neo`, which is fixed in its source.

The route is the one Linux takes: an ordinary program owns UDP port 5353 and
answers and asks on the network itself (`avahi-daemon`'s job). The commands
your simulation pretended to be are then its clients. What it has today:

- **Port 5353 is free.** `mdns::init` runs only from the kshell command
  `mdns init`; no boot binds it.
- **IPv4 multicast works from a native program**: `IP_ADD_MEMBERSHIP` reaches
  `SYS_UDP_MCAST_JOIN` (a `Socket` capability with `WRITE`).
- **Missing, and lane A's** (`roadmap.md`, under the netstack row): an IPv6
  group join a program can reach (libc's `IPV6_JOIN_GROUP` has no call to go
  to); a per-socket multicast TTL and loop setting (the kernel's UDP sends at
  TTL 64, and RFC 6762 wants 255 on what a responder sends); and the multicast
  options through the Linux ABI and the netstack daemon. Tell lane A when your
  responder needs them; they are small.
- **Then lane D's**: libc accepts `IP_MULTICAST_TTL` and `IP_MULTICAST_LOOP`
  and does nothing with them, which is right only until the kernel has
  somewhere to send them.

The RFC 6762/6763 encoding and decoding in `net/mdns.rs` is real and tested;
it is worth lifting into whatever you build rather than rewriting.
