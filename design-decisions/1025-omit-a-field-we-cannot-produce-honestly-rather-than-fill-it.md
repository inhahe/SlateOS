## 1025. Omit a field we cannot produce honestly, rather than fill it

**Date:** 2026-09-16
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** GNU `bc` labels a runtime error with two facts —
`Runtime error (func=(main), adr=3): Divide by zero`. We can produce the first
truthfully and the second not at all. The choice was between printing a number
in the `adr=` slot that looks like GNU's and means something else, or leaving
the field out. We leave it out.

**What `adr` actually is.** GNU `bc` compiles each statement into a little `dc`
program and `adr` is the byte offset within it. We walk a tree and compile
nothing, so there is no corresponding quantity — not a harder-to-reach one, an
absent one.

It is worth saying why "just use the line number" was rejected rather than
merely not chosen: it was *measured* and it is not one. `1/0` reports `adr=3`
whether it sits on the first line of a file or the fourth, and `sqrt(-1)`
reports `adr=4`, because the offsets are into the compiled statement and the
counter restarts for each. Anything we put there that tracked lines would agree
with GNU by coincidence on one-line scripts and disagree on every other.

**The alternatives:**

* **(a) Emit `adr=` with a counter of our own** — statements executed in the
  current function, say.
  *What changes:* the shape matches GNU exactly and a script that parses the
  field gets a number. It is the wrong number, and nothing about the output
  says so.
* **(b) Emit `func=` and omit `adr=`.** *(chosen)*
  *What changes:* `Runtime error (func=(main)): Divide by zero`. A reader
  comparing against GNU sees a field is missing. A script parsing for `adr=`
  fails to find it instead of finding a fiction.

**Why (b).** The asymmetry is in how the two failures are discovered. A missing
field announces itself the first time anyone looks — the harness reports the
difference on every run, and it is marked `differs_by_design` with this
reasoning attached. A fabricated field announces itself never: it is a plausible
small integer in the position a plausible small integer belongs, and the only
way to find out it is meaningless is to already know. We would be spending a
reader's trust to buy a shape.

This is the same distinction that governs a fabricated *fact* versus a
fabricated *action* (§945, lane A): the cost is not in the error, it is in
whether the error is visible. An absent field is a question; a wrong field is
an answer.

**Where it does not apply.** `func=` we *can* produce honestly — the
interpreter always knows which function body it is in — so it is produced, and
it matches GNU in all six cases measured, including the two that are easy to
get backwards (the innermost frame wins, and an undefined callee is blamed on
its caller because the callee has no body to be inside of).

**Reversing this** is one `format!` in `Interpreter::run` plus whatever counter
(a) would need. The harness rows are already marked and would go back to
`known_bug` with the entry reopened.
