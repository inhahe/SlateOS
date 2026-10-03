## A-INTERMITTENT-STACK-CANARY-HALT-AT-REAP-TIME (lane A, 2026-08-17) - **ROOT CAUSE FOUND AND FIXED 2026-08-17**

**In short:** roughly one boot in ten dies with `FATAL: Stack canary
corrupted`, naming a task that exited tens of thousands of log lines
earlier, and halts the machine - throwing away the rest of the boot's
self-tests. Two very different faults produce that message and they want
opposite fixes, and the message as written could not tell them apart. The
halt itself was also wrong at that call site, and is fixed; the underlying
corruption is now instrumented to identify itself on the next occurrence.

**Symptom** (observed once in 74 boots' history; ~9.5% of boots fail for
some reason, this fingerprint is new):

```
[sched]   sleep_ns: PASSED (slept 21.563ms for 20ms request)
FATAL: Stack canary corrupted for task 100 (spawn-test-linux-sysv)!
  Expected: 0xdeadbeefcafebabe, Found: 0x0000000000000000
  stack_bottom=0xffffc10000004000, stack_top=0xffffc10000014000
FATAL: Kernel stack overflow is unrecoverable. Halting.
```

### What the evidence establishes

- The check fired from the **reaper** (`reap_dead_tasks`), not from a
  context switch: task 100 is not `current`, and the message lands
  immediately after another task exited and triggered a reap.
- Task 100 was created at serial line 1474 and **exited at line 1482**; the
  canary was not read until line **25349**. It stayed in the task table that
  whole time because the reaper skips any task still recorded as `current`
  on some CPU. So the corruption happened ~24000 lines before it was noticed.
- `stack_bottom = 0xffffc10000004000` decodes to **slot 0** of the kstack
  allocator (`KSTACK_REGION_BASE = 0xFFFF_C100_0000_0000`, `GUARD_SIZE =
  0x4000`) - the first slot the bitmap allocator hands out and the first it
  re-issues after a free.
- Task 100 predates the per-boot canary randomisation (line 1597), so
  `0xdeadbeefcafebabe` is genuinely the value that was planted.
- `stack_bottom` was non-zero, so `free_stack()` - which zeroes it - had not
  run on that `Task`.

### The two candidate causes

| | **Real marginal overflow** | **Stale reference to a recycled slot** |
|---|---|---|
| What happened | the task's own frames reached the bottom of its 64 KiB stack and overwrote the canary | the slot was freed and re-issued; `kstack::alloc` memsets the whole stack, and this `Task` still pointed at it |
| Where the bug is | the deep path, or `TASK_STACK_SIZE` | the stack free path |

**The guard page does not rule out the first.** The guard sits *below*
`stack_bottom`, so a write landing exactly on the canary corrupts it without
ever leaving the mapped stack - which is precisely the case the canary
exists to catch. Reading exactly zero is consistent with a zero-initialised
local buffer or a `write_bytes` reaching the bottom eight bytes.

The second is much weaker than it first looks: `kstack::alloc` zeroes the
stack, but `Task::new_kernel` then plants a fresh (randomised, non-zero)
canary a few instructions later. A stale read would therefore have to land
inside that window to see zero rather than the *new* task's canary. Possible,
but it requires a coincidence that a 24000-line-later reap does not offer.

Intermittency fits the first cause well: the code path is deterministic, but
an interrupt taken near the deepest point pushes an IRET frame, a register
save and the handler's own frames onto the *same* kernel stack. `spawn_process`
-> ELF parse -> page-table work is among the deepest paths in the kernel.

### What was fixed now

1. **The reaper no longer halts.** Its comment already said *"the task is
   already dead so we can't halt"* while calling `check_stack_canary()`,
   which halts unconditionally - intent and behaviour had silently disagreed.
   Split into two:
   - `check_stack_canary()` - still halts. Correct for the context-switch
     callers, where a live task is about to resume on a stack known to be bad.
   - `report_stack_canary() -> bool` - diagnoses and returns. Correct for the
     reaper, where the task is dead and already removed from the table, so a
     halt buys no safety and costs the rest of the boot's diagnostics.

2. **The failure now identifies its own cause.** The post-mortem prints the
   stack watermark, the kstack slot, and the composition (zero / sentinel /
   other words) of the bottom 512 bytes *and* the top 512 bytes. That last
   pair is the discriminator: a real overflow leaves the top of the stack
   full of ordinary frame data, whereas a recycled slot has been zeroed or
   repainted end to end. It prints an explicit `VERDICT:` line either way.

3. **A system-wide stack census now runs every boot** (`report_stack_census`,
   last in the scheduler self-test). It reports the five deepest kernel
   stacks and warns above 75%. This is the measurement whose absence made the
   bug undiagnosable: `test_stack_watermark` proved the watermark *API*
   worked, but only ever measured a purpose-built task that touches 256
   bytes, so "is any real kernel stack close to overflowing?" was a question
   nothing in the tree could answer - despite every stack already being
   painted with a sentinel that answers it for free.

