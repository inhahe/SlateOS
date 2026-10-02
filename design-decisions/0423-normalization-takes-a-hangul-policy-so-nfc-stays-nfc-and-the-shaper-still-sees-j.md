## §423 — Normalization takes a Hangul policy, so NFC stays NFC and the shaper still sees jamo

**Date:** 2026-08-15
**Decided by:** Claude (autonomous)
**Lane:** C (graphics, apps & net)
**Affects:** `gui/font/src/norm.rs` (`Hangul`, `normalize`, `nfc`, `pieces`,
`split_undrawable`, `needs_work`), `gui/font/src/scaled.rs` (`shape`),
`gui/font/src/hangul.rs`, `gui/font/src/gsub.rs`, `gui/font/src/fallback.rs`

Korean is encoded two ways — 11,172 precomposed syllables, or conjoining
jamo — and the two are canonically equivalent, so Unicode is content either
way. Normalization is not: NFC composes `<L,V,T>` into a syllable, and that
composition destroys precisely the distinction the Hangul shaper reads.
HarfBuzz's Hangul shaper therefore sets
`HB_OT_SHAPE_NORMALIZATION_MODE_NONE` and does its own thing.

We could not simply copy that, because in this crate `pieces`' composition was
the **only** reason Korean rendered at all: the ordinary Korean text font ships
the precomposed syllables and no jamo, so a run of jamo through a
normalization-free path draws three missing-glyph boxes where it used to draw
one correct syllable. Which encoding to *draw* is a question about the face,
not about the text — and that is the observation the decision turns on.

**Three options.**

*Drop Hangul composition from `nfc` outright*, as HarfBuzz's mode name
suggests. Smallest edit, and wrong: `nfc` is also the answer to "what is the
NFC of this string", which is a question about text with a fixed answer.
Bending it to suit one consumer makes a general-purpose function quietly
lie.

*Keep `nfc` and let the shaper undo its work* — compose, then decompose
syllables back to jamo when the face wants them. Preserves every caller, but
it is two passes that cancel, and the round trip is lossy in the cases where
it matters most (a syllable that composed from a jamo sequence the face can
draw is indistinguishable afterwards from one the author typed precomposed).

*Parameterize normalization on a Hangul policy.* A private
`enum Hangul { Normalize, LeaveAlone }` threads through `decompose_once`,
`compose_pair`, `decompose_into` and `compose`; `nfc(text)` passes
`Normalize` and keeps its meaning exactly, while `pieces` — the shaping
entry point — passes `LeaveAlone` and hands jamo to `hangul::preprocess`,
which decides per face whether to compose. Chosen. It costs one enum and
one argument, and it is the only form in which both callers get a true
answer instead of a compromise between them.

Two consequences worth writing down. `nfc` lost its last production caller
in the split — everything in the shaping path now goes through
`normalize(text, LeaveAlone)` — so it carries an explicit
`#[cfg_attr(not(test), allow(dead_code, …))]` rather than being deleted: it
is the text-half of a deliberate split, and deleting it would leave the crate
with no answer to a question it should be able to answer. And `needs_work`
deliberately asks with `Normalize`, the *conservative* direction: it gates a
fast path, so over-reporting work costs time and under-reporting costs
correctness.

The measured result: the HarfBuzz differential sweep went from 892
disagreements to 339 across 556 host faces × 23 strings, with `reordered` and
`misplaced` both staying at 0, and `osfont` from 482 to 501 passing tests
(`hangul.rs`'s own 19 had never run, because an undeclared module compiles
nowhere). The 339 that remain are a different question — font-aware
recomposition of Latin diacritics — filed as `open-questions.md` C-Q1
because it would invert this same layering for a different script.
