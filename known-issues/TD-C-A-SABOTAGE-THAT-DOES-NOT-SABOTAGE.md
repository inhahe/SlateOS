## TD-C-A-SABOTAGE-THAT-DOES-NOT-SABOTAGE -- METHOD 2026-09-15

**In short:** the only way to know a test would catch the bug it was written
for is to introduce that bug and watch the test fail. Doing that is cheap and
it caught real gaps repeatedly today. Doing it *wrong* is also easy, and twice
today a broken sabotage reported a passing test as covered — or a covered test
as broken. This records how to do it and the two ways it misleads.

### Why it is not optional

Three tests written this week passed for reasons unrelated to what they
claimed:

* `apps/podcast` — "playback did not start while the picker was up" held
  because the fixture had no episode selected, so Space did nothing either way.
* `apps/photomanager` — "the slideshow advanced" was measured on a counter that
  a tick past the interval **resets to zero**, so the assertion was false at
  exactly the moment the slideshow did move.
* `apps/kanban` and seven others — `assert!(picker.is_open())` was assumed to
  cover "the picker is drawn". It does not, and `apps/flashcards` shipped a
  dialog that took every keystroke and painted nothing.

None of those was found by reading. All three were found by breaking the
program and noticing the test did not care.

### The two ways the sabotage itself lies

**1. A build error is not a red test.** Deleting a render call with a regex
left `frame.extend(` dangling. The crate did not compile; a check looking for
`test result: FAILED` saw no such line and reported *five* tests as failing to
notice. They were fine. This is the second time this session — `apps/rssreader`
cost three attempts for the same reason. Always distinguish "the test failed"
from "the build failed"; they are different words in the output and mean
opposite things about the test.

**2. A sabotage that does not reach the code under test.** The first attempt at
disabling a picker's drawing removed one app's render line, which says nothing
about the other seven. **Sabotage the shared implementation instead**: making
`FilePicker::render` return an empty `Vec` reddens every caller's test at once,
cannot be confused with a compile failure, and is one edit to revert.

### What makes an assertion resistant

The `fileassoc` case is the sharpest. The test counted frame commands before
and after opening the picker and asserted growth — and that app draws a
**scrim** in the same arm, so the count grew whether or not the picker did. The
assertion was satisfied by a neighbour's output.

The repair is to assert the specific quantity rather than a proxy for it:

```rust
let own = app.picker.render(&app.palette, w, h).len();
assert!(own > 0, "the picker itself draws nothing, so this proves nothing");
assert!(after >= before + own, "...something else grew instead");
```

The first line is a **control**: it refuses to let the test pass when the thing
it measures is trivially zero. Every test that has caught something this week
has one, and every test that passed for the wrong reason lacked one.

### The question to ask

Before trusting a green test: **what would have to change in the program for
this to fail?** If the answer is "an edit that does not matter", or "nothing I
can name", the assertion is pinned to the wrong thing — and it is worse than no
test, because it reads as coverage.

Applied to the eight picker tests, seven happened to pass with the weaker form.
They were strengthened anyway: *happening to* pass is the defect, not a
mitigating circumstance.
