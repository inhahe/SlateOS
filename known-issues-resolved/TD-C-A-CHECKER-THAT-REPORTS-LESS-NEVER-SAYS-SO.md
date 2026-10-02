## TD-C-A-CHECKER-THAT-REPORTS-LESS-NEVER-SAYS-SO -- FIXED 2026-09-15

**In short:** the script that finds programs which stay silent about what they
cannot do was itself silently missing most of them. Four separate bugs, found
in one sitting, every one of them causing it to report *fewer* crates than it
should -- and none of them causing it to report an error. It went from 36
findings to 58 with no change to what it was looking for.

### Why this direction matters more than the other

A checker can be wrong two ways. If it **accuses** honest code, somebody reads
the entry, disagrees, and the noise is visible -- the worst case is that the
tool gets distrusted. If it **excuses** code it should have reported, the
output still looks plausible, the count still looks healthy, and **nobody goes
looking for what a tool did not print.** There is no reader for the absent
line.

`find-silent-incapacity.py` is unusually exposed to this, because its logic is
subtractive twice over: a crate is skipped if it looks *capable*, and skipped
again if it looks like it *admits*. Anything that inflates either judgement
removes a crate from the report entirely.

### The four

1. **Literals pulled with `re.findall('"(...)"')` over raw source.** One
   quotation mark inside a `//` comment pairs with the next one in code, and
   the source between them comes back as a "literal". Any comment containing
   the word "cannot" made a crate that admits nothing look like one that does.
   **7 crates.**

2. **Concatenate, then truncate at the first `#[cfg(test)] mod tests`.** Every
   file sorting after that one was discarded. `apps/editor` is four files;
   `highlight.rs` sorts first, so `main.rs` -- the one holding the file dialog
   -- was thrown away. **The door model of the entire sweep appeared on the
   list of programs that have no door.** 2 crates.

3. **Test code cut at `mod tests` rather than blanked per item**, so a
   `#[cfg(test)]` helper elsewhere survived -- and a test's assertion message
   is written in exactly the vocabulary this check looks for. `apps/chess` was
   excused by `"the king cannot move at all"`; `apps/mixer` by `"muting forgot
   where the fader was, so unmuting cannot put it back"`. **18 crates.** A
   test's failure message is not something the user reads.

4. **The capability regex run over raw source.** A mention in a comment counted
   as the real thing. `apps/undelete`'s module header says the crate "contains
   no reference to `std::fs` or `safeio`" -- so **the sentence denying the
   capability was itself the evidence that the crate had it.** 7 crates.

### The fix

All four are the same fix: ask the question of the right text. Literals come
from `rustlex.string_literals`, code from `rustlex.strip_noise`, and test code
goes via `rustlex.live_code`, which blanks every `#[cfg(test)]` item by brace
matching rather than cutting at the first one.

`scripts/rustlex.py` already existed for precisely this -- it was written
because twelve scripts had each rolled their own masker and three got raw
strings wrong. **I nearly destroyed it**: I wrote a second lexer over the top
of the 408-line original, whose opening line is "One Rust lexer for the
checkers". The pre-push hook caught it, via a caller whose self-test I had
broken. The lesson is the one the module's own docstring already makes, and it
took a second demonstration: when a shared helper looks absent, look harder
before writing another.

`string_literals` is an addition to it rather than a replacement, and derives
its spans from the two passes already there rather than lexing a third time.

### The residual worth knowing

The count is now 58, and most of the list is games -- a sudoku needs no
filesystem and owes nobody an explanation. **The output is a list to read, not
a list to empty.** The number going up was the point; it is not a backlog.
