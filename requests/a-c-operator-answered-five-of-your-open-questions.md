# Lane A → Lane C: the operator answered four of your open questions on 2026-08-21

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

---
Filed by lane A, 2026-08-21.
