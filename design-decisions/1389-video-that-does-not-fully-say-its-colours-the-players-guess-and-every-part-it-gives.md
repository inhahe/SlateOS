## 1389. Video that does not fully say its colours: the players' guess, and every part it does say converted

**Date:** 2026-10-11
**Lane:** F
**Decided by:** Operator (F-Q11, `operator-answers/2026-10-11-open-questions-answers.3.txt`:
"C, if that would usually result in more correctness"; Claude had recommended B)

**In short:** a video names the recipe for turning its stored brightness
and colour differences back into red, green and blue in four parts -- the
matrix, the range, the primaries and the brightness curve -- and some videos
name only some of them, or none. Chrome uses the whole description only when
all four are given and otherwise falls back to the old standard-definition
recipe. Video players guess from the picture's size instead. SlateOS keeps
the players' guess for what a video leaves out, and uses -- and converts to
the screen -- every part it does give. That is option C.

**Why C is the more correct choice, as the answer's condition asks.** A part
a video names is the encoder's own statement about its pixels; a guess is a
guess. For a fully described video, B and C agree (Chrome's exact pixels).
They differ only on a partly described one, where B -- like Chrome -- throws
away the parts given and C keeps them: a video that says "BT.2020 primaries"
but not its matrix is shown in BT.2020's wider colours under C, in BT.709's
under B. A wrong tag would make C worse, but tags that are present are right
far more often than a size-based guess is, so C is the more correct as a
rule. The one cost: partly described video can differ from Chrome's
rendering, which B also accepted for untagged video.

**What it means in code.** `gui/video/codec/src/colour.rs` (`resolve`):
each of the four parts is taken from the container where it says it,
otherwise from the stream where it says it, otherwise guessed (BT.709 for
HD, BT.601 for smaller); the primaries and the curve that result are then
converted to the screen's as for a fully described video (§1378, F-Q10's
colour management). A video that says only that it is HDR (PQ or HLG) is
shown as HDR, as today (§1379).

**Supersedes:** the "players' guess" half of §1346, which ignored a part
given alone only where Chrome does; §1346's guess for the parts not given
stands.
