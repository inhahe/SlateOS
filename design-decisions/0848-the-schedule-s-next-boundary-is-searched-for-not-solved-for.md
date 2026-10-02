## 848. The schedule's next boundary is searched for, not solved for

**Date:** 2026-09-14
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** For the desktop to sleep properly it has to know how long
until the quiet hours next start or stop, so it can set one alarm instead
of checking the clock every minute. Working that out with arithmetic is
fiddly -- the window can run past midnight and only some days are ticked.
So instead the code simply asks "is it different a minute from now? two
minutes? three?" until the answer changes. That is up to ten thousand
integer comparisons, which sounds like a lot and takes well under a
millisecond, and it happens once per change rather than once a minute.

### The decision

`QuietHours::minutes_until_change` walks forward minute by minute over a
week, asking `active_at` at each step, and returns the first offset whose
answer differs. `None` -- the common case, since the feature ships off --
means "set no timer at all", which is design-decisions 812's idle desktop.

| | search | closed form |
|---|---|---|
| cost | ≤10,080 comparisons, once per transition | a few dozen operations |
| how often | twice a day on a typical schedule | the same |
| can it disagree with `active_at`? | no -- it *asks* `active_at` | yes, and silently |
| what has to be re-derived | nothing | midnight wrap × day mask, again |

The closed form is not hard; it is *hard to be sure of*. It would have to
re-derive exactly the midnight-wrap-and-day-mask interaction that 847 is
about, in a second place, with the same opportunity to get it subtly wrong
-- and its failure mode is a desktop that wakes up at the wrong minute,
which nobody would ever notice as a bug.

Measured against what this is for, the cost is not real: the alternative
being avoided is a wake-up every minute, so even a thousand-fold-more
expensive answer computed twice a day is cheaper than the thing it
replaces by three orders of magnitude.
