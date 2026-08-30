# Answers to your follow-up comments (2026-06-13)

You left comments in `todo2.txt` on Q1 (NUMA/UMA), Q2 (overcommit), and Q3
(toolchain ordering) and asked me to write my answers to an `.md` file. Here
they are, grouped by question. All three were already recorded as resolved in
`design-decisions.md` (§10, §11, §9) — but you asked real questions and floated
new options, so this fills in the explanations and reconciles your input with
what's already built.

**Bottom line up front:** none of this blocks current work. The kernel
mechanism for the memory-policy stuff is already implemented, the toolchain is
the agreed next initiative, and your option-4 / option-5 ideas for Q2 are
already in the roadmap. The open items below are *confirmations*, not blockers —
I'm continuing toward the toolchain regardless, and you can adjust any of these
whenever.

---

## Q1 — NUMA vs UMA, and the "option C" overhead

### The vocabulary first (you said these terms don't mean anything yet)

- **RAM "node":** a chunk of physical memory together with the CPU(s) it's
  physically wired closest to.

- **UMA = Uniform Memory Access.** *Every* CPU core reaches *every* byte of RAM
  at the *same* speed. This is what an ordinary desktop or laptop is: one CPU
  socket, one pool of RAM, one node. There is nothing to optimize about *which*
  RAM you use — it's all equally fast.

- **NUMA = Non-Uniform Memory Access.** A machine with **multiple CPU sockets**
  (two or four physical processors on one motherboard), or a few very large
  server chips with multiple internal memory controllers. RAM is split into
  nodes, each attached to one socket. A core reading its *local* node's RAM is
  fast; reaching *another* socket's RAM has to cross a slower interconnect
  (higher latency, sometimes less bandwidth). So *where* a thread's memory lives
  relative to the core running it suddenly matters for performance. This only
  exists on multi-socket servers/workstations — **not** on the desktop hardware
  we target.

- **VMA = Virtual Memory Area.** A single contiguous region of a process's
  address space with uniform properties — e.g. the code segment, the heap, or
  each individual `mmap`'d block is one VMA. A process is just a collection of
  VMAs. "Per-VMA memory policy" (option C) means storing, *for each such
  region*, a preference for which NUMA node should back it.

### What "being a UMA system" gets us vs "being NUMA-aware"

- **Advantage of UMA (treating ourselves as single-node):** *simplicity and zero
  overhead.* There's only one place memory can come from, so "which node?" is
  never a question. The allocator, the scheduler, and `fork` never have to track
  or preserve node preferences. On single-socket desktop hardware this isn't a
  shortcut — it's simply *correct*, because there genuinely is only one node.

- **Advantage of NUMA-awareness:** *only* pays off on multi-socket hardware,
  where keeping a thread's memory on its local node (and the thread on that
  node's cores) avoids the slow cross-socket path — sometimes a large win for
  big server workloads. On a single-socket desktop it buys **exactly nothing**,
  because there's no "remote" node to avoid.

So for our target hardware, UMA is the right model, full stop. NUMA-awareness
would be dead code that does nothing until/unless we ever support multi-socket
machines.

### "Option C creates CPU and memory overhead — how much?"

Option C was "store a real per-VMA memory policy so we can answer the NUMA
syscalls faithfully." The cost:

- **Memory:** a tiny policy object (a node bitmask + a mode, on the order of
  16–40 bytes) per VMA that has a non-default policy. A process has tens to
  low-hundreds of VMAs, so this is at most a few KB per process, and only for
  regions with an explicit policy. Small.

- **CPU:** the real cost isn't storage — it's **bookkeeping on every operation
  that splits or merges a VMA**: `mmap`, `munmap`, `mprotect`, `madvise`,
  `mremap`, and `fork` would each have to clone / split / free / refcount the
  policy object. That's a handful of extra pointer operations per affected
  syscall (tens of nanoseconds), plus permanent extra code complexity and
  bug-surface on some of the most-exercised paths in the kernel.

The honest framing: option C isn't *expensive* in a way that would hurt
performance — it's **fragile, never-exercised code for zero functional benefit
on single-node hardware.** Every VMA split/merge/fork path would have to carry
the policy correctly forever, and the entire payoff would be "a NUMA-tuning
syscall that almost nothing calls returns a slightly more literal error code."
On a one-node box the placement decision is always "the only node," so the
policy changes nothing about what any program computes or how fast it runs.

### Is there a better 4th option?

After the explanation above: not really, on UMA. The only behaviors that
*differ* between A and B are what a NUMA-tuning syscall reports to the <0.1% of
programs that call it:

- **(A) return success (0)** — what we do now. Keeps the common
  glibc/`libnuma` sequence (`mbind(...)` then `set_mempolicy_home_node`)
  quietly succeeding.
