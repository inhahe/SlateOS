## §312 — libc's Linux capability words are a conservative projection of the kernel's `(ResourceType, Rights)` handles, not a fiction

**Date:** 2026-08-15
**Decided by:** Operator (Claude recommended this option)
**Question:** `open-questions.md` Q44 (now RESOLVED)
**Answer, verbatim:** *"Q44: a."*

### The decision

**Option A — conservative projection.** Each Linux `CAP_*` bit is derived from a
specific `(ResourceType, Rights)` predicate over the capabilities the process
actually holds, and reports **not held** whenever no rule matches. The default
is *deny*, not *allow*: an unmapped `CAP_*` is false, never true.

Worked examples of the rule shape:

| `CAP_*` | Predicate over held capabilities |
|---|---|
| `CAP_SYS_RAWIO` | any `PortIo` handle with `READ` or `WRITE` |
| `CAP_KILL` | any `Process` handle with `SIGNAL` |
| `CAP_SYS_PTRACE` | any `Process` handle with `DEBUG` |
| `CAP_SYS_NICE` | any `Thread` handle with `IO_REALTIME` |
| `CAP_NET_RAW` | any `NetRaw` handle |
| `CAP_SYS_ADMIN` | **hand-maintained union** — see below |

`CAP_SYS_ADMIN` is the exception and is explicitly *not* derived. It is Linux's
junk drawer, it has no natural preimage in a per-object model, and it accounts
for 20 of libc's 63 gate sites. It gets an explicit, hand-written union of the
predicates that each of those 20 sites actually needs, maintained as a list with
a comment per member. A derived rule for it would either be permanently false
(breaking 20 sites) or so broad that it re-grants everything (which is the bug
being fixed).

### What this replaces

`posix/src/sys_capability.rs` kept Linux's three capability words in libc's own
memory and initialised them from `CAPS_DEFAULT` with **every bit set**. Nothing
ever asked the kernel what the process held, so `capget()` reported the full set
to a process spawned with `capabilities: &[]`, and every libc-side gate passed.

That was safe only by accident: the kernel re-checks every privileged operation
itself, so libc's optimistic answer could never *grant* anything. The failure
mode is the silent one — a port that trusts `capget()` to decide what to attempt,
or to drop privileges, gets a confidently wrong answer with no error anywhere.

### Why A and not the others

- **B (a `ResourceType::PosixCapability` handle granted per `CAP_*` at spawn)**
  was rejected on principle. It is ambient authority wearing a capability
  costume: a handle meaning "may do `CAP_SYS_ADMIN` things", process-wide, tied
  to no object. It reproduces exactly the property `design.txt` rejects while
  looking compliant because it is spelled as a handle. Notably it is the option
  that would have made `CAP_SYS_ADMIN` *easy*, and it was still not worth it.
- **C (keep libc optimistic, document `capget()` as "the ceiling, not the
  grant")** is the honest do-nothing. It breaks no fixture and carries no risk,
  but it leaves the silent-wrong-answer trap open — which is the entire reason
  `TD-POSIX-CAPS-ARE-NOT-THE-KERNEL'S` was logged.
- **D (make `capget()` fail with `ENOSYS`)** is the most honest answer and the
  worst outcome for a compatibility layer. Linux software calls `capget()`
  informationally and does not expect failure; this trades one silent wrong
  answer for loud breakage across every port.

### Consequences, in the order they have to happen

1. **An enumerating capability query syscall must exist first.** This is not
   optional under any option and is not in dispute. `SYS_CAP_QUERY` (400,
   `kernel/src/syscall/handlers.rs::sys_cap_query`) returns only a *count* of
   the caller's capabilities; its own doc comment says "a future extension will
   support filling a user-space buffer with detailed capability entries", and
   its sole consumer today is `userspace/strace`'s syscall name table. **That
   handler is Lane A's tree** — filed as `requests/b-a-cap-enumerating-query-syscall.md`.
2. **libc seeds its words from that query**, once, at process start, and on
   demand after any operation that could change the set.
3. **The libc gates stay advisory until the fixtures are given real
   capabilities.** This is the staging that keeps the change from being a
   flag-day. Making a gate truthful breaks every fixture that relies on the
   permissive behaviour, and there are known ones:
   `services/ctest-jobctl` (its doc comment says so out loud — "our libc's own
   `CAP_KILL` gate reads the process capability words, which start out with
   every capability held"), `self_test_cctty`, and `self_test_cpgroup`, all
   spawned with `capabilities: &[]`. That is boot-test-visible, so the flip
   from advisory to enforcing lands with QEMU free.

### The cost accepted

`capget()` becomes truthful, which means code that previously sailed through a
libc gate now gets refused by it. That is the point — but it means the flip is a
behavioural change to every fixture's effective privilege, not a no-op refactor,
and it must be sequenced (step 3) rather than switched on with the mapping.

**Where it lives:** `posix/src/sys_capability.rs` (`CAPS_DEFAULT` ~line 251),
the 63 gate sites led by `posix/src/process.rs` (13) and `posix/src/unistd.rs`
(10), `kernel/src/cap/mod.rs` + `kernel/src/cap/rights.rs` (the model being
projected), `kernel/src/syscall/handlers.rs` (`sys_cap_query`), and
`known-issues.md` → `TD-POSIX-CAPS-ARE-NOT-THE-KERNEL'S`.
