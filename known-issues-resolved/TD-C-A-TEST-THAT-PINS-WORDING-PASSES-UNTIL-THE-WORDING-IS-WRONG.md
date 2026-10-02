## TD-C-A-TEST-THAT-PINS-WORDING-PASSES-UNTIL-THE-WORDING-IS-WRONG -- FIXED 2026-09-15

**In short:** four apps had a test asserting that a warning message contained a
particular phrase. Each of those phrases later became untrue, and every one of
those tests went on passing — because the words were still there. The test was
guarding the sentence rather than the thing the sentence was for. All four now
assert the property instead.

### The four

| app | the phrase it pinned | what made it false |
|---|---|---|
| `contacts` | "Nothing is saved between runs" | a vCard door |
| `diagram` | "gone when the window closes" | a save door |
| `flashcards` | "review schedule resets" | a deck door that keeps schedules |
| `mindmap` | "gone when the window closes" | an outline door |

In each case the app gained a way to save, the banner had to change, and the
assertion that was supposed to protect the banner **was the last thing to
notice**. Three of the four were found only because the banner edit made the
test fail; the fourth was found by reading the other three.

### Why the shape is so easy to write

The message is a constant a few lines above the test:

```rust
const NO_CONTACTS_LINES: [&str; 2] = [
    "No contacts.",
    "Nothing is saved between runs -- this app has no filesystem access, ...",
];
```

so the literal is *right there*, and asserting on it feels like asserting on
the thing. It is not. **A phrase is an implementation of a promise, and a test
that pins the implementation cannot fail when the promise stops being kept.**

It is worse than an untested banner, because it reads as coverage. Someone
changing the wording sees a test named
`the_window_says_what_it_cannot_do`, sees it pass, and concludes the window
still says what it cannot do.

### What to assert instead

Ask what has to be true for the message to do its job, and assert that:

```rust
// before: the phrase
assert!(LINES.iter().any(|l| l.contains("gone when the window closes")));

// after: the property -- the reader must learn both halves
assert!(LINES.iter().any(|l| l.contains("Ctrl+S")),
        "the banner does not say how to keep the work");
assert!(LINES.iter().any(|l| l.contains("not opened again")),
        "the banner does not say the diagram cannot be reopened");
```

Those still match on substrings — there is no way to assert "this sentence is
true" — but they match on the **load-bearing** part, the bit whose absence is
the defect. Changing "press Ctrl+S" to "use Ctrl+S" keeps them green, which is
right: that edit does not break the promise. Removing the remedy breaks them,
which is also right.

### A fifth, found by searching rather than by a red test

`apps/calendar` pinned "gone when the window closes". It gained an iCalendar
door hours before the other four were found, and its test **never failed** —
because that phrase happens to still sit at the end of the rewritten sentence:

    "Nothing is saved automatically -- press Ctrl+S to write an .ics file,
     or an event added today is gone when the window closes."

So the assertion survived by luck, drew no attention to itself, and never
checked the half the banner had just gained. It is the most instructive of the
five for exactly that reason: **the other four were found because they broke.
This one could only be found by looking.** A test that pins a phrase does not
reliably fail when the promise changes; whether it fails is an accident of
which words the rewrite happened to keep.

### Where else this shape lives, and why it is a checklist rather than a backlog

Fifteen tests in `apps/` use the idiom `LINES.iter().any(|l| l.contains("..."))`
against a banner constant. The eleven not listed above are **true today**:

    alarmclock   "nothing will wake you"
    clipmanager  "however much you copy"
    credmanager  "Do not rely on it"
    devicemanager "not because the machine has no devices"
    email        "nothing was ever fetched"
    filediff     "no left file and no right file"
    finance      "no way to add an account"
    logviewer    "not a quiet system"
    mediaconvert "do not delete an original"
    habits       "Nothing is saved between runs"
    whiteboard   (drawings cannot be kept)

Rewriting them now would be churn against assertions that are not yet wrong,
and each rewrite risks weakening a check that currently works. They are left
alone deliberately.

What they are is a **list of the exact tests that will go stale on the day each
of those apps gains the capability its banner denies** — which, for most of
them, is the day it gets a door. `habits` is the clearest: its phrase is
word-for-word the one `contacts` had, and `contacts` needed it rewritten within
an hour of its vCard door landing.

So the entry to act on is not "fix these fifteen". It is: **when adding a
capability to an app, grep its tests for `contains(` before editing its banner**
— the assertion that was supposed to protect the banner is the last thing that
will tell you.

### The general form, which is not about banners

This is the same failure as a test that passes because its fixture had nothing
to act on, and as a checker whose green result was computed over the wrong
corpus. In all three the result is *true* and answers a question nobody asked:

* the fixture had no selected item, so "nothing was deleted" held trivially;
* the gate scanned `gui`/`apps`/`scripts`, so "no collapsed messages" said
  nothing about `kernel/`;
* the phrase was still in the constant, so "the window says it" held while the
  window said something false.

**Green is only as meaningful as the question it answers.** The check worth
making on any passing assertion is: *what would have to change in the program
for this to fail?* If the answer is "an edit that does not matter", the
assertion is pinned to the wrong thing.
