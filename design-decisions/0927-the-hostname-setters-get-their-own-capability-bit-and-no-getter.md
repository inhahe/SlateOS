## §927 — The hostname setters get their own capability bit, and no getter

**Date:** 2026-09-10
**Decided by:** Claude (autonomous) — lane B requested the pair and proposed the
no-getter asymmetry; lane A owns the syscall table and made the calls on the
capability, the length bound and the check ordering, and agreed with lane B's
asymmetry rather than adding a getter for symmetry.
**Lane:** A

**In short:** Until today, a program that asked our system to change its host
name was told the change had worked, and nothing outside that one program
changed — because the name was being kept in a variable private to the asking
program, and read back out of the same variable. There is now a real syscall
that changes it for the whole machine. Two choices in it are worth recording:
it needs a specific permission rather than "are you the administrator?", and
there is deliberately no matching syscall to *read* the name back.

### What was wrong

`posix/src/unistd.rs` held the hostname in a `process_global!` — a `static mut`
in the calling program's own address space — initialised to `"localhost"`.
`gethostname` read that buffer, `sethostname` wrote it, `uname`'s `nodename`
read it too, and **`sethostname` returned 0**. So:

| | `setgroups` (instance one) | `sethostname` (until today) |
|---|---|---|
| what it did | nothing | nothing observable |
| what it returned | `-1`, `ENOSYS` | **`0`** |
| what the caller learned | the truth | that it had worked |

A program could `sethostname("web01")`, `gethostname()` back `"web01"`, and
conclude the machine was renamed. **A self-consistent lie is harder to find than
an honest refusal**, which is why this survived from 2026-08-22 —
`known-issues.md` → `B-POSIX-HOSTNAME-IS-PROCESS-LOCAL`, filed when
`userspace/coreutils`' own `hostname` command was rewritten to bypass the C
functions. The workaround went into one program; the defect stayed in libc.

`fs::nameservice::set_hostname` existed the whole time and only the Linux-ABI
table could reach it. This is the fourth instance of that shape, and the first
found by `scripts/check-linux-only-capabilities.py` rather than by tripping over
a symptom.

### Decision 1: a new `Rights::SET_HOSTNAME` bit

**Alternatives.** (a) Mirror the Linux-ABI handler and check `uid == 0` from the
caller's credentials. (b) Reuse an existing right — `WRITE` on the process was
the nearest. (c) A new bit, `1 << 20`.

**Chose (c).** (a) is ambient authority — permission you get by *being* someone
rather than by holding a token — which CLAUDE.md forbids outright, and
`known-issues.md` → `A-SET-CREDENTIALS-IS-GATED-ONLY-IN-USERSPACE` records what
it costs: the kernel primitive behind `setuid` once had no check at all, because
the policy lived in a userspace wrapper that any process could simply not call.
(b) fails for the reason `SET_CREDENTIALS`, `DEBUG` and `MEMORY_LOCK` each got
their own bit: granting "may write its own limit table" would silently also
grant "may rename the machine every other process on it reports", and a bit that
means two things is a bit that gets granted for one of them. `Rights` is a `u64`
with bits to spare.

**CORRECTION, 2026-09-10, and it invalidates the paragraph that followed.**
Lane B checked this claim and it is false; I verified their four findings against
my own tree rather than taking them:

* `Rights::ALL` is `Self(u64::MAX)` -- every bit, not an enumeration of the
  declared rights -- so it contained `1 << 20` the instant `SET_HOSTNAME` was
  declared (`kernel/src/cap/rights.rs:205`).
* `kernel/src/main.rs:9542` grants the init process
  `(ResourceType::Process, 0, Rights::ALL)`, class-wide.
* `pcb::has_capability_type` takes no `resource_id` and never consults one, so a
  class-wide grant satisfies it.
* `pcb.rs:1674` -- fork does `parent.cap_table.clone()`, while a fresh
  `Process::new` starts empty.

**So the reachable set was never empty: it is init, plus everything forked from
it that nothing has narrowed since.** PID 1 passed this gate the moment the bit
existed. The original paragraph read:

> Nothing grants `(Process, SET_HOSTNAME)` yet, so `sethostname` goes from
> returning `0` and lying to returning `PermissionDenied` for everyone.

which was wrong in both halves, and wrong in the direction that matters: I
recorded a privileged write as unreachable when it was reachable from PID 1 and
its descendants. The hazard is general and is recorded separately in
`known-issues.md` -> `TD-A-A-NEW-RIGHT-IS-GRANTED-BEFORE-ANYONE-DECIDES-WHO-HOLDS-IT`:
because `ALL` is a wildcard rather than a list, **every right added to this enum
in future is granted to init retroactively, before the decision about who should
hold it is taken.**

What survives of the original reasoning: the capability is still the right gate,
and still better than the `uid == 0` check the Linux-ABI handler uses. What does
not survive is the claim that it was inert.

**The cost as originally stated, now known to be wrong.** It read that nothing
grants `(Process, SET_HOSTNAME)`, so `sethostname` goes from returning `0` and
lying to returning `PermissionDenied` for everyone. The grant side is `init`'s,
which is lane B's tree. That is a worse *user-visible* outcome than "it works"
and a better one than "it lies", and it is the state lane B asked for when they
wrote that an unprivileged caller should learn it is unprivileged — permanent —
rather than that the call is unimplemented, which is not.

It is also deliberately **narrower than Linux's `CAP_SYS_ADMIN`**, which bundles
several dozen unrelated privileges; a projection from `CAP_SYS_ADMIN` onto this
bit is one-way.

### Decision 2: no getter

**Alternatives.** (a) A symmetric `SYS_HOSTNAME_GET`. (b) Setter only, with
reads served by `/proc/sys/kernel/hostname`.

**Chose (b),** which was lane B's proposal and is right for the reason they gave:
`/proc/sys/kernel/hostname` already serves reads, it is what `osh` fills
`$HOSTNAME` from and what `sysctl` maps `kernel.hostname` onto, and libc reading
it is one `open`/`read`/`close` on a call nothing does in a loop. A second read
path would give one value two sources that can disagree — **which is the exact
shape of the defect this syscall was added to end.** Symmetry is not a reason to
build the thing that caused the bug.

Lane B offered to take a symmetric pair if lane A preferred one. Declined, on
the above.

### Decision 3: 64 bytes, matching Linux, not 253

`fs::nameservice::set_hostname` accepts up to 253 — the DNS limit. The Linux-ABI
`sethostname` caps at 64, Linux's `__NEW_UTS_LEN`. The native syscall takes the
**stricter** bound, so a name a native caller can set is always a name a
Linux-ABI caller can set. The alternative allows a native program to set a name
that a ported program then cannot, which is two paths disagreeing about one
value again.

### Decision 4: the capability is checked before the arguments

An unprivileged caller must not learn which lengths the kernel accepts for a
call it may not make. Linux orders `CAP_SYS_ADMIN` first for the same reason, as
does our own Linux-ABI handler. `test_dispatch_uts_name` pins this by passing a
deliberately over-long name and requiring the answer *not* be
`InvalidArgument` — which also proves the numbers are registered, with no
process needed.

### What is not covered, so the tests are not read as wider than they are

That a *granted* capability lets a name through. That needs a process holding
`(Process, SET_HOSTNAME)`, and nothing grants it yet. "The gate refuses
everyone" and "the gate works" are indistinguishable from a test that only ever
gets refused, so the dispatch test says so in its own doc rather than leaving
the gap silent.
