## 11. /proc/sys/vm/overcommit_memory & the SlateOS memory-commit policy — build Option 5 (both strategies, configurable) now

**Date:** 2026-06-13 (revised same day — see "Revision" below)

**Decided by:** Operator (this was `open-questions.md` Q2; the operator chose
Option 5 — "build both strategies, make them configurable" — with the priority
"maximize the number of programs that run without crashing; log noise is
acceptable." Options 4 and 5 were the operator's own proposals. Initially the
operator accepted a two-phase "C now, Option 5 later" plan; the operator then
asked to **do Option 5 now if there's no good reason to defer** — and a code
survey found most of the mechanism already exists, so the kernel core is being
built now. See "Revision"). **Re-confirmed by the operator 2026-06-14** when the
standing Q2 confirm was put to them: keep the shipped per-ABI commit-policy
defaults — **native strict/committed, Linux lazy/overcommit**, both configurable.
The operator deferred to Claude on whether strict is the better native default
("if you think strict is better for our OS, then i'll go with that"); strict is
kept for native because a desktop OS benefits from honest, immediate allocation
failures over deferred OOM-kill surprises, while Linux keeps overcommit because
Linux programs assume it.

**Update (2026-06-13, later) — split the system-wide knob per ABI.**
**Decided by:** Operator (operator asked "shouldn't we have two system-wide
policy selectors, one for native and one for linux, because linux tends to
expect overcommit?"; Claude agreed and implemented).
The original design had *one* system-wide knob (`mm.lazy_default`) that only
governed the **native** ABI, while the Linux ABI was hardcoded lazy — so an
admin could tune native's default but not Linux's, which is backwards (Linux is
exactly where overcommit-vs-strict is most likely to matter). Fixed by giving
each ABI its own system-wide selector:
- **Native** → `mm.lazy_default` (sysctl id 1), default committed (Desktop).
- **Linux** → new `mm.linux_lazy_default` (sysctl id 8), default 1 =
  lazy/overcommit on all workload profiles. Surfaced to userspace under the
  canonical Linux name `/proc/sys/vm/overcommit_memory`, which now *mirrors the
  live sysctl* (lazy → `0` heuristic-overcommit, committed → `2` never-overcommit)
  instead of being a hardcoded `0`.
`MmapCommitPolicy::linux_lazy` now takes the system-wide value (like
`native_lazy`): `Inherit` follows `mm.linux_lazy_default`, `ForceLazy`/
`ForceCommitted` override per-program. The workload presets carry
`linux_lazy_default = 1` uniformly (Linux apps expect overcommit regardless of
profile; flipping it manually drops profile detection, which is correct). Commit
*"mm: split system-wide commit policy per ABI (native vs Linux)"*. The Settings
front-end (§5.6) therefore exposes **two** system-wide selectors.

**Status (2026-06-13) — all three now-doable kernel items have landed.**
The unblocked kernel work below (items 1–3) is implemented and boot-tested:
- **(2) Linux mmap defaults to lazy/overcommit** + **(3) `/proc/sys/vm/overcommit_memory`
  exposed** (reading `0`, honest now that the Linux path passes `MAP_LAZY`) — commit
  *"mm: Linux mmap defaults to lazy/overcommit + expose vm/overcommit_memory"*.
  The Linux `mmap` path now also translates `PROT_WRITE`/`PROT_EXEC` into
  `MAP_WRITE`/`MAP_EXEC` (a latent read-only-anon bug fixed in passing).
- **(1) Per-program policy** — `pcb::MmapCommitPolicy` {Inherit, ForceCommitted,
  ForceLazy} stored on the PCB, inherited across fork, consulted by *both* `mmap`
  paths via pure `native_lazy`/`linux_lazy` helpers; kernel API
  `pcb::get/set_mmap_commit_policy`; covered by `pcb` self-test. Commit
  *"mm: per-program memory-commit policy override (Option 5 kernel core)"*.
Still following their dependencies (unchanged): the Settings → Advanced GUI
front-end and the capability-gated *writes* to `/proc/sys/vm/*` (`admin.memory_policy`).
The advisory `OvercommitMode` enum in `mmtune.rs` remains unwired — the live
mechanism is `MAP_LAZY` + `PARAM_MM_LAZY_DEFAULT` + the per-program policy; a
future cleanup could retire `OvercommitMode` or fold it into this path.

**Revision (2026-06-13) — do Option 5 now; only the GUI front-end and
capability-gated writes follow their dependencies.**
A survey of the actual code (prompted by the operator asking whether Option 5
could just be done now) found the mechanism is **~80% already built**, so there
is no good reason to defer the kernel core:
- **Both strategies already exist.** Native `mmap`
  (`kernel/src/syscall/handlers.rs::sys_mmap`) supports eager-commit (default)
  *and* demand-paged (`MAP_LAZY`); demand paging is fully implemented
  (`kernel/src/mm/fault.rs`, `VmaKind::Anonymous`).
- **A system-wide toggle already exists.** `sysctl PARAM_MM_LAZY_DEFAULT`
  (`mm.lazy_default`, default 0 = committed on Desktop) flips the system default;
  the per-workload profile presets already set it (Desktop/Dev/Gaming = committed,
  Server = lazy).
- **The advisory `OvercommitMode` enum** in `kernel/src/fs/mmtune.rs` is a second,
  unwired surface for the same concept (no consumer in the commit path).
- **What's genuinely missing (the now-doable, unblocked kernel work):**
  1. **Per-program policy** — today the choice is system-wide only; add a
     per-process override (PCB field consulted by both `mmap` paths).
  2. **Linux programs don't default to lazy/overcommit.** The Linux `mmap`
     (`kernel/src/syscall/linux.rs::sys_mmap`, ~line 4825) routes through the
     native handler with flags=0, inheriting the *committed* desktop default —
     with a now-stale comment claiming it's "demand-allocated." The operator's
     "Linux default = overcommit" is **not actually implemented**; this is a
     latent compat gap (Linux's idiom is large sparse mmaps that expect lazy
     backing). Fix: Linux `mmap` should default to lazy unless a per-program
     policy says otherwise. *(Partly forward-looking: per decision #4 there is no
     Linux ELF loader yet, so no real Linux program runs today — which is why
     this hasn't bitten. Fixing it now makes the path correct for when the loader
     lands.)*
  3. **Expose `/proc/sys/vm/overcommit_memory`** reading the active mode honestly
     (committed ↔ report `2`; lazy ↔ report `0`), plus `overcommit_ratio`/
     `overcommit_kbytes` for completeness.
- **What still follows its dependency (not arbitrary deferral):**
  - **Settings → Advanced GUI** — depends on the GUI/Settings app, which per
    decision #9 comes *after* the terminal/dev phase. Build it when the GUI
    exists; until then the policy is set via sysctl/config.
  - **Capability-gated *writes* to `/proc/sys/vm/*`** (`admin.memory_policy`
    enforcement) — depends on the capability framework (largely unbuilt). Until
    then `/proc/sys` stays read-only and the policy is set via the kernel sysctl
    mechanism.
- **Design nuance noted (not blocking):** SlateOS "committed" currently means
  *eager-populate* (allocate+map all frames at `mmap`), which satisfies "no
  silent overcommit" trivially but costs up-front faulting/RAM for pages never
  touched. Linux's `overcommit_memory=2` instead does *commit accounting* (reserve
  charge against RAM+swap, still demand-page). Eager-populate is the current,
  design-compliant behavior; a future refinement could switch "committed" to
  accounting-style reservation for the same guarantee at lower cost. Out of scope
  for the initial Option 5 build.

