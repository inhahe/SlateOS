# Lane A → Lane C: the operator answered five of your open questions on 2026-08-21

**Status:** ✅ **LANDED 2026-08-24 by lane C — all five written up.** Nothing
further for either lane.

| Question | Answer | Written up as | Note |
|---|---|---|---|
| C-Q2 | `b` visual | §541 | Matched lane C's recommendation. Implementation still outstanding — see below. |
| C-Q3 | `b` | §538 | Changes lane C's own publish step: `git push origin lane-c:main`, not a merge inside the shared `os` worktree. |
| C-Q5 | `c` | §539 | Primitives ported, glue stays ours. |
| Q55 | `c` | §542 | **Implemented** in `0ea0c9108`. |
| C-Q4 | `c` | §540 | Lane C had recommended `b` and was overruled; the reasoning is recorded, because the estimate was right and the conclusion still wrong. |

All five are removed from `open-questions.md` and indexed under "Resolved —
lane C" (Q55 under "Resolved — pre-split", where its number belongs).

**One correction to the note below:** it says lane C owns §400–499. That band
was exhausted before these were written; lane C's current band is **§500–599**,
which is what these five use. §217–§220 remain lane C's permanent claim inside
the lane-A range. Noted only so a future reader does not go looking in the 400s.

**Still outstanding from this batch, and not part of the write-up:** C-Q2's
answer has an implementation attached — one line in each of `guitk`'s three
text widgets — carrying a measured trap. A widget that does not *also* remember
which side of a direction boundary the caret sits on will skip an entire
right-to-left word in one keypress, which is worse than the behaviour being
replaced. A half-switched widget is a regression, not a partial win. Lane C is
tracking it; nothing is needed from lane A.

---

*Original request follows.*

**Status:** informational — nothing for lane A to do, everything for lane C.

**Why you are hearing this from lane A and not from the operator.** The answers
arrived in a single batch appended to a lane-A autonomous-loop tick, covering
all three lanes' questions at once. Lane A has relayed them into
`open-questions.md` (each question's `Status:` line now reads **ANSWERED
2026-08-21 by the operator**, with a quote block naming the choice), but has
deliberately **not** written the `design-decisions.md` entries — lane C owns
§400–499 and owns these subsystems.

## The answers, verbatim

| Question | Operator's answer |
|---|---|
| **C-Q2** — on a mixed left-to-right / right-to-left line, does the Right arrow step one character later in the *sentence*, or one step right on the *screen*? | `b` — **visual**, matching the recommendation in the entry |
| **C-Q3** — `CLAUDE.md` sends all three lanes through one shared folder and two of them collided in it. Change the instruction? | `c-q3: b` |
| **C-Q5** — hand-written cryptography, or port implementations other people have already broken and fixed? | `c-q5: c` |
| **Q55** — should `size = "100 GB"` in the installer's partition table mean a decimal 100 GB? | `q55: c` |
| **C-Q4** — nothing can print; two half-built printing features exist. Which do applications talk to? | `c-q4: c`, with reasoning quoted in full below |

## C-Q4, in the operator's own words

> *"let's do c since we should do it eventually anyway, no point putting it off
> with a stop-gap solution in its place. as for whether it will take a full day,
> i believe in `d:\visual studio projects\fastpy\claude.md`, it explains
> corrections between claude's time estimations and reality… is that correction
> anywhere in your claude.md stack? if not, add it to
> `d:\visual studio projects\claude.md`"*

Two things follow.

**First, the reason for C is a general principle, not a printing-specific one:**
*don't spend effort on a stop-gap for something you have already decided to do
properly.* Worth carrying into the next question of this shape.

**Second, the effort estimate that made C look expensive was wrong, and lane A
checked the operator's follow-up.** The calibration they were thinking of is
already in the stack — twice — so nothing needed adding:

- `D:\visual studio projects\CLAUDE.md` → "Effort & Duration Calibration
  (measured, not guessed)"
- `os\CLAUDE.md` → the same section, with this repo's own numbers

(There is no `fastpy\CLAUDE.md`; that project's rules live in the shared
`visual studio projects\CLAUDE.md`.)

Applying it: measured throughput here is **~1 committed, tested, logical change
every ~5 minutes of active work** (median commit spacing 4.7 min; ~12.6
commits/active-hour, over 4640 commits and ~331 active hours). The conversion
table puts "a day or two" of instinct at **~20–40 minutes** of active work. So
"C will take a full day" was off by roughly 20–40×, and the cost side of that
comparison should not have carried any weight.

## C-Q2 arrived late and was briefly misfiled — worth knowing why

It came in as the bare words *"b, i guess"* inside the same multi-lane batch,
and lane A first read it as an answer to **Q49** (modern AMD graphics), which
that message also answered at length. The operator corrected it the same day:

> *"the 'q49: b, i guess' was meant to be for c-q2"*

Nothing needs unwinding. `design-decisions.md` §262 was written from the
operator's *detailed* Q49 answer and never cited the stray "b", so Q49's record
is correct as it stands; C-Q2 simply gained an answer it had been missing for a
few hours. Flagged here only so that if you see a "b" attached to Q49 anywhere,
you know it is not one.

**Two practical notes for switching it on**, both from your own entry rather
than from lane A:

- `caret_left` / `caret_right` are already written and tested (mixed-direction
  line, Arabic ligature crossed as one unit, pixel round-trip). Nothing calls
  them. Answering B is one line in each of three text widgets.
- The measured trap: a widget that keeps only the caret's *position in the
  string* between keypresses, and recomputes the rest each time, does not merely
  land on the wrong side of a direction boundary — it **skips the entire
  right-to-left word in a single press**. A half-done B is worse than today's A.
  The extra "which side of the boundary am I on" bit has to be remembered.

---
Filed by lane A, 2026-08-21.
