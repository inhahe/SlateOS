### TD-OILS-HISTEXPAND-WORD-RANGE. word-designator ranges clamp where bash errors, and `-`/`$` endpoints are off — ✅ RESOLVED 2026-07-29

**Where:** `userspace/oils/src/histexpand.rs` — `apply_word_designator()`: the
`pick` closure, the leading-`-` arm, and `read_point`.

**What.** Three separate divergences, all measured against bash 5.2 with
`a b c d` as the previous event (probe: record the line, then read back the
stderr echo of `: mark !!<designator>`):

* **Out of range is an error, not an empty selection.** osh's `pick` clamps with
  `to.min(last)` and returns the empty string when `from > to`. bash refuses the
  line: `!!:4` on a four-word event, `!!:9`, `!!:1-9`, `!!:0-9`, `!!:3-1` (from >
  to) and `!!:^` on a one-word event all print `<spec>: bad word specifier` and
  run nothing. The message body quotes the designator back *including* its
  leading `:`. Two shapes are genuinely empty rather than errors, so the rule is
  not simply "clamp → error": `!!:*` on a one-word event, and `!!:2-` on a
  three-word event.
* **A bare leading `-` ends at last-1, not last.** `!!:-` on `a b c d` gives
  `a b c`, and on a one-word event gives the empty string; osh's
  `read_point(...).unwrap_or(last)` gives all four. osh's `n-` arm already gets
  this right, so the two arms disagree with each other.
* **`$` does not let a following `-`, `*` or `$` be read as a range.** After a
  `$` endpoint the next character is left alone: `!!:$-` is `d-`, `!!:$*` is
  `d*`, `!!:$$` is `d$`. But `!!:*-` is `b c d-` and `!!:--` is `a b c-`, so the
  suppression is specific to `$`.

Other endpoints measured and already correct: `!!:-$` → `a b c d`, `!!:-^` →
`a b`, `!!:^-` → `b c`, `!!:1-` → `b c`, `!!:^-$` → `b c d`, `!!:^-2` → `b c`,
`!$` → `d`, `!^` → `b`, `!*` → `b c d`.

**Fixed.** `apply_word_designator` now returns `Result<(String, usize), String>`,
`?`-ed at its single call site in `expand_one`, so an out-of-range designator
fails the whole expansion the way a missing event does — the existing
`Expansion::NotFound` already carries a whole message body, so no new variant was
needed. The range is now carried as a `RangeEnd` enum rather than a bare number,
which is what makes the tolerant case expressible: `RangeEnd::Given` is
range-checked, `RangeEnd::SecondToLast` (the end bash *defaults* for a trailing
`-`) is not, so `!!:3-` is empty while `!!:3-1` is an error. `$` and `*` became
their own arms, terminating the designator instead of falling into the range scan.

Widening the probe from ~20 shapes to ~150 turned up four *more* divergences that
the entry above did not know about, none of them guessable from the manual (which
documents only `x-y`):

* **A bare number needs the colon.** `!!1` is the whole event with a literal `1`
  stuck on the end, not word 1 — osh answered `b`. `^`, `$`, `*` and `-` do not
  need it, so the reader for the *start* of a range now takes `had_colon` into
  account while the reader for the *end* does not (`!!-2` is words 0 through 2).
* **A bare `^` can stand in for `-^` as the end of a range.** `!!:0^` is words 0
  through 1 and `!!:^^` is word 1; `!!:2^` is the `bad word specifier` you would
  expect of a backwards range. Only one may — the second `^` of `!!:^^^` is
  literal — and neither `$` nor a number gets the same treatment (`!!:2$` is word
  2 and a literal `$`, `!!:^2` is word 1 and a literal `2`).
* **`*` never fails.** It is exempt even from the start-of-range check: on a
  one-word event `!!:*` is empty where `!!:^` is an error, though both reach for
  word 1.
* **`*` also terminates the designator**, exactly as `$` does — `!!:*-` is
  `b c d-`. The entry above had this backwards, reading `!!:*-` as evidence that
  the suppression was specific to `$`; in fact the `-` is literal in both, and
  what makes `!!:--` come out as `a b c-` is the *bare leading* `-` form
  consuming the first `-` and defaulting its end.

**Tests.** `word_designators_name_a_range_and_reject_one_that_is_out_of_range`
(≈120 assertions over both a four-word and a one-word event, including the exact
message bodies) plus `tests/corpus/histexpand-word-range.sh`, kept separate from
`histexpand-words.sh` so the tokenizer and the range semantics stay independently
bisectable.

**Measurement note worth keeping.** `history -p` masks the message body with its
own `history expansion failed`, so the exact `:4: bad word specifier` text can
only be seen on the *interactive* path — probes were run as
`printf … | bash --norc --noprofile -i`, reading the echoed line back off stderr.
And a first draft of the corpus case failed for a reason that has nothing to do
with designators: **a script line is added to the history as it is read**, so in
two consecutive `history -p` lines the `!!` of the second names the *first line*
rather than the recorded event. Every probe therefore re-records the event with
its own `history -s` on the line immediately before it. (Both shells agree on
this, so it is a property of the harness, not a divergence — but it silently
turns a designator test into a test of something else.)
