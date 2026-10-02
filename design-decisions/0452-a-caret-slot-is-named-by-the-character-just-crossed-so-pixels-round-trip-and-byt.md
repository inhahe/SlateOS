## §452 — A caret slot is named by the character just crossed, so pixels round-trip and byte offsets need not

**Date:** 2026-08-17
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** when text on one line runs both ways -- an English sentence with
a Hebrew word in it -- there are places where the caret can sit that have *two*
correct byte positions in the text, because the end of the English and the end
of the Hebrew are the same spot on the screen. Something has to decide which of
the two a step-by-the-screen reports. The decision is: report the boundary
belonging to the character just moved past. The consequence is that stepping
left and then right returns the caret to the same *pixel* every time, but the
byte number it reports may come back as the other of that spot's two names.

**Scope — what this does and does not settle.** It settles the *convention* of
the step-by-the-screen primitives, `ShapedRun::caret_left`/`caret_right` and
their toolkit wrappers, which are built, tested and merged. It does **not**
decide whether the arrow keys call them. That is a separate, user-visible policy
question — visual motion (Windows, ICU) versus logical motion (macOS, GTK, Qt) —
and it is the operator's, filed as `open-questions.md` → **C-Q2** and still
unanswered. The arrow keys in every toolkit widget therefore still step by the
string; the primitives sit ready behind a one-line change per site. This entry
is the decision that had to be made *in order to build them*, not a decision to
use them.

### The problem

Stepping a caret by the screen (`known-issues.md` ->
`TD-GUI-ARROW-KEYS-MOVE-IN-LOGICAL-ORDER`) walks it through the *drawn*
order of clusters rather than the byte order. n drawn clusters make n+1 gaps.
Every gap between two clusters read in *opposite* directions is one position on
the screen with two names in the string: the trailing edge of the cluster on
its left and the trailing edge of the cluster on its right are the same pixels
and different offsets. `Affinity` exists to hold that distinction, but it does
not decide the question -- something has to say *which* name a step produces.

Take `ab` + a right-to-left `HR` + `cd`, drawn `a b R H c d`. The gap between
`b` and `R` is byte 2 as "after `b`" and byte 2 as "after `H`" -- here the same
number, but the *affinity* differs, and at other boundaries the numbers differ
too.

The affinity is not a refinement, it is load-bearing: a caret rebuilt from its
byte offset alone on each press does not land on the wrong side of the boundary,
it steps over the entire right-to-left run in one press. On this exact string a
rightward walk that discards the affinity visits 1, 2, 7, 8 and never enters the
Hebrew. `gui/toolkit/src/text.rs` pins that as a test asserting the *broken*
sequence, so the requirement cannot be regressed quietly.

### The options

**A. Name the boundary by the character just crossed.** Moving right from slot
k returns the right edge of `clusters[k]`; moving left from slot k returns the
left edge of `clusters[k-1]`.

*What changes:* the reported byte offset always describes the character the
caret just moved past.

**B. Name it by the character about to be crossed** -- the mirror.

*What changes:* the reported offset describes the character the *next* press
will move over.

**C. Canonicalise: always report the lower offset, or always the
logically-first reading.**

*What changes:* every gap has exactly one name, and a step and its reverse
return the identical `(offset, affinity)` pair.

### The decision, and why

**A**, which is what every established implementation does (ICU, HarfBuzz's
sample caret code, and the behaviour of both major browsers' contenteditable).

**Against C, which looks the most appealing.** Canonicalising buys exact
`(offset, affinity)` round-tripping, but it buys it by throwing away the only
information that makes the *next* step correct. A caret canonicalised to "after
`b`" at a boundary cannot know it arrived there by crossing `H` leftwards, so
the following press steps by `b`'s direction instead of `H`'s and the walk
becomes non-reversible -- exactly the teleporting the whole change exists to
remove. Round-tripping the pair is the wrong property to optimise for; the pair
is an encoding, the *place* is the thing.

**What A costs, stated plainly:** `caret_right` then `caret_left` returns to the
same pixel but may report the other of the boundary's two names. This is not a
bug to be fixed later and it is not hidden: the round-trip tests at both layers
assert on **x**, with a comment saying why, and the same rule is written into
the doc comments of `caret_left`/`caret_right`. A caller comparing two carets
for "same place" must compare drawn positions, not offsets -- which is also why
`TextCursor` is deliberately not `Ord` (see the note in
`TD-GUI-WIDGET-CARETS-ARE-NOT-BIDIRECTIONAL`).

**Between A and B** there is no correctness difference -- they are mirror
images and both round-trip in pixels. A is chosen for conformance: a user who
has used any other text field expects the offset to describe what was just
passed, and matching the platforms is worth more than any argument from first
principles here.

### Where it lives

`gui/font/src/shape.rs`: `VisualCluster`, `ShapedRun::caret_left`,
`ShapedRun::caret_right`, `slot_of_caret`. `gui/toolkit/src/text.rs` wraps
them without adding policy. No caller invokes them yet -- see the scope note
above and C-Q2.
