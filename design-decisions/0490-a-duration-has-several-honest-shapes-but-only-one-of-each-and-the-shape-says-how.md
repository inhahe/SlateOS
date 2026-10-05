## §490 — A duration has several honest shapes, but only one of each; and the shape says how the number was obtained

**Date:** 2026-08-21
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** Lots of Slate's programs show you a length of time — how long a
backup ran, how much battery is left, how long ago you copied something. Thirty
-eight of them each worked out for themselves how to turn a number of seconds
into text, and they disagreed. Two windows showed the *same machine's* uptime as
`3d 4h 12m` and `3d 4h 12m 30s`. The music player couldn't count past 60
minutes, so a 61-minute album read `61:01` instead of `01:01:01`. The task
scheduler could report a task as taking `60.0s` when its own code says that
figure means "under a minute". The decision here is that there is now one
module with **seven** named shapes, a caller picks a shape rather than writing
one, and — the part that is a genuine judgement — **which shape you pick
records whether the number was measured or estimated.**

**Glossary:** a *formatter* is the small function that turns `3661` into
`"1h 1m 1s"`. *Truncating* means cutting a number off (`59.96` → `59.9`);
*rounding* means going to the nearest (`59.96` → `60.0`). A *sentinel* is a
special word a program shows instead of a number, like `"Immediate"`.

### What was decided

1. **Seven shapes, in `textfmt::duration`, chosen by what the reader is doing:**

   | The reader is… | Shape | Looks like |
   |---|---|---|
   | tracking a position in media, or watching a timer run | `clock` | `01:05`, `01:01:01` |
   | …and the milliseconds matter (a lap, a stopwatch) | `clock_ms` | `01:05.250` |
   | reading a finished span exactly (a job took *how* long?) | `units` | `2d 3h 4m 5s` |
   | …for something that may be quicker than a second | `units_ms` | `450ms`, `5.2s`, `2m 30s` |
   | glancing at a **measured** span (uptime, a transfer ETA) | `coarse` | `2d 3h`, `1m 30s` |
   | glancing at an **estimate** | `coarse_minutes` | `2d 3h`, `45m` |
   | asking how long ago something happened | `relative` | `just now`, `5m ago`, `yesterday` |

2. **`coarse` and `coarse_minutes` differ only in their bottom unit, and the
   difference is epistemic, not cosmetic.** `coarse` bottoms out in seconds
   because it renders a span someone actually timed; `coarse_minutes` bottoms
   out in minutes because it renders a guess. Printing `1m 30s` for a battery
   estimate *invents* precision nobody has; dropping the `30s` from a measured
   90-second backup *discards* a measurement that was taken. Both are defects,
   in opposite directions, which is why one function could not serve both. The
   two are pinned to each other by a test asserting they agree across two
   million seconds above the hour, so neither can quietly grow a second opinion
   about what "the two most significant components" means.

3. **Shapes truncate; they never round.** `coarse` shows `1d 0h` rather than
   `1d`, because `1d` reads as "at least a day" when the truth is "a day and a
   bit that we cut off". `units_ms` divides with integers rather than through a
   floating-point value, because the old floating-point version could print a
   string its own branch had excluded (`60.0s` from the under-a-minute branch).

4. **Sentinels stay at the call site.** The notification pane's `"now"` (it has
   no clock reading yet — a fact about the pane, not about a duration) and the
   startup list's `"Immediate"` (a word for *no delay configured*, not a
   rendering of zero) are not durations, and the module does not invent English
   for a caller's special cases.

**Scope of the change:** 31 of the 38 formatters the scan found now delegate.
The 7 that do not are deliberate: two render timestamps rather than spans, one
is a timezone offset (a signed displacement, not a length), one — the calendar's
event lengths — works natively in *minutes*, where routing through a seconds API
would turn a one-hour meeting from `1h` into `1h 0m`, and three were seek/scan
arithmetic the scan mistook for formatting.

### Alternatives considered

**One shape for everything.** Rejected on the evidence: the tree's own usage
spans a media scrubber, a stopwatch, a job report and a battery estimate, and
`01:05` is wrong for a battery just as `45m` is wrong for a scrubber. The
problem was never that there were several shapes — it is that there were
several *copies of each* shape.

**Force the battery estimates through `coarse`** rather than add an eighth
public function. This was the cheaper option and was tempting for exactly that
reason. Rejected: it would have made the battery read `45m 0s`, which claims a
precision the estimator does not have. Decision 2 above is the more laborious
answer and the correct one.

**Leave the differences alone as house style per app.** Rejected because the
defect is not aesthetic. Four of the disagreements were between two windows
displaying *one* underlying number — two views of one machine's uptime, two
views of one battery's remaining time, two lists of one clipboard, two lists of
one recording — and a user with both open sees the system contradict itself.

### Why this is a Claude decision and not an operator question

The bugs (no hours field, no days field, a rounding artefact, two windows
disagreeing) are not tradeoffs; they are wrong under any policy. The one real
judgement — measured versus estimated as the axis that picks a bottom unit —
is now a one-word change per call site (`coarse` ↔ `coarse_minutes`), so an
operator who disagrees can move any readout without touching a formatter.

**Consequence to expect:** durations across the desktop change appearance in
several small ways at once. Minutes are zero-padded in clock readings (`1:05`
→ `01:05`); "how long ago" is lower-case and says `yesterday` rather than
`1d ago`; exact spans gained day and hour fields they were missing; battery and
similar estimates no longer show a seconds field. The music player, benchmark,
camera and screen-capture readouts change most, because they were the ones that
could not count past their largest unit.
