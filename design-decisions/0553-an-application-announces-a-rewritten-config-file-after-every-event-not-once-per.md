## 553. An application announces a rewritten config file after every event, not once per batch of them

**Date:** 2026-08-25
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** Some settings live in files that a *different* program reads. When
you pick a new accent colour, Settings writes `appearance.yaml`, but it is the
compositor — the program that actually paints every window's frame — that reads
it. So Settings has to say "I changed that file" out loud, or the new colour
shows up in Settings' own preview and nowhere else until you log out and back
in. The shared window harness now asks each application, after each thing the
user did, whether it rewrote such a file, and passes the news along. The choice
recorded here is *how often to ask*: after every single event, or once at the
end of each group of events the window delivers together. We ask after every
event, and pay one extra message to the compositor for it.

Some terms, since the rest of this entry uses them:

- **The harness** is `gui/window/src/app.rs` — the ~130 lines of connect / build
  a window / draw / loop that used to be copy-pasted into each application's
  `main`. `known-issues.md` → `TD-NO-APP-CONNECTS-TO-THE-COMPOSITOR` records why
  it was written.
- **A batch** is the group of events the compositor hands over in one go — what
  arrived since the application last looked. A single mouse drag is typically
  many events in one batch.
- **`Settled`** is the harness's "that's all of them for now" callback, fired
  after a batch is drained. It is where a per-batch version of this would live.
- **Draining** means the application hands over its record of what it wrote and
  clears it in the same motion, so the same news is not repeated for ever.

## The decision

`App::take_reloads` is called once per event, *before* the application's answer
to that event is acted on — not once per batch from `Dispatch::Settled`.

*What changes:* dragging a colour slider sends the compositor one "re-read
`appearance.yaml`" message per slider position instead of one per drag; and a
setting changed by the last click before you close the window takes effect
immediately instead of at next login.

### Why not the per-batch version, which is cheaper

Coalescing at `Settled` is the obvious design and it is genuinely better on
traffic: one notification per batch, so a fast drag over a slider costs one
round trip instead of thirty. It was rejected for one reason, found by reading
the loop rather than by taste.

**`run_batched` never reaches `Settled` after a close.** In
`gui/window/src/lib.rs`, dispatching `CloseRequested` sets `self.running =
false` and breaks out of the batch, and the `Settled` call is guarded by
`self.running`. So the *last* batch of a window's life — the one containing
whatever the user did immediately before clicking the X — has no `Settled`.

A per-batch drain would therefore lose exactly one class of change: the setting
you altered with your final click. That is the worst possible one to lose,
because it is written to disk and *looks* saved. Reopening Settings shows the
new value; every window on screen keeps the old corners until the next login.
It is `known-issues.md` lesson — a setting that appears to work and does not —
reproduced by an optimisation.

A second, smaller reason: an application may rewrite a file while answering
`Response::Idle`, because nothing in *its own* window changed. The drain
therefore also has to sit ahead of the redraw decision, not behind it. Both
constraints point the same way.

### Why not fix `run_batched` to always fire `Settled`

The other repair is to make the close path dispatch `Settled` before it stops,
which would make per-batch coalescing safe. Rejected, because the omission is
not a bug — it is what `Settled` is *for*. `run_batched`'s own doc comment says
so: `Settled` is where "the one frame that is not stale" is drawn, and it never
fires on the way out because "an application on its way out has nothing to
show". Firing it on the close path would mean submitting a frame for a window
that is already closing, on every application in the tree, so that one
application could save one message per batch.

That is the general shape of it: `Settled` answers "when should I draw?", and a
file rewrite is not a drawing question. Hanging the announcement off it was
borrowing a callback for a second meaning, and the close path is where the two
meanings come apart.

### What the cost actually is

One request per event, only from applications that implement `take_reloads`.
Today that is exactly one application in the tree — Settings — and only on the
events that actually write a file, because the default implementation returns
an empty record and `announce_reloads` sends nothing for it. So the "thirty
messages per drag" worst case requires a control that writes YAML on every
intermediate position, which is a thing worth not building for its own reasons.
If one is ever built, the fix is to make *that control* write on release rather
than to make the harness forget.

### The shape of the record, and why two flags rather than one

`Reloads { appearance: bool, input: bool }` — two separate flags, becoming two
separate requests, rather than one "something changed". The compositor acts on
them differently: a new double-click interval must not repaint the desktop, and
a new accent colour must not have the compositor re-read the pointer
configuration. A single notification would make every accent-colour change cost
a pointer re-read.

The record carries *no settings*, only the fact that a named file was
rewritten — continuing §523's rule. A client cannot swap the user's mouse
buttons by asking; it can only say "the file you read is not the one you have".

### Tested by

`gui/window/src/app.rs` → the six tests under
`// Announcing a rewritten configuration file`. Note that five of them script
*separate batches* through `TestDesktop::script` rather than using
`inject_event`, which puts everything in one batch: the first draft used
`inject_event` throughout and every test failed together under falsification,
which proved they were one test wearing six names. Only
`the_last_change_before_the_window_closes_is_still_announced` distinguishes the
two placements, and it is the one this entry is about.
