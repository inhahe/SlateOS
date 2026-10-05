## TD-C-ASSERTION-MESSAGES-CARRY-COLLAPSED-LINE-CONTINUATIONS -- FIXED 2026-09-13

**Date:** 2026-09-13. **Lane:** C.
**Where:** about 60 string literals across ~30 files under `gui/**` and
`apps/**`; ten were repaired in `gui/compositor/src/lib.rs` and
`gui/desktop/src/session/tests.rs` on the day this was filed.

**In short:** when a test fails, the message it prints sometimes has a long
gap in the middle of a sentence — *"the stacking&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;
is confined"*. Cosmetic, but it lands at the exact moment somebody is trying
to read quickly.

**The cause.** A message written across two source lines with a trailing `\`
relies on Rust stripping the newline *and* the following indentation. When
`rustfmt` later joins the two lines, the indentation is left in the literal
and the backslash goes. Nothing warns, because the result is a perfectly valid
string.

**Do not fix this with a blanket regex.** It was tried on the day this was
filed and reverted, having found two classes of false positive on the first
pass:

* **Deliberate column alignment.** `apps/screenshot`'s shortcut list is
  `"Alt+PrintScreen      Active window"` — the run of spaces *is* the layout.
* **Embedded indentation.** JSON and table fixtures —
  `"      \"type\": \"{}\",
"` — where the spaces are the output's own
  formatting. Excluding runs that follow `
` is not enough, because a regex
  for four-or-more spaces also matches starting one space into a six-space run.

**FIXED 2026-09-13, by exactly the rule this entry recommended.**
`scripts/check-collapsed-messages.py` rewrites a literal only when it is an
argument to `assert!`, `assert_eq!`, `panic!`, `expect` and their relatives --
found by walking back from the literal while the paren depth says we are still
inside a call. **53 messages repaired across 30 files, and zero hits in
`apps/screenshot`**, the false positive that forced the first attempt back.
Every one of the 83 changed lines is a string literal; nothing else moved.

It is a gate now, not just a repair: 0.7 s over 382 files, wired into
`check_lane_c_gui_gates`. Three of its five self-test fixtures are the false
positives from the reverted attempt -- a shortcut list, a JSON body, a table
row with alignment specifiers -- so the rule that would rewrite them fails its
own tests rather than being caught by review a second time.

**The recurring lesson, stated once because it landed three times today.** The
shape rule and the position rule find almost the same set, and the difference
is entirely false positives: `check-overlay0-ink.py` asks whether a role is
the colour *argument* of a text draw rather than whether the word appears
nearby; `ink-text.py` asks whether a `RenderCommand::Text` is a value or a
pattern; this asks whether a literal is a message or a layout. In each case
the shape version was confidently wrong about a population it could not see.

**The proper fix** is to decide by *position*, not by shape: only rewrite a
literal that is an argument to `assert!`, `assert_eq!`, `panic!` or `expect`.
That is parseable — walk back from the literal to the opening of the enclosing
macro call — and it excludes every false positive above by construction, since
none of them are assertion messages. A `scripts/check-*.py` gate on the same
rule would stop it recurring; two of the ten repaired were introduced the same
day by the same hand that filed this.
