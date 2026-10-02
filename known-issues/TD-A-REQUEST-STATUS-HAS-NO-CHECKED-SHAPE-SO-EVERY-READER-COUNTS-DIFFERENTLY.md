## TD-A-REQUEST-STATUS-HAS-NO-CHECKED-SHAPE-SO-EVERY-READER-COUNTS-DIFFERENTLY (lane A, 2026-09-11) — **open**

**In short:** the `requests/` dropbox is how the three lanes hand work to each other,
and each file is supposed to say whether it has been dealt with. Nothing checks that it
does, or where, or in what words — so anyone trying to find out what is still
outstanding writes a search, and every search gets a different answer. Four attempts in
one night gave four numbers, and the true answer was "essentially none".

### The measurements, in the order they were made

130 files are addressed to lane A. Asking "which are still open":

| rule | answer | what it actually measured |
|---|---|---|
| no `**Status:**` containing ✅/DONE/resolved/LANDED/… | **13** | *unstamped*, not unanswered |
| …and not answered in `design-decisions.md` | **9** | *absent from one document* |
| status line must exist and not say open/blocked | **7** | keyword list missing `FIXED`, and `DONE` case-sensitive against lowercase `done` |
| status may appear mid-line | **~0** | the convention allows `**Filed:** … **Status:** ✅ FIXED` on one line |

Each refinement found a class the previous rule had missed, and each produced a number
confident enough to act on. The fourth one is the tell: `b-a-kcmp-compares-any-two-processes-with-no-authority-check.md`
reads `**Filed:** 2026-08-21 · **Status:** ✅ **FIXED 2026-08-21 by lane A in e62931fb7**`
on a single line, so a line-anchored search sees no status and reports a *security
finding* as outstanding.

### Why it is worth fixing rather than just knowing

Three times in one night I went to implement something that was already done, and
stopped only because I read the code before writing any:

* `b-a-the-kernels-cwd-and-libcs-cwd-are-two-different-directories.md` — answered ten
  days earlier as `design-decisions.md` §648, more thoroughly than the request proposed.
* `b-a-raw-nic-claim-tests-race-and-the-reader-is-the-writer.md` — fixed by moving two
  never-running `#[test]`s to boot self-tests; the resolution lives in a source comment
  citing a `known-issues.md` ID, so no document search finds it.
* `c-a-pseudo-header-sum-is-public-…` — the request said "nothing is blocking you" and
  two preconditions were unmet, which is the same failure pointed the other way.

A lane that believes a settled question is open spends a cycle re-deciding it. A lane
that believes an open question is settled does worse.

### The fix, and why it is the same shape as one that already works

`design-decisions.md` has exactly this problem solved: `scripts/check-design-decisions-bands.py`
requires every new section to carry a `**Lane:**` field within 12 lines of its heading,
*because* a field nothing checks is a field that drifts. A request-status gate is the
same rule one directory over:

* every file under `requests/` carries a `**Status:**` field;
* in a checked position — its own line, within the first N lines;
* whose first word is from a closed vocabulary (`OPEN`, `BLOCKED`, `DONE`, `DECLINED`,
  `WITHDRAWN`), so a search is a dictionary lookup rather than a guess.

Ratcheted at the current population, so nothing has to be back-filled at once. The
point is not tidiness: it is that "what is outstanding" becomes a question with one
answer instead of one answer per reader.

**Until then, the honest method is to read the request and ask whether the tree already
answers it** — which is what found all four of the above, and what no grep found.

### 2026-09-11, later — it is not a `requests/` problem, and the fix is bigger than a gate

The same thing happened four more times the same day, in **this** file. Sweeping
lane-A entries for what is still open gave 23, then 15, then 9, then 6 as the
search improved, and three of the nine survivors turned out to be finished work:
`TD-A-FS-SELFTESTS-NEVER-RUN`, `TD-A-AN-ABSENT-OPERAND-DEFAULTS-TO-A-LIVE-OBJECT-ID`
and `TD-A-PRISTINE-STATE-CAN-BE-TOO-BIG-FOR-THE-STACK`. The last of those already
said **Fixed in `71d7148ad`** — a *fifth* spelling, after `**Status:** RESOLVED`,
`— FIXED <date>` in the heading, `### Resolution`, and `DONE`.

So the diagnosis generalises: any status field nothing checks produces one number
per reader. What changed today is that I measured the corpus before building the
gate, and **the measurement argued against building it**:

| | count |
|---|---|
| `## TD-*` / `## B-*` entries in this file | **415** |
| carrying a `**Status:**` line within 9 lines of the heading | **68** |
| not | **347** (84%) |

At 84% non-compliance a ratchet would not be enforcing a convention that drifted;
it would be **introducing** one, in a document lane B writes more of than lane A,
and every one of their future entries would start failing a gate they never agreed
to. That is a cross-lane authoring decision and not a lane-A gate, which is the
same conclusion this entry reached in the morning for `requests/` — now with a
number instead of an instinct.

**What to do with it instead.** The proposal stands and is cheap *if the lanes want
it*: `**Status:**` on its own line within N lines of the heading, first word from
`OPEN | BLOCKED | DONE | DECLINED | WITHDRAWN`, ratcheted at 347 so nothing is
back-filled at once. It needs agreement first, so it belongs in front of the
operator and the other two lanes rather than in a gate. Until then the honest
method is unchanged and is what found all three finished entries above: **read the
entry and ask whether the tree already answers it.**

*(One measurement in this note is deliberately not quoted: a per-lane breakdown of
the 347. It came from splitting each id on `-` and taking the second field, which
yields `A` for `TD-A-FOO` and `E` for `B-EXT4-DIR`, so it produced thirty "lanes"
including a backtick and the digit 1. The 415/68/347 split is sound; the
attribution was not, and an unsound number next to a sound one devalues both.)*
