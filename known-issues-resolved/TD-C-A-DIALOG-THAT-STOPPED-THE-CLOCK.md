## TD-C-A-DIALOG-THAT-STOPPED-THE-CLOCK -- FIXED 2026-09-15

**In short:** in five apps, opening a dialog quietly froze time. Podcast
playback stopped advancing, the reminders app stopped noticing that something
had become overdue, the calendar never rolled over to the next day, the photo
manager's slideshow stopped between one picture and the next, and **the file
manager stopped copying files** for as long as a confirmation was on screen. Nothing
crashed and nothing looked wrong; the window behind the dialog simply stopped
being told that time had passed. The bug was in code that eleven applications
had each written out by hand, and it was found by collecting that code into one
place rather than by anyone noticing the symptom.

### The shape of it

An app with a file dialog has to route events to the dialog while it is up, or
a keystroke meant for a filename reaches the window behind it. **Thirteen apps
in this tree do that** -- a count corrected twice, because the first grep
(`fn apply_dialog_action`) missed `fileassoc` and `photomanager`, which route
through a `file_dialog_event` helper instead, and a later one matched
`startupmanager`, where the string is in a *test name*.

Nine of the thirteen wrote the routing as an early return:

```rust
if self.file_dialog.is_some() {
    let action = match (event, self.file_dialog.as_mut()) {
        (Event::Key(key), Some(d)) if key.pressed => d.handle_event(key, h),
        (Event::Mouse(m), Some(d)) => d.handle_mouse(m, w, h),
        _ => return EventResult::Ignored,   // <-- here
    };
    return self.apply_dialog_action(action);
}
```

The `_` arm is meant to say "not an input event, nothing for the dialog to do".
What it actually does is **return from the whole event handler**, so
`Event::Tick` never reaches the application at all.

### What each one lost

| App | What the tick does | Consequence |
|---|---|---|
| `podcast` | advances playback and the download queue | audio stops because you opened Save |
| `reminders` | re-reads the clock, fires notifications | stops noticing what has become overdue |
| `calendar` | midnight rollover | "today" stays on yesterday, in blue, in five places |
| `photomanager` | advances the slideshow | the slideshow stops until the dialog is closed |
| `explorer` | retires a batch of a file operation | **a copy stalls** while any confirmation is open |

**The same defect appears in a third shape**, which is the point: it is not
tied to an idiom. `photomanager` and `fileassoc` route through a helper that
returns `false` (or `Some(Consumed)`) for anything that is not input, and the
caller returns on that -- so a tick never reaches the application there either.
`photomanager`'s `tick_interval` fires exactly when a slideshow is running,
which makes it the fourth live case. What the nine have in common is not a
syntax; it is **routing to the dialog and then returning, without deciding
which events that covers.**

The six early-return apps that never ask for ticks (`contacts`, `dbviewer`,
`jsonviewer`, `notes`, `rssreader`, `spreadsheet`) have it **latent rather
than live** -- it would have become a bug the day any of them grew a
clock, an autosave or a progress indicator.

`filesearch` and `hexeditor` wrote the intercept as guard arms on the outer
match instead:

```rust
match event {
    Event::Key(k) if self.file_dialog.is_some() => self.dialog_key(k),
    Event::Mouse(m) if self.file_dialog.is_some() => self.dialog_mouse(m),
    Event::Key(k) => self.handle_key(k),
    ...
}
```

which lets every other event fall through to its own arm. **Two of eleven got
it right, and they got it right by using a structure that made the wrong
answer inexpressible** rather than by thinking about ticks.

### Why nobody caught it

There was no test anywhere, in any of the nine. The bug lives in the arm nobody
thinks about: you write the intercept to solve the keystroke problem, you test
the keystroke problem, and the `_` arm is the part you wrote to make the match
exhaustive.

It is also invisible from the outside. Nothing errors. The window redraws on
the next keypress and the clock appears to catch up, so even someone watching
it happen would see only that the app was briefly slow.

### The fix

`guitk::dialog::FilePicker` owns the routing once. `FilePicker::handle` takes
**input** -- key presses, key releases and mouse events -- and returns
`Picked::Ignored` for **time and geometry**, so a tick or a resize falls
through to the application:

```rust
match self.picker.handle(event, w, h) {
    Picked::Chose(path) => { /* the one arm that differs per app */ }
    Picked::Handled => return EventResult::Consumed,
    Picked::Ignored => {}
}
```

A key *release* is still taken, so the window behind never acts on half a
keystroke whose press the dialog handled.

