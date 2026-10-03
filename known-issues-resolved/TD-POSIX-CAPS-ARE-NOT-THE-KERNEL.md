### TD-POSIX-CAPS-ARE-NOT-THE-KERNEL'S. libc's Linux capability words start as "all caps held" and are never seeded from the process's real kernel capabilities — 🔷 **Q44 ANSWERED 2026-08-15 → §312; steps 1–2 DONE, step 3 (flipping the gates) OPEN** — 2026-08-12

**Status: steps 1 and 2 of §312 are done; step 3 is the remainder and is what
keeps this entry open.**

- **Step 1 — an enumerating query syscall** (lane A). `SYS_CAP_QUERY` (400)
  gained an enumerate mode: `arg0` a `CapEntryInfo` array, `arg1` its capacity
  in entries, truncation reported as `BufferTooSmall`/`ERANGE` with nothing
  written rather than as a short list. See
  `requests/a-b-cap-query-enumeration-landed.md`.
- **Step 2 — libc seeds its words from it** (lane B, 2026-08-16).
  `posix/src/sys_capability.rs` → `mod kernel_view`: an ABI mirror of
  `CapEntryInfo`, the `(ResourceType, Rights)` predicate table from §312, the
  hand-written `CAP_SYS_ADMIN` union, and `refresh()`, which
  `__libc_start_main` calls before the ELF constructors. `capget()` now reports
  the kernel's answer **intersected with** whatever the process has dropped, so
  a real `capset()` drop still binds and a later refresh cannot undo it.
- **Step 3 — the gates are still advisory, and that is what is left.** The libc
  gate sites still read the stored words through `has_capability()`, which on
  the target still starts permissive. Pointing `has_capability` at
  `reported_caps_effective` is one line, but **that line is not the work**. The
  first obstacle — a dozen gates written as capability-*only* where Linux's rule
  is "capability **or** something else" — was its own entry,
  TD-POSIX-CAP-GATES-OMIT-LINUX-S-NON-CAPABILITY-ALTERNATIVE, and is **now
  fixed** (all 14 sites, 2026-08-16). What remains is below.

  **Step-3 audit, 2026-08-16.** After the §314 rework the crate has **48
  production gate sites** across **19 distinct `CAP_*` bits** (the older count
  of 63/22 predates §314, which deleted the four `CAP_KILL` gates among others).
  `kernel_view::project` derives only **six** bits — `CAP_SYS_RAWIO`,
  `CAP_KILL`, `CAP_SYS_PTRACE`, `CAP_SYS_NICE`, `CAP_NET_RAW`, `CAP_SYS_ADMIN` —
  and §312 says every unnamed bit reads **false**, on purpose. So the flip turns
  22 of the 48 sites into permanent denials. Classifying them by what actually
  stands behind the gate:

  | Consequence | Caps | Sites |
  |---|---|---|
  | **Real regression** — the gate fronts a working kernel path, so the operation becomes impossible for every process | `CAP_SETUID`, `CAP_SETGID`, `CAP_SYS_TIME`, `CAP_NET_BIND_SERVICE`, `CAP_IPC_LOCK`, `CAP_SYS_RESOURCE`, `CAP_WAKE_ALARM` | 12 |
  | **Cosmetic** — the call is an `ENOSYS` stub today, so the only change is `ENOSYS` → `EPERM`, which is what Linux reports to an unprivileged caller anyway | `CAP_SYS_MODULE`, `CAP_MKNOD`, `CAP_SYS_BOOT`, `CAP_SYS_CHROOT`, `CAP_DAC_READ_SEARCH`, `CAP_BPF`, `CAP_PERFMON` | 10 |

  The twelve in the first row are the real work, and `CAP_SETUID`/`CAP_SETGID`
  show why §312's "under-reporting is recoverable" premise does not carry all
  the way: it holds only where the kernel re-checks.
  `SYS_PROCESS_SET_CREDENTIALS` is a thin primitive that explicitly does *not*
  re-run the policy check (its doc comment says so), so libc is the sole decider
  and an under-report is final. §314 has already removed every gate that *did*
  stand in front of a re-checking kernel — which means the gates that survive
  are, by construction, exactly the ones where under-reporting cannot be
  recovered from. Each of the twelve therefore needs a projection rule, and a
  rule needs a `(ResourceType, Rights)` pair the kernel is willing to mean it.
  Only `CAP_SETUID`/`CAP_SETGID` has an honest preimage already (`Process` +
  `METADATA` — credentials are process attributes), and that pair is filed as
  `requests/b-a-cap-grants-for-312-step3-fixtures.md`. The other seven sites —
  the three clock setters, privileged `bind`, `setrlimit`, `mlock` past the
  rlimit, alarm timerfds — have **no** object behind them at all, and §312's own
  precedent (it refuses to invent a handle for `sethostname`) says an invented
  one is ambient authority in a capability costume. Whether to give them real
  kernel objects, drop the restriction, or leave them denied is **`open-questions.md`
  Q48**, and step 3 should not flip until that is answered.

  **Fixtures.** The old blocker list here — `services/ctest-jobctl`,
  `self_test_cctty`, `self_test_cpgroup` — is **stale**: §314 deleted libc's
  pre-emptive `CAP_KILL` gate, and those three make no other gated call, so they
  need nothing. Re-auditing every fixture against the 48 sites turns up exactly
  **two** that do: `fastpy-nice` (raises its own priority; needs `Thread` +
  `IO_REALTIME` for `CAP_SYS_NICE`) and `fastpy-setuid` (changes its uid/gid to
  values it does not hold; needs whatever pair the kernel picks for
  `CAP_SETUID`/`CAP_SETGID`). Both are spawned from `kernel/src/proc/spawn.rs`,
  lane A's tree — filed as
  `requests/b-a-cap-grants-for-312-step3-fixtures.md`. Note `fastpy-nice`'s
  spawn-site comment claims "the calls it makes only *lower* priority (need no
  cap)", which is wrong (nice 0 → −7 is a raise); the fixture's own build recipe
  says it is deliberately exercising the gated path.

  **And `refresh()` must become fail-closed.** While the gates are advisory,
  "we could not ask" and "you may" are the same thing; once they are binding,
  they stop being.