**Context:**
`design.txt`/CLAUDE.md mandate "Committed memory by default, **lazy allocation
opt-in**. No silent overcommit." Linux exposes `/proc/sys/vm/overcommit_memory`
(0 = heuristic overcommit [Linux default], 1 = always overcommit, 2 = strict
commit accounting). SlateOS currently hardcodes strict "committed by default, no
overcommit" (`mm/oom.rs`) and our `/proc/sys` is read-only with the `vm/`
subtree omitted. The question was whether to expose the file and at what value.
Options considered: (A) expose `= 2` (honest strict value, but its biggest risk
is that overcommit-expecting apps — Go/JVM/Electron/some WINE paths — may scale
back arenas, warn, or in a few cases refuse to start), (B) expose `= 0` (a lie —
we don't actually overcommit; an app trusting it could over-allocate and hit
commit failures), (C) keep `vm/` omitted (a *missing* sysctl almost never stops
a program — well-behaved code falls back to its built-in default; effect is at
most a line of log noise), (4) per-program user-configurable value with
OS-surfaced diagnosis, (5) implement **both** commit strategies and make the
choice configurable system-wide *and* per-program for both Linux and native
programs.

**Decision — build Option 5's kernel core now (see Revision above for why it's
mostly already built); GUI front-end and capability-gated writes follow their
dependencies.** Until the `/proc/sys/vm/overcommit_memory` surface lands, the
`vm/` subtree stays omitted (the original option C), which is harmless. The full
Option 5 scope:
  - Implement both **strict-commit** and **lazy/overcommit** allocation in the
    kernel (today only strict exists; the `OvercommitMode` enum in
    `kernel/src/fs/mmtune.rs` is advisory-only and **not wired into the commit
    path**).
  - Expose the choice **system-wide and per-program**, for both Linux and native
    programs, under **Settings → Advanced** with warnings.
  - **Default for Linux programs: `overcommit_memory = 0` (overcommit)** for
    maximum drop-in compatibility (operator's call); **native programs default
    to strict-commit** per "committed by default."
  - Option 4 (per-program override + OS diagnosis UX) is folded in as the **UX
    half of Option 5**, not a competing option.
  - Once 5 lands, `/proc/sys/vm/overcommit_memory` simply **reports the active
    mode honestly** (no longer a fabrication), retiring the original A/B/C
    dilemma.
- **Writes to `/proc/sys/vm/*` are gated on the privilege Linux calls
  CAP_SYS_ADMIN.** A Linux program may *write* the sysctl to request a policy
  change if it holds that privilege — but see the capability decision below for
  how that maps onto SlateOS's native model (we do **not** import CAP_SYS_ADMIN as
  a native capability).

