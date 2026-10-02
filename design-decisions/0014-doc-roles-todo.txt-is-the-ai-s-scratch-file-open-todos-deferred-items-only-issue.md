## 14. Doc roles — todo.txt is the AI's scratch file (open TODOs + deferred items only); issues→known-issues.md, decisions→design-decisions.md, open questions→open-questions.md

**Date:** 2026-06-13

**Decided by:** Operator (Claude proposed the "stop todo.txt being a catch-all"
structure and recommended option B; operator chose B and added the refinements
below). The operator also delegated ownership of `todo.txt` to the AI.

**Context:**
`todo.txt` had grown to ~53,000 lines — a `#`-prefixed engineering journal that
duplicated three other sources:
- `roadmap.md` — its ~55 DONE/VERIFIED blocks restate status the roadmap already
  tracks with checkboxes.
- git commit messages — each `DONE: ABI batch NNN` block is essentially the
  commit body re-pasted.
- `known-issues.md` — its ~109 BUG/DIVERGENCE/LIMITATION blocks are exactly what
  `known-issues.md` exists for.
There was also a doc contradiction: `roadmap-detailed.md` said "todo.txt is the
operator's personal file — AI does not write to it," while CLAUDE.md instructs
the AI to write to it (and it is full of AI entries).

Considered: (A) leave as-is (redundant but works); (B) adopt the clean structure
going forward with no mass rewrite; (C) B plus a one-time migration/prune.
Merging the journal into `roadmap.md` was rejected outright — `roadmap.md`'s
value is being a concise, scannable status index; growing it defeats that.

**Decision (option B + operator refinements):**
- **`todo.txt` is the AI's working scratch file** (ownership delegated to the AI;
  `roadmap-detailed.md` updated to agree with CLAUDE.md, resolving the
  contradiction).
- **Going-forward routing:**
  - bugs / divergences / limitations / tech-debt → `known-issues.md`
  - resolved judgment calls & design decisions → `design-decisions.md`
  - judgment calls awaiting operator input → `open-questions.md`
  - completed work → the git commit + the `roadmap.md` checkbox (NOT restated in
    `todo.txt`)
  - `todo.txt` keeps ONLY genuine open TODOs and deferred-with-rationale items.
- **Judgment calls do NOT live in `todo.txt`** (operator's correction): they have
  dedicated homes — `design-decisions.md` (resolved) and `open-questions.md`
  (pending). This supersedes CLAUDE.md's older "document judgment calls in
  todo.txt under a `## Judgment Calls` heading" wording.
- **Legacy migration of the existing issue blocks → `known-issues.md`:** done
  carefully, a chunk at a time, deleting from `todo.txt` as moved. A full
  snapshot was taken first (`todo.backup-2026-06-13.txt`, git-ignored) so the
  move is reversible. (Operator offered either keep-duplicates or
  delete-as-moved-with-backup; chose delete-as-moved-with-backup to actually
  reduce the sprawl rather than add more.)

**Why not the alternatives:**
- *(A) leave as-is:* the redundancy keeps growing and bugs become hard to find
  (three places to look); the operator wants issues consolidated.
- *(C) full bulk reformat now:* `known-issues.md` uses curated formatting
  (`### W1`/`### TD14`, `**Where/What/Why/Proper fix**`), so a faithful migration
  is per-entry human-judgment reformatting, not a mechanical dump — best done
  incrementally to avoid mangling either file.

**Where it lives:**
- `todo.txt` (new scope header at top), `roadmap-detailed.md` (ownership note),
  `known-issues.md` (issue destination), `design-decisions.md`/`open-questions.md`
  (decision/question destinations).

**CLAUDE.md note (operator-owned, NOT edited here):** several CLAUDE.md lines
still describe `todo.txt` as the destination for bugs/limitations (Bug Tracking
section) and judgment calls (`## Judgment Calls`), and for "genuinely stuck"
notes. Those reflect the older catch-all model and are superseded by this
decision. Per the rule that only the operator edits CLAUDE.md, they were left
unchanged — the operator may wish to align them (point bugs→known-issues.md,
judgment calls→design-decisions.md/open-questions.md).

**How to reverse:**
- Restore `todo.backup-2026-06-13.txt` to recover any migrated block verbatim;
  the per-block moves are also in git history. Revert this section and the
  `todo.txt`/`roadmap-detailed.md` headers to return to the catch-all model.