Each of the three live cases has a regression test that sets its clock to a
moment the real one cannot be at (1 Jan 2000), opens the picker, sends a tick
and asserts time moved. All three were watched to fail by restoring the
swallow.

### The fifth case, and why no amount of reading would have found it

`apps/explorer` is the worst of the five and the only one a scanner found.

The other four froze something the user *looks at* -- a clock, a slideshow, a
date. Explorer froze something the user is **waiting for**: its tick retires a
batch of a running file operation, so a paste stopped making progress for as
long as any confirmation or notice was on screen. Its own `tick_interval` asks
for the frame interval because a file operation is, in that function's words,
"the thing the user is watching".

**Every earlier instance was found by reading the thirteen apps that hold a
`file_dialog`.** Explorer holds a `modal`. No search for a dialog field would
ever have reached it, and by that point I had already been wrong twice about
how many callers there were -- both times by searching for one spelling of the
thing and reading the count back as complete.

So `scripts/find-swallowed-ticks.py` looks for the shape of the **consequence**
rather than the shape of the code: a `match event` whose arms mention
`Event::Tick`, with a `return` reachable before it. 72 dispatchers in `apps/`
have a tick arm; 6 can return before reaching one.

Most of those 6 are correct, and the script's docstring says so above the
results, because a checker whose hits are mostly noise teaches its reader to
skim. The two correct shapes both trip it:

* **`apps/diskimager` is the model.** It ticks the dialog itself *above* the
  guard, and the guard is `matches!(event, Event::Key(_) | Event::Mouse(_))`.
  It is the only place in this tree that **names the distinction between input
  and time**, and it names it in a comment as well: "A tick drives the
  confirmation's fade as well as the write's progress, and the dialog has to
  keep animating while it is up."
* Anything on `guitk::dialog::FilePicker` returns on `Picked::Handled` and
  falls through on `Picked::Ignored`, which is correct and indistinguishable
  from a bug at this level of analysis.

`apps/filesearch` and `apps/hexeditor` are not reported at all, and that is the
right answer for the right reason: they write the intercept as guard arms on
the outer match, so every other event meets its own arm **by construction**.
Two of thirteen got this right, and they did it by choosing a shape in which
the wrong answer cannot be expressed.

### The fix in explorer, and one thing it was careful about

The modal still receives the tick -- the toolkit's `AlertDialog` uses it for a
fade -- and the work behind it receives one too. The combination is `|` rather
than `||`: the operation must step even when the fade has already answered
"yes, there is something to redraw", and `||` would short-circuit past the call
rather than merely past its answer.

The tick arm's body moved into `tick_work()` so both paths call one function.
Leaving the modal path to repeat the two calls is precisely how the two copies
would drift, which is the lesson from the eleven hand-written intercepts one
level down.

### The abstraction then lost something too

Worth recording beside the rest, because it is the same failure one level up
and it was found the same way.

`FilePicker::handle` returned `Picked::Handled` both when the dialog had
consumed a keystroke and was still up, and when the dialog had **closed
itself**. Twelve of the thirteen callers cannot tell those apart and do not
need to. `apps/fileassoc` keeps an `ActiveDialog` enum beside the picker, so
Escape closed the dialog, the caller was told only "handled", and that enum
stayed on `ChooseFile` **with no picker on screen**.

Its own cancel test caught it -- and that test exists because somebody had
already noticed it would otherwise pass against a button that opened nothing:

    // Without this the test passes when the button does nothing at all:
    // "no picker is up" is what it asserts afterwards, and that is also
    // true of a button that never opened one.

`Picked::Cancelled` is a separate variant rather than folded into `Handled`,
so that **a caller with parallel state is made to say what it does about
cancellation** rather than inheriting a default that is wrong for it. The
twelve with no such state write `Picked::Handled | Picked::Cancelled` and are
right; the one that has it clears its enum, and its arm says why.

A wrapper that answers a narrower question than its callers ask is not
obviously wrong from inside the wrapper. Every one of its own tests passed.

### The transferable part

**Collecting duplicated code is how you find out the copies disagree.** In the
commit that added `FilePicker` I wrote that the eight hand-rolled
character-boundary truncation loops agreed -- I had read all eight -- and that
the extraction fixed no bug. That was true of the truncation loops. It was not
true of the intercepts, which I had not compared as carefully, and the
disagreement only became visible when a shared type forced a single answer to
"what should a dialog take?".

Duplication is not only a maintenance cost paid later. It is a place where
**two copies can already differ today and nothing reports it**, because each
one is locally plausible and no test compares them.