**CAP_SYS_ADMIN / capability mapping (operator asked: add it to the native
capability list, or does it map to an existing capability?):**
- **Do NOT add `CAP_SYS_ADMIN` to the native capability list in
  `roadmap-detailed.md`.** CAP_SYS_ADMIN is Linux's notorious "junk drawer" —
  one coarse token gating ~1000+ unrelated operations. Importing it as a native
  capability would reintroduce exactly the **ambient authority** SlateOS exists to
  abolish ("capability-based security from day one, no ambient authority"), and
  it contradicts the project's deliberately **fine-grained** capability model
  (`fs.*`, `admin.*`, `resource.*`, `hook.*`, each a distinct risk level).
- **CAP_SYS_ADMIN is a Linux-ABI construct that lives only in the Linux compat
  layer.** When a Linux program performs an operation Linux gates on
  CAP_SYS_ADMIN, the compat layer maps **that specific operation** to the
  fine-grained *native* capability that actually governs it — it never grants a
  blanket "admin" power.
- **For the overcommit-write operation specifically, no existing native
  capability is an exact fit.** `resource.ram` is a *per-process RAM limit*, not
  a *system-wide VM-policy* control; `admin.*` today covers *user* administration
  (`admin.user`/`admin.user_caps`/`admin.cross_user`). Changing the **system-wide
  memory-commit policy** is a distinct, elevated risk that warrants its **own
  fine-grained native capability** — to be added when Option 5 is built (working
  name `admin.memory_policy`, i.e. "change system-wide memory/VM commit policy").
  A tracking entry is added to `roadmap-detailed.md` now.
  - Note the privilege split this enables (better than Linux's all-or-nothing):
    changing the **system-wide** policy needs `admin.memory_policy`; a user
    changing **their own program's** per-program override via Settings is a
    normal user/Settings action, **not** an elevated syscall — so per-program
    tuning doesn't require an admin capability at all.

**Rationale:**
- C now is the safest immediate answer for the stated priority and requires no
  new code (the `vm/` subtree is already omitted).
- Option 5 is *design-faithful*: the spec already sanctions both strategies with
  lazy as an explicit opt-in. It maximizes compatibility (overcommit-expecting
  Linux apps get what they want) without lying (the user opted in; nothing is
  silent), and keeps native code strict per "committed by default."
- The capability stance preserves least-privilege: a fine-grained
  `admin.memory_policy` is far safer than honoring a Linux blanket CAP_SYS_ADMIN,
  and the Linux-cap→native-cap mapping is the general pattern for the whole
  compat layer.

**Alternatives considered:**
- **(A) expose `= 2`** — rejected for now: real refuse-to-start / arena-shrink
  risk for overcommit-expecting apps, against the "max programs run" priority.
- **(B) expose `= 0`** — rejected: a fabrication (we don't overcommit), against
  the "never fabricate in procfs" rule and the design.
- **Add CAP_SYS_ADMIN as a native capability** — rejected: ambient-authority
  junk drawer; contradicts the fine-grained capability model.

**2026-08-18 — build status, recorded by the S305 standing audit. The decision
is unchanged; only this entry's description of the tree was stale.** The prose
below (and the Decision above) says "until the `/proc/sys/vm/overcommit_memory`
surface lands, the `vm/` subtree stays omitted (the original option C)" and
"requires no new code (the `vm/` subtree is already omitted)". Both sentences
stopped being true some time ago — the audit exists because a stale premise
reads as a live blocker to the next person. What is actually built:

| Option-5 element | State |
|---|---|
| Both commit strategies in the kernel | **Built** — `mmap` genuinely backs on touch under lazy |
| System-wide native knob (`mm.lazy_default`, strict) | **Built** (`kernel/src/sysctl.rs`) |
| System-wide Linux knob (`mm.linux_lazy_default`, lazy) | **Built**, independent of the native one |
| Per-program override | **Built** — `pcb::MmapCommitPolicy` (`Inherit`/`ForceCommitted`/`ForceLazy`), inherited across `fork` |
| `/proc/sys/vm/overcommit_memory` **read** | **Built** and honest: `1 → "0"`, `0 → "2"`; `vm` is in `SYS_DIRS`, with a procfs self-test asserting it |
| `/proc/sys/vm/overcommit_memory` **write** | **Not built** — `procfs::write_file` answers `NotSupported` for everything but `oom_score_adj` |
| `admin.memory_policy` capability | **Not built** — no definition exists under `kernel/src/cap/`; it appears only in doc comments |
| Settings → Advanced UI | **Not built** (lane C) |

The last three are consistent with each other rather than a gap: the capability
is absent *because* the write path is absent, so there is **no ungated route to
the system-wide knob** — the read-only surface cannot be used to change policy.
`overcommit_ratio`/`overcommit_kbytes` remain deliberately unexposed (§1: never
advertise an unhonored knob). Whoever implements the write path must land
`admin.memory_policy` in the same change, not after it.

**Where it lives:**
- `kernel/src/fs/procfs.rs`: `SYS_FILES`/`SYS_DIRS` (**`vm/overcommit_memory`
  has since landed and reports the active mode** — the parenthetical below
  describing it as absent is superseded by the build-status table above),
  `gen_sys`.
- `kernel/src/fs/mmtune.rs`: `OvercommitMode` (exists, advisory-only — Option 5
  wires it into the commit path).
- `kernel/src/mm/` commit/allocation path + `mm/oom.rs` (must learn to honor the
  mode), per-program policy storage (PCB / Linux-ABI PCB state).
- `kernel/src/syscall/linux.rs`: the Linux-ABI write path + CAP_SYS_ADMIN→native
  capability mapping (when sysctl writes are implemented).
- `roadmap-detailed.md`: new `admin.memory_policy` capability (tracking entry),
  and the Option-5 "both commit strategies, configurable" feature.
- Settings app: Advanced section (system-wide + per-program overcommit, warnings).

**How to reverse:**
- Immediate: exposing `vm/overcommit_memory` early (still read-only) is a small
  `procfs.rs` change if a specific app needs to *read* the value before Option 5
  lands; pick the honest current value (strict) per decision #1's "never
  fabricate" rule.
- End-state: if Option 5 proves not worth the complexity, fall back to a single
  honest read-only value reflecting the hardcoded strict policy. The capability
  decision (no native CAP_SYS_ADMIN) is independent and should not be reversed.
