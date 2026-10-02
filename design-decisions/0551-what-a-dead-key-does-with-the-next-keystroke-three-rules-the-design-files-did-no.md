## 551. What a dead key does with the *next* keystroke: three rules the design files did not decide

**Date:** 2026-08-24
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** A dead key is one that types nothing when you press it and
instead changes the letter you type next — press `´` then `e` and you get `é`.
That much was already decided. But a user can press anything next, not just a
vowel, and three of those cases had no answer written down anywhere: pressing
*space* after the accent, pressing a *second* dead key, and pressing a key that
types nothing at all (Backspace, Enter, an arrow). This entry records the
answers, which are the same ones Windows, macOS and Linux give, and why each
was not simply derivable from the rules that were already written.

`design-decisions.md` §550 had already decided the fourth and hardest case —
what happens when the composition *fails*, e.g. `´` then `x` — and chose to
type both characters (`´x`) rather than discard one. That is why
`KeyEvent::text` is a `String`. The three below are the remainder.

### Rule 1 — space types the bare accent, and no space

*What changes:* pressing `´` then the space bar inserts `´` alone. The space
is consumed, not inserted after it.

The alternative is to treat space as an ordinary non-composing key and fall
into §550, which would type `´ ` — accent then space. That is defensible and
it is what §550 says for every *other* non-composing key.

Space is the exception because it is the only escape hatch there is. On a
German board the `´` key is dead on both its faces, so without this rule there
is no way to type a bare acute accent at all — a regression for anyone who
writes *about* accents rather than *with* them, and for anyone typing a shell
command containing one. Every major platform makes the same exception, so a
user arriving from any of them already knows it. And it is what lets rule 3
below afford to discard: there is always a deliberate way to get the accent
out, so discarding on an *accidental* keystroke costs nothing recoverable.

### Rule 2 — a dead key is never a base character, and that is checked *before* composition is attempted

*What changes:* pressing two accent keys in a row types the first accent and
leaves the second one waiting, rather than trying to merge them.

This one looks like a no-op until you check Unicode. `¨` (diaeresis) combined
with an acute accent is a real composition: U+0385 GREEK DIALYTIKA TONOS. A
Spanish typist whose board has `´` and `¨` on the two faces of one key can
produce it by accident, and would have no idea what they were looking at. So
the deadness check has to come *first*, before the composition table is
consulted at all — an ordering that is invisible in the common case and
load-bearing in exactly one.

**Which way round it composes matters, and the test was first written the
wrong way.** U+0385 decomposes to `¨` plus a *combining acute*, so the
sequence that composes is `´` **then** `¨` — the acute pending, the diaeresis
typed after it. `¨` then `´` composes nothing at all, and a test written that
way proves this rule vacuously: it passes just as happily with the two checks
reordered. That is not hypothetical — it is how the reintroduction harness
reported this rule unproved (defect `AB`) while the test that claimed to cover
it was green. It is also, across every builtin layout, the *only* pair of dead
faces that composes with each other, so it is the single keystroke sequence in
which the two orderings of the check are distinguishable at all.

A pleasant consequence: pressing the same dead key *n* times types *n-1*
accents and leaves one waiting, with no rule of its own. That is the X11
behaviour, and it means a user who wants two literal accents can simply press
the key three times.

The rejected alternative was to special-case "the same dead key twice" as
"type the accent once, literally" (the Windows behaviour for some layouts).
It needs a rule, it disagrees with itself about what pressing it three times
means, and it does not answer the two-*different*-accents case at all.

### Rule 3 — a key that types no text at all discards the pending accent

*What changes:* pressing `´` and then Backspace leaves you with nothing
pending; the next letter you type is unaccented. Same for Enter, Escape, F5
and the arrow keys.

Backspace is the case that decides it. A user who armed an accent by mistake
reaches for Backspace, and on every real keyboard that is what un-arms it. The
alternative — keeping the accent armed and letting Backspace do its ordinary
job — means the mistake is unrecoverable except by typing a space, which no
user would guess.

The reason this can discard where §550 refuses to is that **nothing visible is
lost**. A pending accent was never drawn on screen; there is no keystroke the
user watched themselves make and then watched vanish. §550's case is the
opposite: the user pressed `x`, saw nothing, and would rightly call that a
dropped keystroke. The two rules are not in tension once that distinction is
named.

Modifier keys are excluded from this rule entirely, and must be: Shift produces
no character, so a naive reading would have Shift cancel the accent — and there
would then be no way at all to type `É`. Ctrl/Alt/Super chords are excluded for
a softer reason: a user who saves with Ctrl+S mid-word is still owed their
vowel.

### Where it lives

`gui/compositor/src/deadkey.rs`, as a `DeadKeys` value held beside
`ModifierState` on the compositor — keyboard state that spans two events, held
once for the whole desktop for the same reason §456 puts the keymap there. The
rules are covered by eleven unit tests in that module and five end-to-end tests
in `lib.rs`, including a cross-crate sweep asserting that every dead face in
every builtin layout carries an accent `osfont::deadkey::combining` knows —
the silent failure being a layout that declares a face dead whose character
composes with nothing, which would swallow every keystroke after it.

### If these turn out wrong

All three are cheap to revisit: each is a single branch in `DeadKeys::press`
with a test named after the rule. None is persisted, none crosses the wire, and
none is visible to a client — the client sees only the resulting text.
