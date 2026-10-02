## §350 — The clock, the privileged ports and the resource limits each get a real kernel object, rather than staying permanently denied

**Date:** 2026-08-21
**Decided by:** Operator (option B; Claude recommended B for the clock and the
limits but C for the port, and the operator chose B for all three)

**In short:** Our C library used to *claim* a program could do anything
privileged, and then let the kernel say no. §312 replaced that with the truth:
work out what the program may do from the tokens the kernel actually handed it.
The catch is that a handful of privileges have no token to derive from, so the
honest answer for them is "no", forever, for everyone — and that would
permanently break setting the clock, running a web server on port 80, and
raising your own resource limits. The decision is to build the missing tokens
rather than accept the breakage or carve out exceptions.

**Answers:** `open-questions.md` Q48 (deleted from that file by this entry).
**Related:** §312 (privileges are computed from held tokens), §314 (the guesses
that could safely be deleted because the kernel checks again), `known-issues.md`
→ `TD-POSIX-CAPS-ARE-NOT-THE-KERNEL'S` step 3.

### What was decided

Three new kernel resource types, and the `kernel_view::project` rules that map
them onto the Linux capability names ported software asks about:

| New object | Grants | Feeds the Linux name | Gate site |
|---|---|---|---|
| system clock | setting absolute time and slewing it | `CAP_SYS_TIME` | `posix/src/time.rs` (`clock_settime`, `settimeofday`), `posix/src/sys_timex.rs` (`adjtimex`), `posix/src/epoll.rs` (`timerfd_create` with `TFD_TIMER_CANCEL_ON_SET`) |
| privileged ports | binding a local port below 1024 | `CAP_NET_BIND_SERVICE` | `posix/src/socket.rs` (`bind`) |
| resource limits | raising a *hard* limit; lowering stays free | `CAP_SYS_RESOURCE` | `posix/src/resource.rs` (`setrlimit`), `posix/src/mman.rs` (`check_mlock_caps`) |

Lowering a soft limit needs no token and never did — only raising the hard
ceiling is gated, which is the Unix rule and the one worth keeping.

### Why the operator's answer differs from the recommendation, and why that is right

The recommendation was to build objects for the clock and the limits but to
*drop* the privileged-port rule as a Unix relic: port 1024 protects a namespace
of numbers rather than a thing, and it exists because 1980s Unix had no better
way to say "this daemon is the real one." Linux itself now lets you set the
threshold to zero.

The operator took B for all three, and the argument for it is the stronger one
on reflection: option C is not actually "no policy", it is a *different* policy
— "everyone may bind port 80" — chosen once and unchangeable, whereas an object
is a policy *mechanism* that can express both. A system that hands the port
token to every process at boot behaves exactly like C, and can stop doing so
later without a code change. Dropping the check throws that away to save one
resource type. The relic argument also proves less than it seems: what is a
relic is the *number* 1024, not the idea that some ports are claimed by the
system, and an object lets us keep the idea while choosing any set of numbers we
like.

The cost of B over C is one more kernel resource type and one more boot-time
grant decision. That is small, and it is paid once.

### Consequences, and what happens next

- **A request to lane A** for the three resource types, since `kernel/**` is not
  lane B's to write. Until it lands, step 3 of §312 stays parked — which is the
  status quo and breaks nothing.
- **A boot-time grant policy is now needed and did not exist before.** Who holds
  the clock token: `init`? An NTP service and nothing else? This is a real
  question that B creates and C would have avoided for the port case. The
  default taken until someone decides otherwise: `init` holds all three and
  passes them down explicitly, because a token nobody holds is
  indistinguishable from option A.
- `sethostname` remains denied with no object, and is deliberately *not* in the
  table. §312 already refused to invent an object for it and nothing here
  changes that; if the same reasoning applies to it later, it is a fourth row
  rather than a revision of this entry.

**Revisit if:** the boot-time grant policy turns out to hand all three tokens to
every process, in which case the objects are ceremony and C's argument was right
after all. The way to tell is to look at what `init` actually does with them
once the kernel types exist.
