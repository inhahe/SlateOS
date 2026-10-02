## §443 — A base direction is a third argument to one full `shape_with`, and it switches off the left-to-right fast path

**Date:** 2026-08-16
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** Which way a line of text runs is usually decided by the text
itself — if it starts with a Hebrew letter it runs right to left. That guess is
wrong whenever the direction is a property of *where the text is* rather than
what it says: an `OK` button in a Hebrew interface, a Hebrew folder name in a
left-to-right path bar. The shaper had no way to be told. It does now, and two
things about how are worth recording.

**One full form with defaults, not a method per knob.** `shape_with(text, lang,
base)`; `shape(text)` is `(text, None, Base::Auto)` and `shape_lang(text,
lang)` is `(text, lang, Base::Auto)`.

*The alternative* — `shape_with(text, base)` beside the existing
`shape_lang(text, lang)`, which is what the issue that asked for this proposed
— reads better at each individual call site and does not survive the second
knob. Language and direction are orthogonal: a Hebrew UI rendering a Turkish
place name needs both, and under the two-method shape there is nowhere to say
so short of a fourth method. Three methods where the third is the full form and
the other two are it with defaults is the arrangement that does not grow when a
fourth parameter arrives.

*The alternative also considered* — an options struct, `shape_with(text,
ShapeOptions { .. })` — is what this becomes if a fourth parameter does arrive.
It was not worth introducing for two fields, and introducing it later is a
mechanical change at three call sites rather than a redesign.

**The left-to-right fast path is gated on the base, not only on the text.**
`byte_levels` returns an empty level vector — skipping the entire bidi
algorithm — for any string `bidi::is_trivially_ltr` accepts. That now requires
`base != Base::Rtl` as well.

*The alternative* — keep the check purely textual, since the text really does
contain no right-to-left character — is wrong in a way worth naming, because
the check reads like a property of the string and is not one. What
`is_trivially_ltr` actually asserts is a property of the *answer*: every level
comes out even, so the permutation is the identity and rule L4 mirrors nothing.
That is true under `Auto` and under `Ltr`, and false under `Rtl`, where the
same plain-Latin string resolves to level 2 inside a level-1 paragraph — the
letters still read left to right, but the neutrals around them take the
paragraph's direction and any brackets mirror. Left ungated, the fast path
would have discarded the base the caller had just supplied, and done it
silently and only for the strings where the caller most needed it: a UI label
is exactly the kind of string that has no strong right-to-left character in it.

**A note on the example this was filed with.** The issue's motivating case was
`"(123)"` in a right-to-left paragraph, "the parentheses come out mirrored the
wrong way". They do not: under `Base::Rtl` rule L4 mirrors both brackets and
rule L2 then swaps their positions, and the two cancel exactly, so the string
renders identically to the `Auto` answer. That is correct — a number inside a
Hebrew sentence reads left to right, brackets and all. The cancellation is
general for a balanced pair around a single embedded run, which is why the
regression test uses the unbalanced `"(a"` instead: `(a` under `Auto`, `a)`
under `Rtl`. Recorded because the wrong example was persuasive enough to sit in
the issue file unchallenged, and a test written from it would have passed
against a `base` argument that did nothing at all.

**How it is measured.** A host-font test over 547 of this machine's faces
asserts the drawn order and the mirrored glyph, taking every glyph id out of a
*whole shaped string* rather than out of a character shaped alone — on
`Amiri-Bold.ttf` a lone `(` is scriptless and shapes to glyph 3, while the `(`
in `"(a"` joins the Latin run and reaches a `latn` lookup that substitutes
6460. An oracle built from isolated characters would have been testing the
itemizer. The test also does *not* assert that the two directions measure the
same width: L4 substitutes a genuinely different glyph, and Amiri-Bold gives
`)` a different advance from `(` — 11.904 px against 11.552. What is asserted
is that each answer is self-consistent, drawn width equal to measured width.

**Where it lives.** `gui/font/src/scaled.rs` — `shape`, `shape_lang`,
`shape_with`, `byte_levels`; `gui/font/src/bidi.rs` — `Base`, unchanged, and
`is_trivially_ltr`, whose doc already said what it asserts;
`gui/font/tests/host_fonts.rs` —
`a_paragraph_direction_can_be_given_and_changes_the_answer`.
