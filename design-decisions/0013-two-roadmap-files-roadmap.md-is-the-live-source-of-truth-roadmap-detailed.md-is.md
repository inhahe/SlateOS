## 13. Two roadmap files — roadmap.md is the live source of truth; roadmap-detailed.md is an annotated design reference

**Date:** 2026-06-13

**Decided by:** Claude (operator-approved scope) — the operator delegated the
call ("you're the developer, so I'll make it your call") and suggested the
annotation convention (flag parts done/blocked/blocked-by in
`roadmap-detailed.md` without deleting information). Claude made the specific
policy.

**Context:**
The repo has two roadmap files that had drifted apart:
- `roadmap.md` — 846 commits, continuously updated with task-completion status
  (procfs `/proc/sys`, DRM shim, ALSA, ld.so/dynamic-linker all recorded here).
- `roadmap-detailed.md` — its own header calls it "the fine-grained companion to
  `roadmap.md`. Every actionable feature from `design.txt` … as a checkbox item."
  Only 41 commits; recent work is largely absent (e.g. ld.so/dynamic-linker: 0
  mentions). 1207 items, of which only 156 were marked done — i.e. its status
  flags lag reality badly.

The operator initially believed `roadmap-detailed.md` was "the final say" and
`roadmap.md` might be old news; investigation showed the opposite for *status*
(roadmap.md is the maintained one). The naming misleads: "detailed" = finer
feature enumeration from `design.txt`, not more current.

**Decision:**
- **`roadmap.md` is the single source of truth for live progress/status.** It is
  the file to consult and update when starting/finishing a task.
- **`roadmap-detailed.md` stays the design reference** — the exhaustive
  design.txt-derived feature enumeration. It is **annotated in place** with
  concise status flags (`[x]` done, `[-]` in progress, `[~]`/blocked + a short
  "blocked by …" note) **without deleting any information**, so a reader of the
  design reference can see at a glance what is built. Annotation is **incremental
  and verification-based** — items are flagged done only when verified (cross-
  referenced against `roadmap.md` or the code), never fabricated. A full one-shot
  reconciliation of all 1044 unchecked items is deliberately NOT attempted (too
  large, too error-prone); the gap closes as items are touched.

**Rationale:**
- Avoids dual-maintenance churn and the risk of the two files contradicting each
  other on status, while preserving the genuine value of the detailed file (a
  complete, design-anchored feature inventory the high-level roadmap lacks).
- The operator explicitly wanted at-a-glance status in the design reference, met
  by inline flags rather than by promoting it to the authority.

**Alternatives considered:**
- *Promote `roadmap-detailed.md` to source of truth / deprecate `roadmap.md` in
  CLAUDE.md.* Rejected: roadmap.md is the actually-maintained file; deprecating it
  would discard the live status history. (Also CLAUDE.md is operator-owned; the
  operator's permission to edit it was conditional on roadmap.md being "old news,"
  which proved false, so CLAUDE.md was left untouched.)
- *Keep both fully synchronized.* Rejected: 1207 vs 846-commit drift shows the
  cost is real and the payoff low; the files serve different purposes.
- *Delete `roadmap-detailed.md`.* Rejected: the operator wants it kept as a
  no-information-lost design reference.

**Where it lives:**
- `roadmap.md` (live status), `roadmap-detailed.md` (annotated design reference).

**How to reverse:**
- If maintaining annotations in the detailed file proves not worth it, stop
  annotating and treat `roadmap-detailed.md` as a frozen design snapshot; or, if
  the detailed file becomes the working file, migrate status tracking there and
  note it in CLAUDE.md (operator's call, since CLAUDE.md is operator-owned).
