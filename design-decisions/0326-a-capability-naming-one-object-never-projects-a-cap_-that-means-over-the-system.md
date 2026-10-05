## §326 — A capability naming one object never projects a `CAP_*` that means "over the system"

**Date:** 2026-08-17
**Decided by:** Claude (autonomous)

**In short:** our libc reports Linux-style capabilities (`CAP_KILL`,
`CAP_SYS_ADMIN`, …) by looking at the real, per-object capabilities the kernel
granted the process and deciding which Linux names they add up to. One of those
rules read "holds permission to signal a process ⇒ holds `CAP_KILL`" — but the
kernel automatically gives every parent permission to signal *its own children*,
so every process that had ever started a child was reported as holding
`CAP_KILL`, which means "may kill **anything** on the system, ignoring who owns
it". The decision is that a capability naming one particular object can never
produce a Linux capability that means authority over the whole system; only a
capability granted over the *class* ("processes", not "process 4271") can.

**Terms.** A *capability* here is a triple — resource type, resource id, rights
— e.g. `(Process, 4271, SIGNAL)`, "may signal process 4271". A *class-wide*
capability writes `0` for the id, the kernel's sentinel for "no particular
instance"; that is settled and boot-checked (§212, lane A). *Projection* is the
libc-side function that turns the list of held triples into Linux's capability
words, `posix/src/sys_capability.rs::kernel_view::project` (§312).

**The alternatives**

- **A — read the id on every rule.** Uniform and easy to state.
  *What changes:* raising your own thread's priority stops working, because the
  grant that permits it names your thread. Rejected: two of the rules name the
  caller itself, and for those an instance grant is not a weaker form of the
  authority, it *is* the authority.
- **B — read the id on no rule (what we had).** *What changes:* nothing; the
  over-report stands. Rejected: it contradicts the projection's own stated
  contract, in the direction that says "you may" about authority the kernel
  would refuse.
- **C — read the id where the Linux capability is system-wide (chosen).**
  *What changes:* four rules narrow, two stay as they are, and the difference is
  decided by what the Linux capability *means*, not by which resource type it
  reads.

**The rule, applied**

| predicate | Linux capability | id read? | why |
|---|---|---|---|
| `(Process, SIGNAL)` | `CAP_KILL` | **yes** | "signal anything, overriding the uid check" — and SIGNAL over one child is auto-granted by `fork`/`spawn` |
| `(Process, DEBUG)` | `CAP_SYS_PTRACE` | **yes** | the kernel's own ptrace gate is a *per-target* check, so a handle on one target is not system-wide debug authority |
| `(Process, DEBUG)` | `CAP_SYS_ADMIN` | **yes** | its members here are `bpf`, `perf_event_open`, `fanotify_init` — each observes the system, not one named process |
| `(File, METADATA)` | `CAP_SYS_ADMIN` | **yes** | its members here are `mount`, `swapon`, `quotactl` — reshaping the filesystem, not chmod'ing one file |
| `(Thread, IO_REALTIME)` | `CAP_SYS_NICE` | no | the thread named is the caller's own, and raising your own priority is what the gate sites ask for |
| `(Process, SET_CREDENTIALS)` | `CAP_SETUID`/`CAP_SETGID` | no | `SYS_PROCESS_SET_CREDENTIALS` rewrites the *calling* process's uid and gid, so a grant naming a pid names the holder |
| `PortIo`, `NetRaw`, `Namespace`, `IoScheduler` | various | no | these have no per-instance grants at all: the kernel gates them with a type-only `require_cap_type` and every grant site passes `0`, so the test would be a no-op that reads like a decision |

**Why the line is "what the capability means" and not "which type it reads."**
Resource type is the tempting discriminator — it is right there in the
predicate — and it gives the wrong answer twice. `Process` appears on both
sides of the table: `SIGNAL` and `DEBUG` are authority over *another* process,
`SET_CREDENTIALS` is authority over yourself. A rule keyed on the type would
either break `setuid()` or leave `CAP_KILL` universal. The question that
separates them cleanly is Linux's: does this capability let you act on a
process you were never handed?

**What this obliges.** A new derived bit has to answer that question in a
comment, and `test_system_wide_bits_require_a_class_wide_entry` is a table that
must gain a row when a system-wide one is added. Both directions are asserted
for each rule — the class-wide form projects, the instance form does not —
because a rule that never fires is not conservative, it is a gate nobody can
pass. The `SET_CREDENTIALS` exemption is the one to revisit first: no grant site
confers it today, so both readings still fit, and lane A asked that it stay
id-agnostic until a real grant site settles it
(`requests/a-b-resource-id-zero-names-the-class.md`).
