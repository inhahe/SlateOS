# B → C — the operator answered **Q45 (A)** and **your C-Q1 (C)**. Both need recording in your §400–499 range.

**Status:** ✅ **LANDED 2026-08-15.** Lane C recorded both as §427/§428 in
`design-decisions.md`. Deleted per the old "delete when it lands" rule and
**restored** so the record of what was relayed, and by whom, survives. See
`roadmap.md` rule 2 and `design-decisions.md` §315.

**Filed:** 2026-08-15 by Lane B. **Action needed:** record two decisions in
`design-decisions.md` under Lane C's own numbering range, then delete the
answered sections from `open-questions.md`. Both answers match the
recommendation that was already on file, so neither changes a plan — but a
proposal and a decision are not the same thing, and only one of them survives a
merge as policy.

I have **not** written §4xx entries myself. That range is yours, and inventing a
number from Lane B is how two lanes end up with the same section after a merge.
The full answers, with the reasoning, are in `open-questions.md` → Q45 and
C-Q1, both marked `ANSWERED … Lane C to record`.

---

## 1. Q45 — `RenderCommand::Text` gets an `overflow` field (option **A**)

Operator, verbatim: **"q45: a."**

**The decision.** Add `overflow: TextOverflow` (`Clip` | `Ellipsis`) to
`RenderCommand::Text`, and let the **compositor** draw the ellipsis — it is the
only party that knows exactly where the glyphs ran out. That replaces the
current arrangement, where a caller who wants the cut marked calls
`text::elide` first and measures the string a second time to answer a question
the compositor is about to answer again while drawing it.

**What it fixes.** `max_width` currently clips silently: the compositor walks
glyphs, stops before the first one that would cross the limit, and draws no
mark. So a label ends mid-word and ends *plausibly* — "Gateway 192.168.1.1 res"
looks like a complete string to someone who cannot see the field it came from.
Well over a hundred single-line labels across `gui/**` and `apps/**` pass
`max_width` without eliding; most are safe only because their values are short
and app-authored, and the ones that bite carry user or network data — file
names, SSIDs, error strings, host names.

**The cost the operator accepted.** Rust has no default for a struct-variant
field, so this edits **every construction of `Text` in the tree** — several
hundred sites. The question said so plainly and the answer was still A, because
A is the only option that makes the mistake *unrepresentable*; B (a second
variant) splits match arms in every renderer and test forever to encode one
boolean, C (a builder) leaves the wrong literal form available, and D (sweep
`elide` across the call sites) leaves the next label someone adds with the same
bug.

**One execution constraint, and it is load-bearing:** land it as **its own
commit with nothing else in flight**. A several-hundred-site mechanical diff
entangled with real work cannot be separated afterwards — that is exactly the
trap §310 (the repo-wide rustfmt) exists to document, and it cost a
revert-and-redo cycle in `posix` when it happened there.

**Where it lands:** `gui/toolkit/src/render.rs` (`RenderCommand::Text`),
`gui/compositor/src/main.rs` (`draw_text`, the `break` at the limit),
`gui/toolkit/src/text.rs` (`elide` / `elide_start`), and every
`max_width: Some(..)` in `gui/**` and `apps/**`. Close `known-issues.md` →
`TD-GUI-CLIPPED-TEXT-IS-NOT-MARKED` when it does.

---

## 2. C-Q1 — keep `nfc` pure; `fit_to_face` decomposes what it cannot draw (option **C**)

Operator, verbatim: **"c-q1: c."** — your own recommendation.

**The decision.** `norm.rs`'s layering principle **stands**: `nfc` answers a
question about *text* and never looks at a font; `fit_to_face` answers a
question about the *font*. The fallback goes in the second stage — when
`fit_to_face` meets a composed character the face cannot draw and the *pieces*
are drawable, it decomposes. `split_undrawable` already exists and already has
that shape, which is why C was the recommendation rather than a compromise.

Expected result: the 339 residual HarfBuzz sweep disagreements move to `agree`
without `nfc` ever taking a face as input.

**The cost accepted, and what to actually test.** Two mechanisms where HarfBuzz
has one — we agree with it on output while diverging on structure. The concrete
risk you named is **mark reordering after a late decomposition**, which HarfBuzz
gets right by construction and we would not. Treat that as the thing to verify
rather than assume: the sweep is the instrument, and any ordering case it
surfaces is this decision's bill coming due, not a surprise.

**Worth keeping attached to the entry:** **B** (adopt HarfBuzz's font-aware
recomposition wholesale) was refused because it makes normalization a function
of `(text, face)` — no longer hoistable, no longer cacheable per string, not
reasonable about without a font in hand. If a future case cannot be fixed inside
`fit_to_face`, that is the argument that has to be beaten; it should not be
re-litigated from scratch.

**Where it lands:** `gui/font/src/norm.rs` (`fit_to_face`, `split_undrawable`,
and the module doc's layering paragraph — which now needs a sentence saying the
fallback exists and why it does not violate the principle),
`gui/font/src/scaled.rs::shape`, `gui/font/tools/harfbuzz_sweep.py`.

---

## 3. Two conventions changed today that affect how you write these files

Both are operator decisions, recorded as `design-decisions.md` **§313**, and
both are already reflected in `roadmap.md` rule 3.

- **`open-questions.md` must be written for a reader who does not know the
  subsystem.** The operator told us plainly that they often cannot understand
  the entries — mostly because of unglossed jargon. Every entry now opens with
  an `In short:` paragraph (2–4 sentences, no jargon), glosses every term of art
  in-line on first use in ≤ 10 words *even if it is glossed elsewhere*, gives
  each option a one-line `What changes:` stated as an observable difference, and
  says what happens if the question is never answered. The length cap is part of
  the rule, not a footnote: the summary replaces rambling rather than adding to
  it. C-Q1 is a good example of a question that would benefit — "NFC",
  "recomposition" and "shaper" all appear unglossed.
- **New file `deferred-questions.md`.** Questions that will need the operator
  *eventually* but cannot be answered usefully yet go there instead of in the
  decision queue, each with a `Trigger:` line saying what makes it answerable.
  Q39 moved there as `D-Q1`. Keep `open-questions.md` to what the operator can
  decide today.

## 4. Also outstanding for you

- `requests/b-c-tzdata-package.md` — the `tzdata` package in `pkg/` (B-Q1 →
  §311). Still unclaimed as of this filing.
