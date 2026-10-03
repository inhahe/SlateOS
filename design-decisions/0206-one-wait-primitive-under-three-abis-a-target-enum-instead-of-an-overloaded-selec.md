## §206 — One wait primitive under three ABIs, a target *enum* instead of an overloaded selector, and an extensible out-parameter for the facts that do not survive the reap

**Date:** 2026-08-16
**Decided by:** Claude (autonomous) — in response to lane B's request
`requests/b-a-waitid-needs-an-explicit-idtype-wait.md`, which asked for three
specific capabilities; the shape chosen to deliver them is mine.
**Lane:** A

**In short:** three different syscalls let a program wait for its child process
to finish — one for programs built for our own system, and two more for
programs ported from Linux. All three were separately written copies of the
same logic, and they had drifted apart: one of them could not be interrupted by
a Ctrl-C at all, none of them could say "wait for the processes in group 1",
and none could tell you the child's user ID or how much CPU it used, because by
the time they answered, the child's records had already been deleted. This
change merges the three copies into one and adds the missing facts to the
answer. The choices worth recording are: one shared body rather than three;
naming *what kind* of thing you are waiting on with a separate field instead of
squeezing it into the sign of a number; and returning the extra facts in a
caller-sized structure that can grow later without needing a new syscall.

### The decision

1. **One primitive.** `kernel/src/syscall/wait.rs` owns child selection, the
   reap, and the blocking loop. `wait4`, `waitid` and the native
   `SYS_PROCESS_WAIT_STATUS` are marshalling layers over it: `wait4` and the
   native syscall encode a POSIX `wstatus` word, `waitid` encodes a
   `siginfo_t`, and none of them decides *which* child.
2. **`WaitTarget { Any, Pid, Pgid }`** replaces the overloaded signed selector
   inside the kernel. The `waitpid`-shaped callers convert at the boundary with
   `WaitTarget::from_posix_selector`; callers that can say what they mean
   (`waitid`'s `P_PGID`, the native `WPGID` bit) construct `Pgid` directly.
3. **Native option bits `WPGID` (1<<16) and `WNOWAIT` (1<<24)**, both
   previously rejected by the options mask, so no existing caller changes.
4. **A `WaitInfo` out-parameter** (`arg3` pointer, `arg4` size, 72 bytes today)
   carrying pid, uid, wstatus and six usage counters, written under the
   `clone3`/`sched_setattr` convention: the kernel writes `min(caller, kernel)`
   bytes and zero-fills any tail it does not know about.

### Why one primitive, and what the two extra copies had already cost

The three entry points read the *same* records — process groups, zombie exit
info, job-control reports. If they disagree about which child a selector names,
which transition is reported first, or whether a report was consumed, then a
program on our libc and a ported glibc program waiting on the same child
observe different histories. That is not hypothetical: `sys_waitid` had **no
signal handling whatsoever** — no pending-signal check, no `ERESTARTSYS`, no
signalfd registration, just `block_current()` — while `wait4` a few hundred
lines away had all three. A process blocked in `waitid` could not be
interrupted. Nobody decided that; it is what two copies of one primitive decay
into.

*Against:* a shared body is a shared blast radius — a bug in `scan_once` is now
a bug in three ABIs at once, and the three callers can no longer be tuned
independently. *For:* they were never *supposed* to differ, and the one place
they did differ was a defect. A shared bug that shows up in three tests is
strictly easier to find than three subtly different behaviours that each look
locally reasonable.

### Why a target enum rather than "just add a flag"

`waitpid`'s single `pid_t` overloads four meanings onto one integer (`>0` a
pid, `0` my group, `-1` any child, `<-1` group `-pid`), which cannot name
process group 1: that would need `-1`, already spoken for. This is lane B's
item 1, and it is a real user-visible hole — an init or a shell most wants to
wait on exactly that group.

Linux does not patch the encoding; it never overloads internally, resolving
`sys_waitid`'s arguments to a `(type, struct pid *)` pair before calling
`do_wait`. `WaitTarget` is that pair. The alternative — keep the signed
selector everywhere and special-case group 1 with a sentinel — was rejected
because a sentinel is exactly how the hole was made in the first place, and the
next un-nameable value would need another one.

