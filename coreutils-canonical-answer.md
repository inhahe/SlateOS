# coreutils bundle vs. standalone per-tool crates — design-merits answer

**You asked:** ignore *implementation effort* and *disk/network space*; tell me
which option produces the **best end result for the OS design**, with pros/cons
and a recommendation that don't lean on those two concerns.

**Short answer:** **(a) standalone per-tool binaries are clearly the better
design for *this* OS** — not by a small margin, and not because of where the
maintenance has gone, but because a single multi-call binary is fundamentally
**incompatible with capability-based least privilege**, which is a core,
non-negotiable design principle of SlateOS. The one genuine advantage a multi-call
binary has *that isn't about disk space* — shared code — is fully obtainable
**without** a multi-call binary by factoring the common logic into a shared
**library crate**. So you can have the code reuse and keep per-binary identity.

Below, every argument is framed on design quality alone (effort and size
deliberately excluded).

---

## The decisive axis: capability-based security / least privilege

SlateOS is "capability-based security from day one… no ambient authority." That
principle is what makes this not a close call.

- **Per-tool binaries → per-tool capability sets.** Each tool is a distinct
  artifact with a distinct identity the kernel and package system can reason
  about. `ping` can be granted a raw-socket capability that `cat` never
  receives. `rm` can hold a filesystem-write capability that `ls` is denied.
  `mount` can be handed a mount capability in isolation. The capability you
  grant is scoped to exactly the tool that needs it.
- **A multi-call binary → one identity, one capability set, for *all* tools.**
  argv[0] dispatch happens *inside* one ELF with one on-disk identity. The
  kernel sees the same binary whether it's invoked as `cat` or as `ifconfig`.
  To let the binary act as `ifconfig` you must grant the net-admin capability to
  **the same binary that also runs as `cat`**. The effective authority of every
  tool collapses to the **union** of what *any* bundled tool needs. That is the
  textbook definition of ambient authority — precisely what this OS is designed
  to eliminate.

There is no clean way around this for a multi-call binary: you cannot give the
kernel a different capability set per argv[0] without effectively making each
entry point its own identity — at which point you've reinvented separate
binaries with extra indirection. **This single point is sufficient to decide the
question** for a capability OS.

---

## Supporting design arguments (all independent of effort and size)

### 1. Trusted Computing Base / dependency isolation
A standalone crate links only its own dependency closure. `sort`'s collation
crate is not present in the address space of `true`. With a multi-call binary,
the linked dependency set is the **union** of every tool's dependencies, so a
vulnerability in a library used by exactly one tool sits inside the process
image of **every** tool invocation. Per-tool binaries give each tool the
**smallest possible TCB**; the bundle gives every tool the **largest**.

### 2. Fault isolation at the artifact level
At *runtime* both designs spawn a fresh process per invocation, so a crash in
one running tool never touches another either way — that part is a wash. The
difference is at the **build/artifact** level: a miscompilation, bad
optimization, or corrupted build of one standalone tool affects only that tool.
A bug in the **shared startup/dispatch path** of a multi-call binary — or any
corruption of that single artifact — degrades *every* tool at once. Smaller
blast radius favors separate binaries.

### 3. Fit with the content-addressed package store + generations
SlateOS ships a content-addressed store with generational updates (`pkg/`).
Per-tool artifacts are the natural granularity for it:
- A fix to `wc` republishes only `wc`'s content hash; every other tool keeps its
  existing hash and is **deduplicated across generations** untouched.
- Rolling back one tool, pinning one tool's version, or A/B-testing one tool is
  expressible.

A multi-call binary is **one content hash for the whole set**: any one-line
change to any tool invalidates the hash of the entire bundle and forces the
store to treat it as a wholly new object. The packaging model the OS already
committed to rewards per-tool granularity and is fought by the bundle.

### 4. Observability / the security & process UIs
The OS has a process explorer and a capability/security surface. Per-tool
binaries show **meaningful, distinct names and distinct capability sets** there:
an auditor sees that `dd` holds a device capability and `echo` holds nothing.
A multi-call binary shows the **same** binary name and the **same** (unioned)
capability set for every tool, so the security UI can no longer tell you what
authority a given tool actually exercises. Per-tool identity makes the system
*legible*; the bundle makes it opaque exactly where opacity is most harmful.

### 5. Conceptual model / debuggability
"One tool = one crate = one binary" is the simplest possible mental model and
matches the GNU coreutils lineage and the Unix philosophy. argv[0] multiplexing
is a layer of indirection that shows up in backtraces (every crash routes
through the dispatcher), complicates per-tool profiling, and makes "what does
this binary do?" answerable only by reading the dispatch table. Separate
binaries are self-describing.

### 6. Independent evolution
Per-tool crates can adopt different language editions, lint levels, `unsafe`
budgets, or even be reimplemented (e.g. a Rust tool replaced by a fastpy one)
**one at a time**, without coordinating a single shared binary's build. The
bundle forces every tool onto a single shared toolchain/edition/configuration
and a single atomic cutover.

---

## The bundle's *real* (non-size) advantage, and why it doesn't bind

The honest argument for a multi-call binary that **isn't** about disk space is
**code sharing**: common argument parsing, error/usage formatting, exit-code
conventions, locale handling, and I/O helpers written once.

But a single binary is **not required** to share code. Factor the common logic
into a **library crate** (call it `coreutils-common`) and have every standalone
tool depend on it. You then get:

- one canonical implementation of the shared logic (the reuse benefit), **and**
- per-binary identity, per-tool capabilities, per-tool TCB, per-tool packaging
  (everything above).

This is exactly how the `uutils/coreutils` project is structured (a shared
library plus per-tool crates, buildable as individual binaries), and it's the
arrangement that dominates the design comparison. It strictly dominates the
bundle on design grounds: you lose **nothing** that mattered and keep
per-tool identity.

(The two things the bundle wins on — on-disk footprint and a single warm
page-cache image for exec — are exactly the size/perf-of-startup concerns you
asked to set aside. For completeness: exec latency of coreutils is not on any
hot path for a desktop OS, and demand paging + the shared library's shared pages
neutralize most of even that.)

---

## Recommendation

**Adopt option (a): standalone per-tool crates are canonical.** Concretely, the
best end-state design is:

1. **`userspace/<tool>/`** standalone crates are the canonical, shipped tools —
   one crate, one binary, one identity, one capability grant each.
2. Extract the genuinely shared logic into a **`coreutils-common` library
   crate** that the standalone tools depend on, so code reuse is preserved
   without a multi-call binary.
3. **Retire the `coreutils/src/bin/*` multi-call bundle.** Its useful content is
   the shared logic, which moves into the library crate; its multi-call *binary*
   role is the part that conflicts with the capability model and should not
   ship.
4. Point image-build / kernel-embedding at the standalone crates.

Option **(c) keep both** is not a design position — it is the drift-generating
status quo and should be rejected outright.

The deciding factor is not maintenance history or effort; it is that **a
multi-call binary cannot express per-tool least privilege**, and least privilege
via unforgeable per-object capabilities is the security spine of this OS. Every
other axis (TCB size, packaging granularity, observability, fault blast radius,
independent evolution) points the same direction, and the bundle's only
non-size advantage — shared code — is recovered in full by a shared **library**
rather than a shared **binary**.

---

*Written 2026-06-12 in response to the operator's request. This is analysis to
inform your decision — the open question (open-questions.md §1) stays OPEN until
you choose. If you pick (a), I'll move it to design-decisions.md as
`Decided by: Operator`, set up `coreutils-common`, and repoint the build.*
