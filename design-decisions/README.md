# Design Decisions Log

This directory records **deliberate design decisions** made during development,
each with enough context to reconsider it later. It is distinct from the
broad spec (`design.txt`) and the original rationale notes
(`design desicions.txt`, `other design decisions.txt`): it is a
running, dated log of decisions taken while implementing, especially ones
where a reasonable alternative exists and the operator might want to revisit.

Format for each entry:

- **Decided by** — who made the **final call**, *not* who first proposed the
  idea. Use `Operator` whenever the decision was put to the operator and the
  operator chose — **regardless of who suggested the chosen option.** Claude
  having proposed (or argued against) the option that was picked never moves the
  attribution to Claude; it stays `Operator`. Use `Claude (autonomous)` only when
  Claude resolved it without putting it to the operator (`Claude
  (operator-approved scope)` when the operator pre-approved the direction but
  Claude made the specific call). A parenthetical may record the collaboration —
  who proposed the option and whether Claude agreed — e.g. `Operator (Claude
  proposed this option)` or `Operator (Claude recommended otherwise; operator
  overruled)` — but that note never changes the attribution. An **Operator**
  decision is settled policy and should not be silently revisited; a **Claude**
  one is Claude's to revisit and the operator may want to overrule it.
- **Context** — what problem forced a choice.
- **Decision** — what was chosen.
- **Rationale** — why.
- **Alternatives considered** — and why they were rejected.
- **Where it lives** — files/symbols, so the decision can be located and reversed.
- **How to reverse** — what changing the decision would entail.

## One file per decision

Each decision is its own file, `design-decisions/NNNN-<slug>.md`, named by its
section number (zero-padded to four digits) and a slug of its title. The first
line is the decision's heading, exactly as it was: `## 538. Lanes publish to ...`
or `## §482 — A convention ...`. Citations elsewhere in the tree (`design-decisions.md
§538`) keep working: the number is the start of the file name.

**Taking a number.** Take the next unused number in your lane's *open* band (the
table below) -- one above the highest number already in that band, never a gap
below it, because a gap may be a number that was spent and withdrawn, and
reissuing it makes an old citation resolve to the wrong entry:

    python scripts/check-docs.py --next-decision <your lane letter>

prints the number and the file name to create. Write a `**Lane:** X` field near
the heading and a `**Decided by:**` field (see the format above).

**What changed, and why** (2026-10-02): with one file per decision
two lanes can no longer write the same lines, so the old rule that each band must
ascend in *file order* -- the rule that kept merges conflict-free in the single
file -- is gone. The bands remain because two lanes must still never take the same
number. `scripts/check-docs.py` enforces the numbering; it replaced
`check-design-decisions-bands.py`.

## Numbering bands

**This table is machine-read** by `scripts/check-docs.py`, which parses each
`§lo–§hi` row for its owner and for the word `open` or `closed`. Keep the shape.

| Band | Owner | Status | Region of this file |
|---|---|---|---|
| §1–§127 | single-agent history | closed | the head — never renumber these |
| §200–§299 | **lane A** | closed — full at §299 | after the history |
| §300–§399 | **lane B** | closed — full at §360 | after A's first band |
| §400–§499 | **lane C** | closed — full at §498 | after B's first band |
| §500–§599 | **lane C** | closed early at §579 — 20 numbers unused | interleaved with A's §600s |
| §600–§699 | **lane A** | closed early at §679 — 20 numbers unused | interleaved with C's §500s |
| §700–§799 | **lane B** | closed early at §779 — 20 numbers unused | interleaved before C's §800s |
| §800–§899 | **lane C** | closed early at §885 — 14 numbers unused | immediately after §579; C's own run ascends |
| §900–§999 | **lane A** | closed early at §979 — 20 numbers unused | immediately after §679; A's own run ascends |
| §1000–§1099 | **lane B** | **open** | the tail — B alone still appends at EOF |
| §1100–§1199 | **lane D** | **open** | immediately after §360, the end of lane B's first band; D's own run ascends from there |
| §1200–§1299 | **lane E** | **open** | immediately after §498, the end of lane C's first band; E's own run ascends from there |
| §1300–§1399 | **lane F** | **open** | immediately after §127, the end of the single-agent history; F's own run ascends from there |
| §1400–§1499 | **lane C** | **open** | immediately after §885; C's own run ascends |
| §1500–§1599 | **lane A** | **open** | immediately after §979, the end of lane A's §900 band; A's own run ascends from there |

Bands below the open ones are closed but **not free**: every number in them is
spent, and spent numbers are never reissued. §217–§220 are lane C's although they
sit in lane A's first band (lane C's choice, 2026-08-17: eight things cite them).
§268–§276 and §626 each name two different decisions; the duplicates are recorded,
never renumbered, and no new duplicate is allowed.