### First census results (2026-08-17) - the headroom is smaller than assumed

The census ran on the next green boot and answered the question directly:

```
[sched]   Stack peak this boot: 43896 bytes (66% of 65536) by task 283 (spawn-test-glibc-forkexec)
[sched]   Stack census: 5 live task(s) with allocated stacks, deepest first
[sched]     task 397  kworker                   42424 bytes ( 64% of 65536)
[sched]     task 396  kswapd                     5048 bytes (  7% of 65536)
[sched]     task 406  efd-to-test                4904 bytes (  7% of 65536)
[sched]     task 407  svc-accept                 4552 bytes (  6% of 65536)
[sched]     task 408  cgroup-e2e                 4200 bytes (  6% of 65536)
```

Three things in that are worth reading carefully.

**The deepest stack of the boot belonged to a task that was already dead** -
task 283 was reaped long before the census ran, and is visible only because
the reaper folds each dying task's watermark into the peak before freeing its
stack. A census of live tasks alone would have reported 64% and missed the
real maximum. This is the half that makes the instrument honest, and it is
also the half that was easiest to leave out.

**`kworker` sits at 64% at steady state, and it is long-lived.** The peak is
not a one-off spike in a short-lived spawn task; a permanent kernel worker is
routinely two-thirds of the way down its stack. The distance from there to the
canary is about 23 KiB.

**The deep tasks are the spawn family** - `spawn-test-glibc-forkexec` at the
peak, and `spawn-test-linux-sysv` is the task whose canary failed. Same family
of paths (`spawn_process` -> ELF parse -> page-table work), which is what
hypothesis (a) predicted.

What this does *not* yet do is prove (a). 23 KiB is a large amount for one
interrupt to consume, so a single badly-timed IRQ at `kworker`'s depth does not
obviously reach the canary; nested interrupts, or a spawn path deeper than any
seen in these boots, would be needed. The census will show that as a rising
number, which is the point of printing it every boot: the next occurrence now
arrives with both a `VERDICT:` line and a depth history to compare against.

No task crossed the 75% warning line on this boot, so the threshold has not yet
been exercised in anger.

### Generalisation

Two rules fell out of this one.

**An assertion that halts must be sited where halting helps.** The same
canary check was correct at the context-switch callers and wrong at the
reaper, for the same reason in both cases: whether anything is going to
*run* on that stack again. A check copied to a second call site inherits its
severity, and severity is a property of the site, not of the condition.

**When a diagnostic fires intermittently, the first fix is to make the
diagnostic conclusive, not to guess at the cause.** Both hypotheses here are
plausible, they want opposite fixes, and picking one on a hunch had an even
chance of hardening the wrong path while leaving the real one live. The
evidence needed to choose was cheap - it was sitting unread in a sentinel
pattern the kernel already paints on every stack.

---

### RESOLVED 2026-08-17 - hypothesis (a) was right, and the cause was one field

Everything above stands as written except its conclusion.  Hypothesis (a)
(a real marginal overflow on the spawn path) is now **proven**, and the
reason 23 KiB of headroom was not in fact enough is a single struct field.

**In short:** `Task` embedded its 4096-byte FPU save area *by value*.  That
made `Task` a ~4.4 KiB type, and in Rust a by-value move of a large type is
a `memcpy` through a stack temporary.  The spawn path performs several such
moves back to back, and at `opt-level = 0` - which is what
`scripts/boot-test.sh` builds by default, and the profile every observed
halt came from - the compiler elides none of them.  Three frames on one
call chain therefore claimed **40 640 bytes of a 64 KiB stack** for nothing
but copies of a zeroed 4 KiB array.

#### How it was found

A watermark says *which task* came closest to its canary.  It structurally
cannot say *which function* put it there.  That gap is what kept the bug
alive for as long as it did, and it is now closed by a new tool,
**`scripts/stack-frames.py`**, which disassembles the built kernel and
reports, per function, the stack its prologue claims.

Two traps in writing that tool are worth recording, because either one
alone would have produced a confident wrong answer:

1. **Stack-probe chains.**  A function needing more than a page does not
   emit one `sub $N, %rsp`.  It emits `sub $0x1000, %rsp; movq $0, (%rsp)`
   repeated a page at a time, so a guard page can never be jumped over
   untouched.  A naive "first `sub` in the prologue" reading therefore
   reports **exactly 4096 for every large function** - hiding precisely the
   ones worth finding.  The first version of the tool did exactly this, and
   the uniform 4096s looked like a clean bill of health.  The chain must be
   summed.
2. **Profile matters enormously.**  Measuring the release build and
   concluding the kernel is fine is a real way to be wrong here: the same
   spawn path measured **40 640 bytes in debug and 8 960 in release**.