*Cost:* every caller now converts, and `from_posix_selector` is one more place
a mistake can live. *Benefit:* the conversion is total — there is no encoding a
caller can pass that means two things at once — and the ABI-specific gate that
rejects `INT_MIN` stays at the syscall boundary where it belongs, rather than
becoming a property of waiting itself.

### A second bug the consolidation surfaced: registration was answering a question it could not see

The any-child/group branch of the blocking wait registered the waiter *before*
scanning, and returned the registration error as the answer:

```rust
// "Registering first ... also reports ECHILD when the caller has no children at all."
if let Err(e) = pcb::set_wait_any_task(parent_pid, task_id) { return Err(e); }
```

That comment's second clause was false. `set_wait_any_task` looks up **the
caller** and fails with `NoSuchProcess` when the caller has no PCB — it never
examines children at all. So a registration failure was standing in for ECHILD,
and every any-child and every group wait from a caller without a process record
returned ECHILD without scanning.

It survived because the two pre-existing tests both miss that branch: the
group-filter test goes through `SYS_PROCESS_TRY_WAIT`, which calls `scan_once`
directly, and the job-control test names a pid, taking the branch that scans
*before* registering. That asymmetry between the two branches was itself the
tell, and is only visible once both live in one function.

The fix separates the two questions: registration failure is recorded rather
than returned, `scan_once` answers ECHILD from `!has_child` as it already
could, and only a caller that would actually have to **park** is defeated by
having no wake slot — parking without one is a permanent hang, so that case
returns `NoSuchProcess`, which `wait_err_to_echild` folds to ECHILD at the
boundary. That guard is load-bearing rather than defensive: `linux.rs`'s wait4
register-truncation test deliberately passes an `options` word that truncates
to `0` — no `WNOHANG` — on a wait-any, and is the one caller in the tree that
reaches the park point with no PCB.

### Why an extensible struct rather than a second return register

Lane B's items 3 and 4 (`si_uid`, CPU times) share the property that decides
the shape: **they do not survive the reap.** Once the PCB is destroyed there is
no one left to ask, so a follow-up query syscall is not merely inelegant, it is
impossible. The facts must ride out on the wait itself.

Two ways to do that were considered:

- **A second return register** (`syscall3_2ret` already exists on the libc
  side). Cheap, and enough for `si_uid` alone. Rejected because it is enough
  for *exactly* `si_uid`: the six usage counters do not fit, and lane B
  explicitly noted that leaving room for CPU time now saves a second ABI change
  later. A one-off register would have made that second change mandatory.
- **A caller-sized struct.** Chosen. An older caller on a newer kernel passes a
  smaller size and gets the prefix it understands, never a write past its
  buffer; a newer caller on this kernel passes a larger size and gets zeros in
  the fields this kernel cannot fill — which is the *correct* answer for every
  counter here, since an unknown count reads as none rather than as garbage.
  Size 0 (or a null pointer) means "don't want it", so the cost to callers that
  never use it is two zero registers.

*Against the struct:* it is a userspace layout the kernel writes field by
field, which is the larger trust boundary §112 cited when it deferred a
`siginfo_t`-shaped syscall. *Why that objection does not bite here:* §112's
concern was a 128-byte layout with a union whose interpretation depends on
`si_code`. This is a flat array of `u64`s — no union, no pointers, nothing
variable-length — written through the same `copy_to_user` as every other
out-parameter, and validated **before** the wait blocks as well as inside the
write, because a caller that passed a bad pointer should learn so now, not
after sleeping for an arbitrarily long wait and reaping a child it then cannot
be told about.

### The out-parameter must be opt-in — an extension bit, not just a size

**In short:** you cannot start reading a syscall argument register that the
callers who already exist never write. The first version of this change did,
and it turned `waitpid` into a wild pointer.

