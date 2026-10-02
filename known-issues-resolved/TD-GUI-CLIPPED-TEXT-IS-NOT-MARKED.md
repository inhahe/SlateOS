## TD-GUI-CLIPPED-TEXT-IS-NOT-MARKED — `max_width` cuts mid-glyph and says nothing — ✅ **RESOLVED 2026-08-15**

**Resolution.** `RenderCommand::Text` gained a **required** `overflow:
TextOverflow` field (`Clip` | `Ellipsis`), and the compositor draws the mark.
The operator chose "required, no `Default`" from four options precisely so that
every one of the 4,517 constructions in the tree had to answer the question
`max_width` had been posing and never answering; see `design-decisions.md` §427
for the options and §429 for why the commit also had to fill in lane B's 31
sites. The second measurement the entry complains about below is gone from the
policy path: the compositor decides about the mark from the run it has already
shaped, so `text::elide` is no longer the only way to get a cut marked.

Bounded sites default to `Ellipsis` rather than to the behaviour-preserving
`Clip`, because today's behaviour *is* this entry — a sweep that faithfully
preserved it at four thousand sites would have done nothing.

Tested at all three layers: the compositor (a mark appears only when earned,
stays inside the limit, falls back to clipping when the mark itself does not
fit, and never blanks a field clipping would have filled), the toolkit (each
helper emits the right policy), and `guiremote` (both policies survive the wire,
are distinguishable on it, and an unknown byte is a `DecodeError` rather than a
guess — `PROTOCOL_VERSION` went to 2 for it).

**Status.** ~~Open, and deliberately not fixed in the pass that closed
`TD-GUI-TEXT-COMMAND-DOES-NOT-WRAP`, because the good fix is a change to
`RenderCommand::Text` itself and wants a decision rather than a sweep.~~
The decision was asked and answered.

**What it is.** `max_width` clips: the compositor walks glyphs and stops when
the next one would cross the limit. It draws no ellipsis. So a label that does
not fit ends mid-word — and, worse, ends *plausibly*: "Gateway 192.168.1.1 res"
reads as a complete string to anyone who cannot see the field it was cut from.
A caller that wants the cut marked has to call `text::elide` first, which
measures the string a second time to answer a question the compositor is about
to answer again while drawing. That is the same two-calculations-for-one-quantity
shape as the wrap bug, one layer down.

**How widespread.** Every single-line label in the app tree that passes
`max_width: Some(..)` without eliding first — well over a hundred sites. Most
are fine in practice because the values are short and app-authored; the ones
that bite are those carrying user or network data (file names, SSIDs, error
strings, host names). `netmanager`'s diagnostics detail line was fixed by hand;
the rest were left.

**Proper fix.** Give the command an explicit overflow policy — `Clip` (today's
behaviour, correct for a progress label that must not jitter) versus `Ellipsis`
(the right default for a data-bearing label) — and let the compositor draw the
mark, since it is the only party that knows exactly where the glyphs ran out.

**Why it is not done.** Adding a field to `RenderCommand::Text` touches every
struct-literal construction of it in `gui/**` and `apps/**` — several hundred —
because Rust has no default for a struct-variant field. The alternatives are
each a compromise: a second variant (`TextClipped`) splits the match arms in
every renderer; a builder function leaves the literal form available and so does
not actually prevent the mistake; a blanket `text::elide` sweep at the call
sites fixes the symptom while keeping the double measurement. The mechanical
churn is cheap to *do* and expensive to *review* against three lanes' in-flight
work, so it should be scheduled deliberately rather than smuggled into an
unrelated fix. Recorded for the operator in `open-questions.md`.