#### The measurement

Static, debug profile, before and after boxing `FpuState`:

| Frame | before | after |
|---|---:|---:|
| `FpuState::new_default` | 8 320 | off path |
| `Task::new_kernel` | 9 600 | 1 256 |
| `sched::spawn_inner::{{closure}}` | 22 720 | 2 000 |
| **total on one call chain** | **40 640** | **3 256** |

40 640 bytes is **93% of the 43 896-byte peak the census measured**, in
three frames.  There was never 23 KiB of headroom to spend on an unlucky
interrupt; the frames themselves had already spent it.

Two independent confirmations, taken before any fix:

- **Static:** the identical source measured 40 640 in debug and 8 960 in
  release - a 4.5x profile gap that only a by-value-copy explanation
  accounts for.
- **Runtime:** booting release instead of debug moved the census peak from
  43 896 (66%) to 13 560 (20%), and `kworker` from 42 424 (64%) to
  10 200 (15%).

#### The fix

`Task::fpu_state` is now `Box<FpuState>`, built by a new
`FpuState::new_default_boxed()` that allocates zeroed memory and patches
the 14 non-zero bytes **in place**.  This detail is the whole point:
`Box::new(FpuState::new_default())` would have fixed *nothing*, because it
builds the value in the caller's frame and only then copies it to the heap.
Only allocate-then-initialise-in-place keeps it off the stack.

A second instance of the same defect was found by the same tool and fixed
in the same way: `KernelFdTable` is `[Option<FdEntry>; 256]` = 8192 bytes,
and was likewise built by value and then `Box::new`'d, at two sites.  It
now has `new_boxed()` / `with_stdio_boxed()`.  (`alloc_zeroed` is
deliberately *not* used there - `Option<FdEntry>`'s `None` is not
guaranteed to be all-zero bytes, a layout detail Rust does not promise.
`Box::new_uninit()` plus an explicit per-element `write(None)` is.)

Together the two fixes removed roughly **85 KiB of stack claims across nine
functions**:

| Function | before | after |
|---|---:|---:|
| `sched::spawn_inner::{{closure}}` | 22 720 | 2 000 |
| `proc::pcb::fork_create::{{closure}}` | 16 624 | 0 |
| `Task::new_kernel` | 9 600 | 1 256 |
| `sched::init` | 9 344 | 0 |
| `sched::register_ap_idle` | 9 216 | 0 |
| `proc::pcb::linux_fd_install_stdio` | 8 336 | 0 |
| `Result::<Task, _>::branch` | 4 608 | 0 |
| `Task::new_ap_idle` | 4 352 | 0 |
| `Task::new_idle` | 4 352 | 0 |

`linux_fd_install_stdio` also stopped allocating while holding
`PROCESS_TABLE`'s spinlock, which is a separate latent bug (see the Q24
allocation-under-spinlock sweep) fixed incidentally by the same change.

#### Runtime confirmation

Debug boot, after both fixes (boot 81, PASS, streak 8):

```
[sched]   Stack peak this boot: 38280 bytes (58% of 65536) by task 283 (spawn-test-glibc-forkexec)
[sched]   Stack census: 5 live task(s) with allocated stacks, deepest first
[sched]     task 397  kworker                    7664 bytes ( 11% of 65536)
[sched]     task 396  kswapd                     5048 bytes (  7% of 65536)
[sched]     task 406  efd-to-test                4904 bytes (  7% of 65536)
[sched]     task 407  svc-accept                 4552 bytes (  6% of 65536)
[sched]     task 408  cgroup-e2e                 4200 bytes (  6% of 65536)
```

`kworker` - the long-lived task that sat at **64%** for the whole boot, and
the one whose 23 KiB of remaining headroom the entry above worried about -
is now at **11%**.  That is 34.8 KiB freed on a permanent kernel thread, a
5.5x drop, and headroom from 23 KiB to 57 KiB.  This is the number that
closes the bug: `spawn_inner` runs on the *caller's* stack, and the caller
is `kworker`.

The **peak** moved less - 43 896 to 38 280 - and this is expected rather
than disappointing.  The peak belongs to task 283's own stack, so it
measures code running *inside* the spawned task, not the spawn machinery.
See the follow-on entry below for what is still down there.

#### Generalisation (added to the two rules above)

**A large struct destined for the heap must never be built by value.**
This is recorded as `design-decisions.md` §226 with the pattern to use.
The rule has teeth because the failure is invisible in release builds and
invisible in code review: `Box::new(Big::new())` reads exactly like
heap allocation, and is not.

**A watermark localises a symptom to a task; only static frame analysis
localises it to a function.** The census was a real improvement and still
could not have found this. `scripts/stack-frames.py` is now part of the
tree; run it after any change to a spawn, fork, or context-switch path.
