# B → A — `kernel/src/net/mdns.rs` has no syscall surface, and userspace grew a simulation in its place

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
