## TD-B-AVAHI-WAS-A-SIMULATION-OF-A-REAL-KERNEL-SERVICE (lane B, 2026-09-10) — crate deleted

**In short:** `userspace/avahi` was 5,147 lines and 192 tests answering mDNS
questions from a hardcoded table, while `kernel/src/net/mdns.rs` is a real
mDNS/DNS-SD responder on the real multicast addresses. Deleted under
`design-decisions.md` 1006.

**What it did.** `resolve_hostname` was a `match` on the hostname:

```rust
"slateos-host.local" => {
    results.push((IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)), PROTO_INET));
```

So `avahi-resolve slateos-host.local` printed an IP address as a fact about the
network having sent no packet, and any other name resolved to nothing — also a
fact it had not measured. All six personalities are network operations
(`avahi-daemon`, `-browse`, `-resolve`, `-publish`, `-autoipd`,
`-set-host-name`) and the crate did no network I/O of any kind; its only reads
were of its own config file.

**Why deletion rather than the `wpa` treatment.** `wpa` kept
`wpa_passphrase`, which is pure computation and needs no I/O. Nothing here is:
every command asks the network something. The config parser is real, but a
parser is a component and not a command, and it validated a file no daemon
would read.

**The real one already exists, one lane over.** `kernel/src/net/mdns.rs`
exposes `resolve_local`, `resolve_local_v6`, `browse_services`,
`register_service`, `unregister_service` and `set_hostname` — the exact
operations the simulation fabricated. They are `pub fn` with no syscall
surface, so userspace cannot reach them today. Asked in
`requests/b-a-mdns-has-no-syscall-surface-and-userspace-had-a-simulation-instead.md`.

**How it was found.** Not by reading the crate. Its module doc says "All data
is simulated" in line 16 — an honest note nobody sees, because a user reads
`avahi-resolve`'s output and not its source. What made it a deletion rather
than a note was checking the roadmap and finding mDNS marked done TWICE: once
for the kernel responder with the real multicast addresses, once for this. Two
`[x]` entries for one feature is the tell.
