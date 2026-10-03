## 535. A refactor that deletes call sites must resolve the reintroduction defects it strands, splitting them into re-targeted and retired-by-construction

**Date:** 2026-08-24
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** We keep a harness that proves our tests are real, by putting each
bug back into the code on purpose and checking that a test complains. When the
switch and slider work replaced 22 hand-drawn controls with two shared modules,
it deleted the exact lines 56 of those recorded bugs were written against. The
harness noticed and said so — but a bug it can no longer put back is a test we
have stopped proving, while the list still *looks* as long as it was. The
decision is that this is never allowed to sit: every stranded entry is either
pointed at wherever the choice moved to, or deleted with a note saying which
single new entry replaced it, and nothing may be left merely stale.

### What happened

`scripts/reintro-palette.py` holds 1705 *defects*: a named bug, a file, one or
more literal find/replace pairs that put it back, and the tests expected to
fail when it is in. `--check` applies nothing and only asks whether each
pattern still matches. After the two control modules landed it reported **56
stale** — spread over nine files, all of them either a switch or a slider.

That number is the refactor's own footprint. `touchpad.rs` had eleven, because
it had a switch *and* a slider and both were hand-drawn.

### The split

The 56 are not one thing, and treating them as one would have thrown away real
coverage or kept fake coverage. They divide by a single question: **does the
panel still choose this colour?**

| | Count | Disposition |
|---|---|---|
| The panel still chooses it; only the text around it moved | 30 | Re-target |
| The panel no longer names it at all | 26 | Retire, naming the replacement |

*Re-targeted* means, for example, that `color: if enabled { p.green } else {
p.surface2 },` is now the sixth argument to `switch(...)` and so is written
`if enabled { p.green } else { p.surface2 },`; a slider's track and fill became
the named struct fields `track:` and `fill:`. The bug is unchanged and still
belongs to the panel. Eight of the accessibility panel's defects are of this
kind — the choice of `green` over `accent` there is a real editorial decision
that panel makes, and it should stay guarded at that panel.

*Retired* means the opposite, and it is the refactor's whole point. Fourteen
separate defects said "the switch knob keeps Mocha's `text`" — one per panel.
There is now exactly one line in the shell that inks a knob, so that bug can be
written down once and only once. Seven thumb-colour defects and three
empty-fill guards collapse the same way. Deleting them is not a coverage loss;
it is the coverage becoming *cheaper to state*, and the count dropping from 56
sites to 3 lines is the measurement of what the refactor bought.

One entry is retired for a third reason and says so explicitly: `AAAA…x12`
asserted that a slider thumb following the body text was **wrong** and that the
accent was right. That is exactly backwards — an accent thumb sitting on an
accent fill is 1.00:1 — so the expectation it encoded is the bug that §534
fixed. Retiring it as merely relocated would have hidden that.

### The rule this establishes

1. **A refactor is not finished while `--check` is non-zero.** Stale is not a
   warning to triage later; the whole value of the list is that its length
   means something.
2. **Ask "does the caller still choose this?" per entry, not per file.** The
   nine affected files each contained both kinds.
3. **A retirement must name its replacement.** Every deleted entry leaves a
   four-line comment in place saying which module-50 defect now covers it. A
   silent deletion is indistinguishable from giving up.
4. **Check for orphans before accepting the retirements.** Diff the (file,
   test) coverage sets before and after; any pair that loses its last prover is
   a test that has quietly stopped being proved.

That last check earned its keep immediately. Two pairs came back:

- `mouse_settings::the_knob_is_legible_on_both_pills` had **no** prover at all,
  because it is a test the switch work added and I had not yet declared
  anywhere. It pins the ink by equality with `readable_on(pill)` rather than by
  a contrast floor, so it fails for *any* change to that one line — which makes
  it the natural inheritor of all fourteen retired knob defects, and it is now
  declared on `CCC…x80`.
- `touchpad::the_panel_draws_nothing_that_is_immediately_erased` lost its only
  prover when the empty-fill guard moved into `slider.rs`. The guard itself is
  proved once now, by `JJJ…x80` — but the harness sweeps per file, so a defect
  in `slider.rs` never asks touchpad's sweep anything, and a sweep nothing can
  trip proves nothing. `NNN…x80` patches touchpad's `frac` to `1.0` so the fill
  covers the track exactly, which is the erasure the test is named for.

Two further pairs looked orphaned and were not: `the_knob_is_the_same_ink_on_both_pills`
and `the_thumb_is_derived_from_the_fill_it_ends` no longer exist — §534 renamed
and inverted both — and their successors are proved.

### Alternatives rejected

- **Leave the 56 stale and let `--check` report them.** Rejected: this is the
  failure the harness exists to prevent, one level up. A test nobody proves and
  a defect nobody can apply fail the same way — silently, while looking fine.
- **Re-target all 56 at `switch.rs`/`slider.rs`.** Rejected: seventeen defects
  all patching one line is seventeen copies of one experiment. It would keep
  the count flattering and make the sweep seventeen times slower for no extra
  information, which is the same "wrong copies of one answer" disease §534 was
  about — just moved into the test harness.
- **Delete all 56 and rely on module 50.** Rejected: it would throw away the 30
  that are still genuine per-panel editorial choices. That the accessibility
  page uses `green` where every other page uses `accent` is a decision that
  belongs to that page and should break there if someone changes it.
- **Regenerate the defect list mechanically from the diff.** Rejected: the
  interesting half of each entry is the *title* — the sentence saying what a
  user would see. That cannot be derived from a patch, and an entry without it
  is a checksum, not a test of a test.

### Limits

- The split was made by reading all 56, not by a rule a script could apply. A
  future refactor of this size gets the same manual pass; there is no shortcut,
  and pretending otherwise is how the wrong half gets deleted.
- The orphan check compares against the *previous* defect list, so it only
  catches coverage lost in the same change that introduced the retirements. A
  test that was never proved by anything is invisible to it — which is how
  `the_knob_is_legible_on_both_pills` came to have no prover in the first
  place, and is logged in known-issues as its own gap.