**What.** `posix/src/sys_capability.rs` keeps the three Linux capability sets
(effective/permitted/inheritable) in its own store and initialises them from
`CAPS_DEFAULT` — every defined capability bit set. Nothing ever asks the kernel
what the process actually holds. So on the target:

- `capget()` reports "all caps" to a process the kernel granted none.
- Any libc-side capability gate (e.g. the `CAP_KILL` check in
  `posix/src/signal.rs`) is a userspace-only test that always passes on a
  freshly-started process, regardless of the capability list its `SpawnOptions`
  named.
- `capset()` "drops" a capability the process may never have had, and the
  kernel is not told, so the drop constrains only libc's own gates.

Today this is *safe by accident*: the kernel re-checks every privileged
operation itself, so libc's optimistic answer cannot grant anything — it can
only fail to pre-empt a denial the kernel will issue anyway. It is recorded
because the failure mode when that stops being true is silent: a caller that
trusts `capget()` (a port that decides whether to attempt an operation, or
drops privileges based on what it thinks it has) gets a wrong answer with no
error anywhere.

**Where.** `posix/src/sys_capability.rs` (`CAPS_DEFAULT`, ~line 251, and the
two `store` modules below it). Consumers: `capget`/`capset` in the same file,
and every `has_capability(...)` gate in the crate. **Surveyed 2026-08-12:**
**63 production gate sites** spanning **22 distinct `CAP_*` bits**, all of them
inside `posix/` (0 in `userspace/`, `services/`, `apps/`). Led by
`posix/src/process.rs` (13) and `posix/src/unistd.rs` (10); by bit, by
`CAP_SYS_ADMIN` (20), `CAP_SYS_NICE` (6) and `CAP_SYS_PTRACE` (5). The
`CAP_KILL` gate in `posix/src/signal.rs` named below is one example of 63, not
the extent of it. (Note when re-counting: a bare `grep -c has_capability(`
returns ~251, but the great majority are `assert!`s in `#[cfg(test)]` modules —
filter those out or the surface looks 4× larger than it is. The symbol is
`has_capability`, not `has_cap`; grepping the latter returns zero and makes the
gap look like it is confined to `CAP_KILL`.)

