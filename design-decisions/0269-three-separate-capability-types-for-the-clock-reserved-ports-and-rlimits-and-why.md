## §269 — Three separate capability types for the clock, reserved ports and rlimits — and why `mlock` gets its own right bit

**Date:** 2026-08-21
**Decided by:** Claude (autonomous). The surrounding policy — that privileged
POSIX operations are *projected* from real capabilities rather than refused
outright — was the operator's (Q48 answer B); what is decided here is only the
shape of the objects that projection reads.
**Lane:** A

**In short:** Linux programs ask for a handful of powers that a plain user is
not supposed to have: setting the wall clock, listening on a low-numbered
network port such as 80, raising their own resource limits, and pinning memory
so it can never be swapped out. Our system does not have a "root" that simply
owns all of those; it has tokens, and you can only do a thing if you hold the
token for it. Up to now there were no tokens for these four powers, so the
translation layer had nothing to check and had to just say no. This adds three
token types (clock, reserved port, resource limits) and one extra permission
bit (may pin memory), and grants them to the first process at boot so it has
something to hand down.

### Why three types and not one "privileged operation" type

A single `PrivilegedOp` type with the specific power encoded in its
`resource_id` would have been a smaller change, and the numbers would have been
easier to extend later without touching the enum. It was rejected because the
`resource_id` field is the only place a capability can be *narrowed*, and these
three narrow along completely different axes:

| Type | What `resource_id` means | The narrowing that matters |
|---|---|---|
| `SystemClock` | reserved, always 0 | none — there is one clock |
| `PrivilegedPort` | a port number, or 0 for all | "may bind 443, and nothing else" |
| `ResourceLimit` | a target PID, or 0 for all | "may raise its own limits, not other processes'" |

Collapsing them means one field carrying three incompatible meanings, and every
check site having to know which meaning applies before it can read the number.
The narrowings above are the entire point of having capabilities here rather
than a boolean; a design that makes them awkward to express is the wrong one
even though it has fewer moving parts.

Port 0 is safe as the class-wide wildcard because port 0 is "pick one for me" in
the sockets API and is never itself a bindable address, so the two readings
cannot collide.

### Why `mlock` is its own right bit rather than `WRITE`

`ResourceLimit` with `Rights::WRITE` means "may raise a limit". Locking memory
past the quota looks superficially like the same thing — both defeat a bound the
system set — so folding it into `WRITE` was tempting and would have cost no new
bit.

Rejected because the two failure modes are not comparable. Raising your own
file-descriptor limit costs a few kilobytes of kernel bookkeeping. `mlock`ing
past the quota takes *physical* memory permanently out of circulation for every
other process on the machine, and that is precisely what the quota exists to
bound. Under one bit, "this daemon may raise its own fd limit" silently also
means "this daemon may pin all of RAM". Bit 19 (`MEMORY_LOCK`) keeps the
expensive power separately grantable. The cost is one more bit out of 64 and one
more thing for a grant site to remember.

### The `TRANSFER` right is granted to init even though nothing reads it

Lane B asked directly whether the delegation path reads `Rights::TRANSFER` at
the grant or at the transfer. The answer, checked rather than assumed, is
**neither: nothing in the kernel reads that bit at all today.** A child's
capabilities come from an explicit list its spawner builds
(`SpawnOptions::capabilities`), not from narrowing the parent's table, so there
is no delegation step for the bit to gate.

It is granted anyway. Init's whole role for these three objects is to hand
narrowed copies down — a time daemon gets the clock, a web server gets its one
port — so when that path is built, init must already be permitted to use it. The
alternative is a delegation that fails for PID 1 specifically and gets debugged
from scratch by whoever adds it. The grant site says plainly in a comment that
the bit is currently inert, so nobody reads enforcement into it that does not
exist; if `TRANSFER` ever acquires a *different* meaning, that comment is the
thing that must be revisited.

### The ABI pin was widened, not just extended

`test_cap_entry_info_abi` pinned the discriminants of the first and a middle
variant. It now also pins the **last** one (`ResourceLimit = 29`), because
appending a variant without updating the pin is exactly the change that would
otherwise go unnoticed — and lane B mirrors these numbers by hand in
`posix/src/sys_capability.rs`, where no compiler ties the two copies together.