- **(B) return the literal Linux error (`-ENOENT`)** — "more faithful," but
  reports failure for a sequence real Linux *accepts*, which can trigger a
  "kernel lacks home-node support" warning or a degraded fallback path in a few
  NUMA-tuned server apps.

The genuine "fourth option" is the *trigger condition*, not a different
behavior today: **if SlateOS ever targets real multi-socket hardware**, implement
NUMA properly (real node tracking + placement) — and at that point per-VMA
policy (option C) comes along for free and *is* worth it. Until then there's no
multi-node to be aware of.

### "What's the end result on native and Linux programs? What % is affected?"

- **Native SlateOS programs:** **zero affected.** NUMA mempolicy is a Linux-ABI
  construct; native programs don't call these syscalls at all.
- **Linux programs:** only those using `libnuma` / `numactl` /
  `set_mempolicy_home_node` — i.e. **server software hand-tuned for
  multi-socket boxes**. That's **<0.1% of all programs and ~0% of desktop
  programs.**
- **Neither A nor B can crash a program or stop it starting.** The *only*
  observable difference is that B might cause a harmless warning log or a
  fallback code path in a handful of NUMA-tuned server apps; A keeps them
  quietly succeeding.

**My recommendation stands: keep A now** (it negatively affects the fewest
programs), and revisit with real NUMA only if multi-socket hardware ever
becomes a target. This is what `design-decisions.md §10` already records. If you
now want to change it after the explanation, just say so — it's a one-line
behavior change in `kernel/src/syscall/linux.rs`.

---

## Q2 — overcommit / memory-commit policy

### The vocabulary first

- **"Committing" memory** = the kernel *promising* that the RAM (or swap) you
  asked for really exists and is reserved for you. If a program asks for 1 GB
  and the kernel commits it, those pages are guaranteed to be there when the
  program touches them.

- **"Overcommit" / "lazy" allocation** = the kernel says **yes** to allocation
  requests *without* reserving the backing RAM up front. Pages only get real
  RAM when first *written* (demand paging). This lets programs allocate huge
  address ranges they never fully use — extremely common on Linux (`malloc`,
  `fork`, sparse arrays). The risk: if everyone actually touches their memory at
  once and there isn't enough, the system has over-promised and has to **kill
  something** (the OOM killer).

- **"Strict commit" (no overcommit)** = the kernel *refuses* an allocation it
  can't fully back. No OOM surprises — but programs that allocate-big-use-little
  (or `fork` a large process) can **fail to allocate, or refuse to start.**

Linux exposes this as `/proc/sys/vm/overcommit_memory`:

- **0 = heuristic overcommit** (Linux default): allow most overcommit, reject
  only obviously-insane single requests. **Almost all Linux software expects
  this.**
- **1 = always overcommit:** never refuse. Used by software doing huge sparse
  mappings (Redis fork-based persistence, some databases, some JVM/scientific
  workloads).
- **2 = strict / never overcommit:** total commitment capped at
  swap + a percentage of RAM; beyond that, allocations fail.

### Your point: "option A's biggest con is some apps refuse to start"