**Reproduce.** Spawn a ring-3 fixture with `capabilities: &[]` (as
`self_test_cctty` and `self_test_cpgroup` both do) and call `capget()` on
itself: it reports the full set. `services/ctest-jobctl`'s doc comment already
records the consequence out loud — "our libc's own `CAP_KILL` gate reads the
process capability words, which start out with every capability held" — which
is why that fixture needs no capabilities to make a real cross-process send.

**Proper fix.** Seed the words at libc startup from the kernel, and push
`capset` changes back to the kernel. The work is not the plumbing but the
*mapping*: the kernel's model is 25 `ResourceType` variants × 12 `Rights` bits
of **per-object** authority, not Linux's 41 **ambient** numbered bits, so
someone has to define which kernel rights imply which `CAP_*` — and
`CAP_SYS_ADMIN`, which is 20 of the 63 gate sites, has no natural preimage at
all. That mapping decides what a Linux port is allowed to conclude about our
capability model, so it was an operator decision: **asked as
`open-questions.md` Q44** (2026-08-12) and **answered 2026-08-15 — option A**,
recorded as `design-decisions.md` **§312**. See "The answer and what is left"
below.

**CORRECTED 2026-08-12.** An earlier version of this section named
`SYS_CAP_QUERY` (400) as the syscall to seed the words from. **It cannot serve
that role.** Its handler (`kernel/src/syscall/handlers.rs`, `sys_cap_query`)
returns only a *count* of the caller's capabilities — it takes no buffer and
enumerates nothing; the doc comment on it says so explicitly ("A future
extension will support filling a user-space buffer with detailed capability
entries"), and its only consumer today is `userspace/strace`'s syscall-name
table. So an **enumerating** query syscall has to be built first, under any
answer to Q44. Do not start the libc side expecting 400 to hand you the set.

**The answer, and what is left — 2026-08-15.** Q44 was answered **A —
conservative projection** (`design-decisions.md` §312). Each `CAP_*` is derived
from a specific `(ResourceType, Rights)` predicate and reports **not held**
whenever no rule matches, so the default is *deny*: `CAP_SYS_RAWIO` ⇐ a `PortIo`
handle with `READ|WRITE`, `CAP_KILL` ⇐ `Process` with `SIGNAL`, `CAP_SYS_PTRACE`
⇐ `Process` with `DEBUG`, `CAP_SYS_NICE` ⇐ `Thread` with `IO_REALTIME`.
`CAP_SYS_ADMIN` — 20 of the 63 sites — is deliberately **not** derived: it gets an
explicit hand-maintained union with a comment per member, because Linux's junk
drawer has no preimage in a per-object model and any derived rule would be either
permanently false or broad enough to re-grant everything.

Rejected, and worth not re-litigating: **B** (`ResourceType::PosixCapability`)
was refused as ambient authority wearing a capability costume — process-wide
authority tied to no object — even though it is the option that would have made
`CAP_SYS_ADMIN` easy. **D** (`capget()` failing outright) was refused because
Linux ports call it informationally, so it trades one silent wrong answer for
loud breakage everywhere; that argument still stands and is why the optimistic
answer is allowed to persist through steps 1–2 below rather than being turned off
first.

**This entry stays open until step 3.** The three steps, in order, from §312:

1. **An enumerating query syscall must exist** (see the CORRECTED note above —
   400 returns a count). **Lane A's tree**, filed as
   `requests/b-a-cap-enumerating-query-syscall.md`. Nothing changes behaviourally
   when it lands.
2. **libc seeds its three words from that query** instead of `CAPS_DEFAULT`.
   Still no behavioural flip: the gates remain advisory.
3. **The gates stop being advisory** — the boot-test-visible step, gated on the
   fixture grants in the paragraph below.

**Do not make a gate truthful without freeing QEMU first.** Fixtures depend on
the permissive behaviour: `services/ctest-jobctl` (documented in its own doc
comment), and `self_test_cctty` / `self_test_cpgroup`, which spawn with
`capabilities: &[]`. All three need real capability grants in the same change,
and that is boot-test-visible.