`SYS_PROCESS_WAIT_STATUS` shipped taking three arguments. Every caller it has
therefore reaches it through a three-argument wrapper — `posix`'s `syscall3`
emits `in("rdi")`, `in("rsi")`, `in("rdx")` and nothing else, so `r10` and `r8`
arrive holding whatever the caller last left in them. Reading them as
`(ptr, size)` meant the kernel validated a pointer userspace never supplied,
and on the success path would have written 72 bytes through it. `ctest-pgroup`,
`ctest-jobctl` and `ctest-ctty` all failed inside one boot; the EFAULT they saw
was the lucky outcome, not the characteristic one.

So `arg3`/`arg4` are read only under a new option bit, `WINFO = 0x0002_0000`.

*The tempting wrong answer* is that the size field already covers this —
`arg4 == 0` means "don't want it", so surely an old caller is safe. It is not:
garbage is zero with probability 2⁻⁶⁴. **The size convention protects against a
struct that grew, not against a register that was never written**, and those
are different failures with different remedies. The option word is the only
argument every existing caller demonstrably sets, so an option bit is the only
place an "I am new enough to have filled in `r10`" signal can live.

*Against the bit:* it spends option space on something the size field
superficially seems to cover, and a caller must now set two things
(bit + pointer) instead of one. *For it:* the alternative is unsound, and the
generalisation is worth stating plainly for the next syscall that grows —
**`number.rs` already carried this exact warning about `SYS_PROCESS_WAIT`'s
callers leaving garbage in the argument registers, and this change failed to
apply it to the syscall doing the growing.** Any syscall gaining an argument
beyond the arity it shipped with needs an in-band opt-in, not just a
self-describing payload.

### The one unit that is deliberately inconsistent

`WaitInfo` carries CPU time in **microseconds**; `waitid`'s `si_utime` /
`si_stime` carry it in **USER_HZ ticks**. That looks like an oversight and is
not. `siginfo_t`'s fields are `clock_t` and Linux fills them via
`nsec_to_clock_t`, so any conversion applied there would make ported binaries
wrong — no conversion is right, and any conversion would be wrong. `WaitInfo`
has no such constraint, so it uses the unit that does not require the caller to
know `USER_HZ`; a caller that guessed wrong would be off by a factor of ten
with nothing to notice it by. The tick-to-microsecond factor lives in exactly
one place (`US_PER_TICK` in `wait_info_image`), next to a note pointing at
`sys_getrusage` — the other consumer of the same counters, which must not
disagree.

### A premise in the request that was already false

Lane B's item 4 states "There is no per-process CPU accounting, so libc zeroes
all of it." That was true when `wait4`'s `clear_user_rusage` was written and is
not true now: `pcb`'s `acct_*` / `child_*` counters and
`thread::process_cpu_ticks` / `process_fault_counts` / `process_ctxsw_counts`
have existed for a while, and `sys_getrusage` already encodes all 144 bytes
from them. `clear_user_rusage` was a **false zero** — the data was there and
the call threw it away. `wait4` now reports it, so item 4 is closed rather than
deferred.

### How to reverse

`wait.rs` is additive: the three callers could be given back private copies by
inlining `scan_once` / `wait_for_child_event`, at the cost of reintroducing
exactly the drift described above. The two option bits and the out-parameter
are pure extensions — removing them returns the syscall to its previous
behaviour for every existing caller, since all three inputs were previously
rejected or ignored.

### Where it lives

- `kernel/src/syscall/wait.rs` — the whole primitive.
- `kernel/src/syscall/handlers.rs` — `wait_opt`, `WAIT_INFO_SIZE`,
  `wait_info_image`, `write_wait_info`, `sys_process_wait_status`,
  `sys_process_wait`, `sys_process_try_wait`.
- `kernel/src/syscall/linux.rs` — `sys_wait4`, `write_user_rusage`,
  `sys_waitid`, `waitid_scan`, `waitid_siginfo`.
- `kernel/src/proc/thread.rs` — `ProcessUsage`, `process_usage_both`.
- Tests: `dispatch.rs` — `test_dispatch_wait_status_wpgid_and_wnowait`,
  `test_dispatch_wait_info_layout`, `test_dispatch_wait_process_group_filter`;
  `linux.rs` — `test_waitid_scan`.
