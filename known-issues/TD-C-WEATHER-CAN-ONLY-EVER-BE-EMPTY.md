## `TD-C-WEATHER-CAN-ONLY-EVER-BE-EMPTY` -- **WITHDRAWN, was never a defect** (lane C, 2026-09-18)

**In short:** I filed this saying `apps/weather` draws a full dashboard over
an empty model. It does not. When it has no weather it draws four lines
saying it cannot fetch any, and returns before the dashboard. The app was
already doing the right thing, and had been since the invented cities were
deleted. Nothing here needed fixing; the entry is kept because the *way* I got
it wrong is worth more than the finding would have been.

**What is actually true**, and all of it is fine:

| | |
|---|---|
| the model is empty in production | yes -- `new` holds nothing and every generator is `#[cfg(test)]` |
| the five location operations are unreachable | yes -- and irrelevant, since there is no weather to attach to a location |
| it draws the dashboard over that emptiness | **no.** `render_commands` does `let Some(current) = self.current.clone() else { self.render_cannot_fetch(...); return cmds; }` |
| it says so on screen | yes, in `CANNOT_FETCH_LINES`, and better than I would have written it |

The fourth of those lines is the one to keep: **"It cannot deliver
severe-weather alerts either. Silence here is not an all-clear."** An empty
alerts banner is not a neutral absence -- it is read as "no warnings in
force", which is a claim about the world. `apps/partmanager` makes the same
move for disks ("This is not a finding that you have none"), so this is a
settled pattern in the lane, not one app being careful.

**How I got it wrong, twice, on one entry.**

1. I checked `main`'s constructor, checked `current`'s only writer, checked
   `add_location`'s callers -- and filed. Every one of those checks was
   correct. I never read the render path, which is the only place that could
   answer the question I was actually asking.
2. Told the first version was wrong about the *generators*, I corrected that
   and still did not read the render path -- so the correction repeated the
   same false premise in stronger words, and I put it in the source file too
   ("draws its chrome over an empty model", since removed).

**The lesson is not "check more things".** It is that *"can this app show
anything"* is a question about the **render path**, and I answered it from the
**model**. A model check tells you what the app has; only the draw tells you
what the app claims. Those differ exactly when the app is handling emptiness
well -- so my method was blindest precisely where the code was best, and would
have gone on reporting careful apps as broken ones.

**Concretely, for the remaining no-pointer sweep:** before filing "app X shows
nothing", grep its render entry point for an early return on the empty case.
`weather` and `partmanager` both have one. A test-only data generator is not
evidence of a defect -- it is the normal state of an app whose real source is
not built yet, and the presence of an honest-empty path is what tells the two
apart.
