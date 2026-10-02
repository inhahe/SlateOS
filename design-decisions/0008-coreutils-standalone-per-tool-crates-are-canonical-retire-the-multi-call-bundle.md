## 8. coreutils — standalone per-tool crates are canonical (retire the multi-call bundle)

> **SUPERSEDED by §1005** (2026-09-07) — `coreutils` is the one home for the
> command-line tools; the surviving code is chosen per utility. This entry's
> "standalone per-tool crates are canonical" is no longer the policy. Kept for
> the reasoning, which §1005 argues against rather than ignores.

**Date:** 2026-06-12

**Decided by:** Operator (Claude recommended option (a) — standalone per-tool
crates + a shared library — in `coreutils-canonical-answer.md`; the operator
agreed and chose it).

**Context:**
There was duplication between the `coreutils` crate's bundled binaries
(`coreutils/src/bin/{tr,dd,chown,df,…}`, a busybox-style multi-call binary that
dispatches on `argv[0]`) and the standalone per-tool workspace crates
(`userspace/{tr,dd,chown,df,…}`, one crate → one binary). Both implement the
same tools and were drifting. We needed a single canonical set for the OS image.
The operator asked for an analysis weighed purely on **design quality**, with
implementation effort and disk/network footprint explicitly excluded.

**Decision:**
- **Standalone per-tool crates (`userspace/<tool>/`) are canonical.** One tool =
  one crate = one binary = one identity.
- **Extract shared logic into a `coreutils-common` library crate** that every
  standalone tool depends on (arg parsing, usage/error formatting, exit-code
  conventions, I/O helpers) — code reuse without a multi-call binary.
- **Retire the `coreutils/src/bin/*` multi-call bundle.** Its useful content is
  the shared logic, which moves into the library crate; the multi-call *binary*
  role does not ship.
- Repoint the OS image build / kernel-embedding wiring (currently targeting
  `coreutils/target`) at the standalone crates.

**Rationale:**
The deciding factor is **capability-based least privilege**, a core
non-negotiable principle of SlateOS ("capability-based security from day one, no
ambient authority"). A multi-call binary has **one on-disk identity** for every
tool, so the kernel must grant it the **union** of every bundled tool's
capabilities — `cat` would carry the same authority as `ifconfig`. That is
exactly the ambient authority the OS exists to abolish. Per-tool binaries get
per-tool capability grants (`ping` gets raw-socket, `cat` gets nothing).
Supporting axes all agree: smaller per-tool TCB / dependency closure, smaller
fault blast radius at the artifact level, natural granularity for the
content-addressed package store + generations (a one-tool fix doesn't
invalidate the whole bundle's hash), and a legible security/process UI (distinct
names + distinct capability sets). The bundle's only non-size advantage — shared
code — is fully recovered by a shared **library** crate rather than a shared
**binary**, which is the `uutils/coreutils` structure.

**Alternatives considered:**
- **(b) `coreutils` multi-call bundle is canonical** — rejected: a single binary
  cannot express per-tool least privilege (the decisive point), gives every tool
  the largest possible TCB and capability set, and is a single coarse unit for
  the package store. Its on-disk-size win is exactly the concern excluded from
  the decision.
- **(c) Keep both, one generated from the other** — rejected: not a design
  position; it is the drift-generating status quo.

**Where it lives:**
- `coreutils/src/bin/*` (bundled binaries — to be retired; shared logic migrates
  to a new `coreutils-common` library crate).
- `userspace/<tool>/` (standalone crates — canonical; to depend on
  `coreutils-common`).
- The OS image build / kernel-embedding wiring that currently targets
  `coreutils/target` (to be repointed at the standalone crates).
- `coreutils-canonical-answer.md` (the full analysis).
- `todo.txt`: the "DUPLICATION between the coreutils crate's bins …" judgment
  call (2026-05-31) and the "USE STD" audit note.

**How to reverse:**
- Reintroducing a multi-call binary would require giving up per-tool capability
  grants (or building per-`argv[0]` identity into the kernel's capability check,
  which effectively reinvents separate binaries). Only revisit if the capability
  model itself changes.
