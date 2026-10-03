### [A] `netdiag`'s DNS lookup is simulated and cannot fail, so the tool a person runs to diagnose name resolution always says it works -- 2026-09-21
**Status:** HALF DONE (stamped 2026-09-25) -- the honest half is in: `ping`, `traceroute` and `dns_lookup` in `kernel/src/fs/netdiag.rs` return `NotSupported` instead of inventing answers. Real lookups through `fs::nameservice`, and a writer for `connectivity`, are still open

**In short:** the kernel has a network-diagnostics tool with a `dns_lookup`
command. It does not look anything up. It returns `127.0.0.1` for the name
`localhost` because that string is hardcoded in it, and invents an address
for everything else. Someone typing it to find out why a name will not
resolve is told the name resolves.

**Measured:** `kernel/src/fs/netdiag.rs` `dns_lookup`:

```rust
    let resolved = if name == "localhost" {
        String::from("127.0.0.1")
    } else {
        // Simulate resolved address.
```

It never calls `fs::nameservice::resolve` or `net::dns::resolve`. Reachable
from `kshell.rs:81476`, i.e. a command a human types; `procfs` touches only
`stats()`.

**Two things follow, and the second is the sharper one.**

1. The hardcoded `localhost -> 127.0.0.1` **duplicates the hosts table**,
   which holds exactly that mapping. So the tool agrees with the real
   resolver by coincidence of two constants, and would keep agreeing after
   someone edited the hosts file.
2. **It cannot fail.** A diagnostic whose purpose is to report a failure has
   no path that reports one. `[5/10] DNS lookup: OK` in every boot log is a
   test of the simulation.

**Related, same file:** `[2/10] ping localhost: OK` is `ping("127.0.0.1", 4)`.
The name is in the message and never in the call -- which is how this
morning's `sys_dns_resolve` hosts-table fix nearly shipped unexercised
behind a log line that appears to cover it.

**The fix, and why it is not one line.** Point `dns_lookup` at
`fs::nameservice::resolve` first and `net::dns::resolve` on `NotFound` --
the order `nameservice` already declares. But that changes what the module
self-test means, and the self-test must change with it:

| rung | today | after |
|---|---|---|
| `[3/10] ping remote` | `ping("example.com")`, asserts latency 25000 | a real lookup of `example.com` has no DNS server in QEMU; must expect failure or skip |
| `[5/10] DNS lookup` | asserts success on a simulation | assert `localhost` -> 127.0.0.1 from the **table**, and that an unknown name FAILS |

That second row is the point of doing it at all: the rung has to gain a
failing case, because a diagnostic that cannot report failure is the defect,
not the missing lookup.

**Why it is recorded rather than done now:** 22 changes are queued and
unverified behind a running boot, and this one alters a self-test's
expectations -- the exact kind of change that turns one red boot into an
ambiguous one. It is the first thing to pick up once the batch is green.
