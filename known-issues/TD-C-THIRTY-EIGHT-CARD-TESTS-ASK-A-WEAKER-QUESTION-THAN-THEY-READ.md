## `TD-C-THIRTY-EIGHT-CARD-TESTS-ASK-A-WEAKER-QUESTION-THAN-THEY-READ` (lane C, 2026-09-22)

**In short:** Thirty-eight apps have a test asserting that their keyboard-help
card "reaches the window". Each one joins every string the window drew into a
single haystack and asks whether it *contains* the row's text. For a row whose
key is one character -- `S`, `N`, `/` -- that question is very nearly always
answered yes by some other word on screen, so the test can pass with the row
deleted. Twenty-four of the thirty-eight have at least one such row.

**Found by running a control.** `apps/whiteboard`'s card draws a row per
drawing tool, generated from `Tool::shortcut`. I removed the Note tool to
check the guard caught it, and the test passed: `contains("N")` is satisfied
by "Nudge the selection" in another row, and `contains("Note")` by the toolbar
button drawn behind the card.

**The fix exists and is used in one place.** `guitk::shortcut::missing_rows`
asks whether some drawn string *equals* the row's keys, which is the honest
question given that `render_card` draws each row's keys as their own `Text`
command. It carries the Nudge case as its own control.

**Why the other thirty-seven are not swept yet, which is a judgement and may
be the wrong one.** The exposure is not uniform. `whiteboard` was vulnerable
because both halves of the row appeared elsewhere on screen -- a
one-character key and a description sharing a word with a toolbar button.
Most of the thirty-eight assert against a full description like "Pause or
resume the selected transfer", which nothing else draws, so deleting the row
*is* caught by the description half even though the key half is weak.

The helpers those tests use are closures as often as functions, several per
file, with join separators that differ, and other assertions in the same test
depend on the joined form. A blanket rewrite of thirty-eight files is a large
diff whose failure mode is a spuriously green suite -- the same shape as the
defect it is fixing.

**I sampled, and the first sample was not a control at all.** I deleted a
single-character row from `apps/torrent` and `apps/notes` and ran their tests;
both passed. That looks damning and proves nothing: the test iterates
`for (keys, what) in SHORTCUTS`, so deleting a row removes it from the
*expectation* as well as the card. **That direction is tautological and no
version of this test could ever catch it** -- including the fixed one. The
`whiteboard` control worked only because those rows are generated from
`Tool::all()`, so the expectation survived the deletion.

**What the real exposure turned out to be.** `render_card` drops rows that do
not fit the window and spends a line saying "... and N more, in a taller
window". A dropped row is the one thing these tests *can* catch -- and the
one thing a substring match can hide, because the missing key is one letter
that some other row's description supplies. So the question is whether any
card truncates at the size its own test renders at:

| | |
|---|---|
| largest static card | 20 rows (`apps/slides`) |
| largest card in total | 24 rows (`apps/whiteboard`, 15 written plus 9 generated) |
| rows that fit at 600px | 27 |
| rows that fit at 800px | 37 |

**Nothing truncates, so nothing is being hidden today.** The thirty-eight
tests pass because their cards really are drawn in full, not because the
check is weak. The weakness is latent: it bites the day a card outgrows its
window, which is exactly when a reader most needs the test to speak up.

**So: not swept, deliberately, and this entry is the record of why.** Each app
gets `missing_rows` when it is next touched for another reason. Sweeping
thirty-eight files to fix a latent fault, in a diff whose own failure mode is
a spuriously green suite, is the shape of change this catalogue keeps warning
about.

**The general rule, which is the part worth keeping:** `Vec::contains`
compares whole elements; `str::contains` asks about substrings. They share a
name, they read identically at the call site, and only one of them answers
"is this row on the card". This is the same distinction `names_the_key` makes
in `scripts/key-survey.py`, written to fix this exact defect in the survey --
by the same hand that then wrote it into thirty-eight tests.
