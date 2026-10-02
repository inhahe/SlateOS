## TD-C-THREE-TESTS-AND-TWO-CHECKS-THAT-PROVED-NOTHING-IN-ONE-DAY -- METHOD 2026-09-17

**In short:** five times in one session a green result meant nothing. Three
were tests that passed against code deliberately broken to make them fail; two
were checks that reported success while the thing they were checking was still
wrong. Every one was caught the same way -- by breaking the code on purpose and
watching -- and none would have been caught by reading.

**Date:** 2026-09-17. **Lane:** C.

**The five.**

| What | Why it passed anyway |
|---|---|
| `safeio`'s temp-name collision test | The name also carries the pid and an atomic counter, so it was unique whatever the lossy stem did. The bug it was written for could not happen. |
| `splitter`'s `panes_tile_the_area_exactly` | Asserted with a `0.01` tolerance, which is far looser than the float error it existed to catch. Tightening it to exact equality was **still** not enough: `[0.2, 0.5, 0.3]` lands on the edge either way. It took thirds, sevenths and elevenths before the sabotage failed. |
| `explorer`'s "sides hidden while the panel is closed" | Asked `column_menu_items`, which only ever returns columns. The sides are added by a different function, so the assertion was true regardless. |
| The `\r` line-ending check | The shell escape collapsed to an empty pattern; `grep -c ''` matches every line, so it reported the file's own length as a count of CRs. Three files, all "entirely CRLF", all pure LF. |
| The Rect rename loop | Searched for `E0609`, which is a field *access*. Struct *literals* are `E0560`, so it announced "clean after 1 pass" with six errors outstanding. |

**The one thing that worked, every time.** Change the code so the test *must*
fail, and run it. Not inspection -- all five survived being read, twice in some
cases, by someone who had just written them and knew exactly what they were
supposed to prove.

**The shapes worth recognising, since the mechanism differed each time:**

* **A tolerance wider than the effect.** A property about rounding needs a
  fixture that *rounds*; exact equality on numbers that happen to divide
  cleanly proves nothing either.
* **A fixture that cannot exhibit the bug.** The `safeio` name was unique for
  an unrelated reason, so no input could have failed it.
* **Asking the wrong function.** The assertion was about the menu; the call was
  to something that never contained the rows in question.
* **A pattern that matches everything.** When a count equals the size of the
  thing counted, suspect the predicate.
* **A checker that knows one error shape.** "Clean" meant "clean of the errors
  I know how to look for".

**What this costs when it is missed.** A test that cannot fail is worse than no
test: it occupies the place where a real check would go, and it is *evidence*
to the next reader that the behaviour is pinned. Two of the five would have
shipped a fix whose commit message explained a defect that does not exist --
which is the same waste recorded in
`TD-C-FOUR-CLAIMS-WALKED-BACK-IN-ONE-SESSION`, but wearing a green tick.

**The habit to keep:** after writing a test that passes first time, break the
thing it tests. It costs one command. Three of today's five were found in the
minute after the test first went green, and the two that were not had already
been committed.
