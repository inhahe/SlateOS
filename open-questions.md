# Open Questions — Operator Decision Queue

Decisions that genuinely need the human operator: architectural forks,
user-visible policies, and tradeoffs with no obviously-correct answer that
Claude has **deferred** rather than resolved autonomously.

This file is distinct from:

- **`design-decisions.md`** — decisions already *made* (each marked with who
  decided it). When the operator answers a question here, move it there as a
  `Decided by: Operator` entry and delete it from this file.
- **`known-issues.md`** — bugs and accumulated technical debt.
- **`todo.txt`** — the working scratchpad / judgment-call log.

Format for each entry:

- **Question** — the decision to be made.
- **Options** — each with its pros and cons.
- **Claude's recommendation** — if there is a defensible default (and what
  Claude is doing in the meantime).
- **Where it bites** — files/symbols affected, so the resolution can be applied.
- **Status** — `OPEN` until the operator decides.

---

(The coreutils "which set is canonical?" question was resolved on 2026-06-12 —
standalone per-tool crates are canonical; see `design-decisions.md` §8.)

---

### Q1. Per-VMA mempolicy storage — and the `set_mempolicy_home_node` 0-vs-`-ENOENT` choice

- **Question** — Should the kernel implement real per-VMA NUMA mempolicy
  storage, or keep treating `mbind`/`set_mempolicy` as UMA no-ops? This drives
  the return value of `set_mempolicy_home_node` on a valid non-empty range.
- **Background** — We are a single-node (UMA) system, so NUMA policy has no
  functional effect; `mbind` currently accepts and drops the policy (returns
  0). Linux's `set_mempolicy_home_node` walks the range's VMAs and returns
  `-ENOENT` when none has an explicit `MPOL_BIND`/`MPOL_PREFERRED_MANY` policy,
  `-EOPNOTSUPP` for a wrong-mode policy, or 0 once a bind policy is found.
  Without per-VMA policy storage we can't distinguish these cases.