**Exactly right** — and it's the deciding factor. Option A (strict-commit
everywhere, no overcommit) is the *safest* against OOM kills, but it's the
*most* likely to make Linux software fail allocations or refuse to start,
because Linux software is written assuming overcommit (value 0). So A maximizes
safety but **minimizes compatibility** — the opposite of your stated priority
("maximize the number of programs that run without crashing; log noise is an
acceptable tradeoff"). That's why we didn't pick A as the global behavior.

### Your question on option C: "some readers treat it as an error — does the program not run? log noise is okay"

"Option C" in the original framing was **omit `/proc/sys/vm/overcommit_memory`
entirely** (don't expose the file). The worry was that software which reads that
file and finds it *missing* might log an error or take a fallback path.

- **Does the program fail to run?** Almost never. A missing `/proc/sys` file
  essentially never *stops* a program — it logs or assumes a default and
  continues. So the realistic worst case is exactly the "just log noise" you
  said is acceptable.
- **But here's the good news: this concern is now moot.** The implemented
  design does **not** omit the file. `/proc/sys/vm/overcommit_memory` **exists**
  and reports a sensible live value (`0` when the Linux default is
  lazy/overcommit, `2` when it's strict). So there's no missing-file noise to
  worry about — software reads it and gets a real answer.

### Your option 4 (per-program, user-modifiable, with a default that's also user-modifiable, and an explanation surfaced when the OS detects trouble)

This is already built and planned:

- **Per-program override** exists in the kernel today:
  `pcb::MmapCommitPolicy {Inherit, ForceCommitted, ForceLazy}`, consulted by
  both `mmap` paths, for both native and Linux programs.
- **The user-modifiable default** is the system-wide sysctl (per ABI — see
  below), changeable at runtime.
- **The "make the fix obvious when the OS detects a program having trouble"**
  idea is captured in `roadmap-detailed.md §5.8`: *"When the OS detects a
  program may be failing due to commit policy (allocation failures,
  refuse-to-start), surface a contextual hint pointing the user here with an
  explanation."* That's exactly your idea, already on the list.

### Your option 5 (build both strategies; configurable system-wide and per-program, for both Linux and non-Linux; advanced options in Settings with warnings)

**This is precisely what's now implemented and planned**, and it's what
`design-decisions.md §11` records as the chosen end-state:

- **Both strategies exist** in the kernel (strict-commit and lazy/overcommit),
  selectable.
- **System-wide, per ABI** (this was the refinement you suggested while I was
  working — "two selectors, one for native, one for Linux"):
  - **Native** → `sysctl mm.lazy_default`, default **strict-commit** (matches
    our design principle: "committed memory by default, lazy opt-in").
  - **Linux** → `sysctl mm.linux_lazy_default`, default **lazy/overcommit**
    (Linux programs expect it), surfaced as `/proc/sys/vm/overcommit_memory`
    (so `0` by default, exactly as you suggested).
- **Per-program override** for *both* ABIs (the `MmapCommitPolicy` above).
- **Settings UI as "advanced options with warnings"** is on the roadmap:
  system-wide selectors in `§5.6` (marked *Advanced + warning*), per-program
  override in `§5.8` (also *Advanced + warning*). Changing a program's *own*
  override is a normal user action; changing the *global* default needs the
  `admin.memory_policy` capability.

So your option 5 is the design, your per-ABI default suggestion is implemented,
and your option-4 "explain it when trouble is detected" idea is queued.

### The one thing still genuinely open: "what should the *default* default be?"

You're right that even with everything configurable, we still have to pick a
shipped default. My recommendation (and what's currently set), consistent with
the design spec's "committed memory by default, no silent overcommit":

- **Native default → strict-commit.** Matches the SlateOS principle; native apps
  are written for our ABI and can opt into lazy explicitly.
- **Linux default → lazy/overcommit (value 0).** Matches what Linux software
  expects; maximizes the number of Linux programs that run — your stated
  priority.
- Both **user-changeable** system-wide and per-program.

This is a low-risk default because it follows the design spec for native and
follows Linux's own default for Linux. **If you're happy with it, nothing
changes.** If you'd rather, say, ship native as lazy too (more permissive,
fewer native refuse-to-start cases at the cost of the "committed by default"
guarantee), that's a one-value change — tell me and I'll flip it.

---

## Q3 — toolchain ordering

Your decisions here are recorded in `design-decisions.md §9`:

- **Terminal / dev environment before the GUI stack** — agreed and recorded.
  Intuitive ordering: build the tools before the storefront.
- **GCC / CMake / Make toolchain before bash** — recorded as the next
  initiative (roadmap task 5031). One nuance I flagged: the dependency is
  *mostly* the other way around (Make/configure scripts are driven *by* a
  shell), but it doesn't actually block us, because the toolchain is
  **cross-built on the dev host** initially, not self-hosted on SlateOS — and we
  already have a kernel shell + coreutils. So toolchain-first is fine; a real
  `/bin/sh` (bash or a smaller POSIX sh) becomes the natural follow-on when we
  want `make`/`configure` running *on* SlateOS.

- **fastpy vs CPython — one correction.** Your comment suggested porting fastpy
  *before, or instead of,* CPython. fastpy is indeed our preferred *fast* Python
  (AOT-compiled, many times faster, kept CPython-3.14-compatible) — **but fastpy
  depends on the CPython runtime/DLL as a bridge** for operations it doesn't
  implement natively, most importantly **importing binary/compiled C-extension
  modules.** So fastpy can't run without CPython present. The order has to be
  **CPython first (it's both the prerequisite and the resident runtime bridge),
  then fastpy layered on top as the fast path.** Neither is ported yet. This is
  already corrected and recorded in `design-decisions.md §9`; I'm flagging it
  here because your `todo2.txt` note still reflected the earlier
  "fastpy-instead-of-CPython" framing. (If fastpy ever gains its own native
  binary-extension loader, the dependency could be dropped — but that's not the
  case today.)

---

## What I'm doing next

Per the agreed ordering (§9), the next initiative is the **GCC / CMake / Make
toolchain port** (roadmap task 5031), riding the already-mature POSIX layer. I'm
continuing on that — none of the above blocks it. The only items awaiting your
word are *confirmations* you can give whenever:

1. **Q1:** keep returning success (option A) for the NUMA syscalls? (recommended)
2. **Q2:** keep the shipped defaults — native strict-commit, Linux
   lazy/overcommit? (recommended)

If you want either changed, both are tiny edits.