- **Options**
  - **(A) Keep UMA no-op, return 0** *(current)* — pro: matches the common
    real-world path (`mbind(MPOL_BIND)` then `set_mempolicy_home_node` → 0);
    libnuma/glibc see success. con: returns 0 where Linux returns `-ENOENT`
    for a default-policy range; not fully faithful.
  - **(B) Keep UMA no-op, return `-ENOENT`** — pro: matches the "no explicit
    policy" path literally. con: breaks the common post-`mbind` success path
    (we'd report failure for a sequence Linux accepts); glibc would log
    "kernel lacks home-node" warnings.
  - **(C) Implement per-VMA mempolicy storage** — pro: fully faithful errno
    discrimination for the whole mempolicy family. con: substantial machinery
    (per-VMA policy objects, mbind_range, mpol_dup) for zero functional effect
    on a UMA system.
- **Claude's recommendation** — Stay on **(A)** for now (done). Only pursue
  **(C)** if a real multi-node target appears or an app actually depends on the
  `-ENOENT` discrimination. Documented as `known-issues.md` TD7.
- **Where it bites** — `kernel/src/syscall/linux.rs`:
  `sys_set_mempolicy_home_node`, `sys_mbind`, `sys_set_mempolicy`,
  `sys_get_mempolicy` (the empty-mask/default-policy answers).
- **Status** — OPEN

#### Answers to the operator's questions (2026-06-13)

**Plain-language primer (UMA vs NUMA vs VMA).**

- **RAM and "distance" to the CPU.** A modern machine can have its RAM wired up
  in one of two layouts. In a **UMA** (Uniform Memory Access) machine, every CPU
  core reaches every byte of RAM at the *same* speed — there is one pool of
  memory and no core is "closer" to part of it. Essentially all desktops,
  laptops, and single-socket servers are UMA. In a **NUMA** (Non-Uniform Memory
  Access) machine, the hardware is split into **nodes** — each node is a group of
  CPU cores bundled with its own bank of RAM. A core reads its *own* node's RAM
  quickly ("local") but reaching *another* node's RAM is slower because the
  request has to cross a chip-to-chip interconnect ("remote"). NUMA only appears
  on big iron: multi-socket servers (two or more physical CPU chips) and some
  high-core-count workstation chips. **OuRoS targets desktops, so we are UMA: one
  pool, one speed, no near/far distinction.**

- **Why a program would ever care.** On a NUMA box, a performance-sensitive app
  (a database, a scientific simulation, a JVM tuned for big servers) can ask the
  OS: "please keep *this* chunk of my memory on the *same* node as the threads
  that use it, so accesses stay local and fast." The Linux calls for expressing
  that wish are `mbind`, `set_mempolicy`, and `set_mempolicy_home_node` — the
  "NUMA mempolicy" family. This is a *hint about placement*; it changes *where*
  pages physically live, never *what* the program computes.

- **What a VMA is.** A running program's address space isn't one flat blob — it's
  a series of labelled regions: the code, the heap, each memory-mapped file, each
  thread's stack, and so on. Each such region is a **VMA** (Virtual Memory Area):
  a contiguous span of addresses that share the same properties (permissions,
  backing file, and — on Linux — NUMA placement policy). It's the unit Linux uses
  to track "this stretch of memory behaves like *this*." You can see a process's
  VMAs in `/proc/<pid>/maps`.

- **What option C actually is.** "Per-VMA mempolicy storage" means: attach a
  little NUMA-placement record to *each* VMA, so the kernel can remember "the
  caller asked for node 2 on this particular region." Linux needs this because on
  real NUMA hardware the policy has teeth. **On UMA hardware there is only one
  node, so any placement policy resolves to "use the one and only pool" — the
  record would be stored, dutifully maintained across every memory operation, and
  then never change any actual behavior.**

**Advantages of being UMA (your first question).** This isn't a choice we make —
it's a property of the desktop hardware we target — but the *consequences* are
all upside for us:

- **Simpler, faster memory manager.** No node-aware allocator, no per-node free
  lists, no "fall back to a remote node when the local one is full" logic, no
  inter-node page migration, no NUMA balancing daemon. Linux carries thousands of
  lines for all of this; a UMA kernel needs none of it.
- **No placement decisions on the hot path.** Every page allocation just pulls
  from the single pool. On NUMA the allocator must first decide *which* node,
  consult the policy, and maybe migrate — overhead on an operation that runs
  constantly.
- **Predictable performance.** Memory latency is uniform, so there's no
  "accidentally slow" memory and no tuning burden. NUMA's whole reason to exist
  is to *recover* the performance it costs you when placement goes wrong.
- The flip side — NUMA's *only* advantage — is that it lets you build machines
  with far more total RAM and cores than a single memory bus can serve. That
  scaling matters for 2-socket+ servers; it is irrelevant to a desktop OS.

**How much overhead does option C add (your second question)?** Three kinds, and
the important one is the third:

- **Runtime CPU: effectively zero.** The policy record is only ever consulted
  inside the NUMA syscalls themselves (`mbind`/`set_mempolicy`/…), which a normal
  program never calls. It is *not* touched on the page-allocation hot path on a
  UMA system, because there's only one node to allocate from regardless. So
  day-to-day CPU cost ≈ 0.
- **Memory: tiny.** One extra pointer-sized field per VMA, normally null (no
  policy set). A policy object is allocated only for the rare VMA that actually
  has an explicit policy. For virtually every process the added memory is a
  handful of null pointers — negligible.
- **Code complexity: the real cost, and it's significant.** This is the reason
  not to do it. Per-VMA policy means every operation that *splits* or *merges*
  VMAs (`mmap`, `munmap`, `mprotect`, `madvise`, `mremap`) has to correctly
  duplicate/split/merge the attached policy too; `fork` has to deep-copy policies
  (`mpol_dup`); and the whole `mbind_range` machinery has to exist. That's a
  meaningful, bug-prone chunk of kernel code **whose entire payoff is faithful
  errno values on syscalls almost nothing calls, with zero effect on what any
  program computes or how fast it runs** — because we're UMA.

**Is there a better 4th option (you couldn't judge, so here's mine)?** No — and
that's a genuine conclusion, not a dodge. The whole question only has stakes on
NUMA hardware we don't target. The realistic choices collapse to "how honest is
the errno on a syscall almost nobody calls," and option A already picks the
answer that keeps the *common* sequence working. A 4th option would just be a
different shade of errno bikeshedding. If OuRoS ever targets real multi-node
servers, the correct move isn't a clever 4th option — it's to implement option C
*properly* (real placement, not just storage), at which point the errnos come
for free.

**End-results of A vs B, and what % of programs are affected (your third
question).** The split is tiny and low-stakes:

- **Who is affected at all:** only programs that call `set_mempolicy_home_node`,
  a NUMA-tuning syscall added in Linux 5.17 (2022). In practice that's a short
  list of server software explicitly tuned for multi-socket boxes (some database
  and big-JVM deployments) and the `numactl`/`libnuma` tooling. **A normal
  desktop program — a browser, an editor, a shell, a game, a compiler — never
  calls it.** Realistic impact: **well under 0.1% of programs**, and ~0% of
  desktop programs.
- **Native OuRoS programs:** unaffected entirely. NUMA mempolicy is a Linux-ABI
  construct; native code doesn't use it.
- **For the few Linux programs that do call it:**
  - **Option A (return 0, current):** we report "your placement request
    succeeded." Since there's one node, the request is trivially satisfied. The
    common real-world sequence — `mbind(MPOL_BIND)` then
    `set_mempolicy_home_node` — sees success, which is exactly what
    glibc/libnuma expect; they proceed normally. Worst case: a program *thinks*
    it pinned memory to a node that doesn't separately exist, which on UMA is
    harmless (there's nothing to pin it away from).
  - **Option B (return `-ENOENT`):** we report "no policy found for this range."
    This is the literal Linux answer for a *default-policy* range, but it
    **breaks the common post-`mbind` success path** — a program that just set a
    bind policy and asks to confirm the home node gets a failure for a sequence
    Linux would have accepted. libnuma/glibc may then log "kernel lacks home-node
    support" warnings or take a degraded fallback path.
  - **Net:** A keeps more Linux programs on their happy path; B is "more literal"
    only for a case that, on UMA, has no practical consequence. Neither A nor B
    makes any program *crash* or *fail to start* — at most B causes a warning
    log or a slightly different (still-functional) code path in NUMA-tuning
    software.

**Claude's recommendation (restated for the answer):** keep **option A** and
**close this question** — adopt it as settled rather than leaving it open. The
stakes are negligible on desktop (UMA) hardware: A maximizes Linux-program
compatibility, costs nothing, and the only "more faithful" alternative (C) is a
real pile of fragile code for zero functional benefit until/unless OuRoS targets
multi-socket servers. If that day ever comes, the trigger is "implement C
properly," and the errno question answers itself. **Suggested resolution: adopt
A, record it in `design-decisions.md` as the operator's call, and remove Q1 from
this file.** (Leaving as OPEN pending your nod, since you asked me to lay out the
reasoning first.)

---

### Q2. Should `/proc/sys/vm/overcommit_memory` (and the `vm/` tree) be exposed, and at what value?

- **Question** — The new `/proc/sys` sysctl tree (procfs.rs, task 5092)
  deliberately omits the `vm/` subtree. The first candidate is
  `vm/overcommit_memory`. Should we expose it, and if so report which value?
- **Background** — `design.txt`/CLAUDE.md mandate "Committed memory by default,
  lazy allocation opt-in. No silent overcommit." That policy maps cleanly onto
  Linux's `vm/overcommit_memory = 2` (strict accounting: total commit may not
  exceed swap + RAM·ratio), **not** the Linux default `0` (heuristic
  overcommit). So the *honest* value reflecting our design is `2`. The hesitation
  is purely about second-order app behavior: some Linux apps read this file and
  change strategy (e.g. Go/JVM/Electron/WINE allocate large sparse mappings
  expecting lazy backing; on seeing strict accounting they may shrink arenas or
  refuse to start). Our `/proc/sys` is read-only, so an app that tries to *write*
  it (to request overcommit) gets a write error — which Linux apps generally
  tolerate (the write needs CAP_SYS_ADMIN anyway).
- **Options**
  - **(A) Expose `vm/overcommit_memory = 2`** — pro: honest reflection of the
    "no silent overcommit" design; apps that respect it allocate within real
    limits. con: **its biggest risk is that some apps refuse to start or scale
    back** — software tuned for the Linux default (Go/JVM/Electron/some WINE
    paths) reserves large sparse mappings expecting lazy backing; on seeing
    strict accounting (`= 2`) it may shrink its arenas, warn, or in a few cases
    bail out at startup. A further con: read-only means an app can't flip it.
  - **(B) Expose `vm/overcommit_memory = 0`** (advertise heuristic overcommit) —
    pro: matches what most Linux desktop apps assume, maximizing drop-in
    compatibility. con: a *lie* — we don't actually overcommit, so an app that
    trusts `0` and over-allocates would hit commit failures our design intends
    to surface up front; contradicts the design and the "never fabricate" rule.
  - **(C) Keep `vm/` omitted** *(current)* — pro: an absent file makes glibc/apps
    fall back to their built-in default assumptions rather than acting on a
    value we're unsure about; no fabrication. con: some readers log a warning
    when the sysctl is missing. **Clarification (answering your question):** a
    *missing* `/proc/sys/vm/overcommit_memory` almost never stops a program from
    running. Well-behaved code treats the open/read failure as "this knob isn't
    available, use my built-in default" and carries on; the visible effect is at
    most a line of log noise. The "treated as an error" case is the program's
    *own internal* error path for the file read, not a refusal to run — so under
    your stated priority (maximize programs that run and don't crash, log noise
    acceptable) option C is *safe*. The thing that actually risks a refusal-to-
    start is option A's strict value, not C's absence.
- **Operator-proposed options (2026-06-13):**
  - **(4) Per-program user-configurable value, with OS-surfaced diagnosis.** Ship
    a default (still TBD — see below), but let the user override the
    `overcommit_memory` value *per program*. Crucially, when the OS detects a
    program is hitting (or likely to hit) an overcommit-related problem, it
    surfaces a plain-language explanation of the issue and how to fix it (i.e.
    which knob to change), so the user isn't left guessing. — pro: turns an
    obscure kernel tunable into a discoverable, fixable setting; a single
    misbehaving app can be accommodated without changing global policy. con:
    needs (a) a per-program config store, (b) real detection of "this commit
    failure was overcommit-related," and (c) UI/notification plumbing to explain
    it — none of which exist yet; and it still leaves the "default default"
    open.
  - **(5) Implement BOTH memory strategies and make them configurable.** Actually
    build strict-commit *and* lazy/overcommit allocation in the kernel, expose a
    choice **system-wide** *and* **per-program**, for both Linux and native
    programs. Default Linux programs to `overcommit_memory = 0` (overcommit) but
    let the user change that default and override per Linux program; allow
    per-program override for non-Linux (native) programs too. All of this lives
    under **Settings → Advanced**, with warnings (and/or a blanket "changing
    advanced options can cause problems" warning). — pro: **this is the
    design-faithful answer.** `design.txt`/CLAUDE.md already say "Committed
    memory by default, **lazy allocation opt-in**. No silent overcommit." — i.e.
    *both* strategies are already sanctioned, with lazy as an explicit opt-in.
    Option 5 is the full realization of that policy: maximum app compatibility
    (overcommit-expecting Linux apps get what they want) without lying (the user
    opted in; nothing is silent), and the strict default for native code stays
    true to "committed by default." con: it's the most engineering. It requires
    the allocation path to *actually honor* the mode (see feasibility note),
    per-program policy storage, and the Settings UI. It's a real feature, not a
    one-line sysctl value.
- **Feasibility note (state of the code, 2026-06-13).** A config *surface* for
  this already exists but is **advisory-only**: `kernel/src/fs/mmtune.rs` defines
  `OvercommitMode { Never, Heuristic, Always }` with per-profile `overcommit` +
  `overcommit_ratio` fields, and `set_overcommit`/`set_overcommit_ratio`. **But
  nothing in the allocation/commit path reads it** — there are no consumers of
  `.overcommit` in `mm/` or `accounting.rs`; the actual behavior is hardcoded
  "committed by default, no overcommit" (`mm/oom.rs:39`). So Options 4/5 are not
  starting from zero (the mode enum, ratio, and a tuning-profile system exist),
  but the *load-bearing* work — wiring the mode into `mmap`/commit-charge so it
  changes real allocation behavior, plus per-program policy and Settings UI — is
  genuinely unbuilt. That's the cost line for Options 4 and 5.
- **Claude's recommendation (revised 2026-06-13)** — Two-phase:
  1. **Now (cheap, unblocks the immediate question):** since your priority is
     "max programs run and don't crash, log noise OK," the lowest-risk immediate
     value is **(C) keep `vm/` omitted** — it can't cause a refusal-to-start the
     way (A) can, and it doesn't lie the way (B) does. Avoid (A) precisely
     because of the refuse-to-start risk you flagged.
  2. **Target (the right end-state):** **Option 5**, because it's the literal
     implementation of the existing design ("lazy allocation opt-in"), gives the
     best compatibility *and* honesty, and Option 4's per-program override +
     diagnosis is naturally a *subset* of Option 5's per-program policy. Treat 4
     as the UX half of 5 rather than a competing option. The honest caveat: 5 is
     a multi-step feature (kernel commit-mode enforcement → per-program policy →
     Settings UI → detection/diagnosis), so it should be scheduled as its own
     roadmap initiative, not slipped in. **Open sub-question for you:** the
     "default default" for *Linux* programs — you proposed `0` (overcommit) for
     max compatibility, which I agree with; native programs stay strict-commit
     per the design. Once 5 lands, the procfs `vm/overcommit_memory` value simply
     *reports the active mode* honestly (no longer a fabrication), which retires
     the original A/B/C dilemma entirely.
- **Where it bites** — `kernel/src/fs/procfs.rs`: `SYS_FILES`/`SYS_DIRS`
  (add `"vm"` dir + `"vm/overcommit_memory"`), `gen_sys` (the value/report).
  For Options 4/5: `kernel/src/fs/mmtune.rs` (`OvercommitMode`, already present),
  the `mm/` commit/allocation path (must learn to honor the mode — currently
  doesn't), per-program policy storage (PCB / Linux-ABI PCB state), and the
  Settings app (Advanced section + warnings).
- **Status** — OPEN (immediate: stay on C; strategic: Option 5 as a scheduled
  initiative — awaiting operator confirmation of the two-phase plan and the
  Linux-default `= 0`).
