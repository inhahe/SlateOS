## 1479. A widget tree signals what its user did, and a program hears it three ways

**Date:** 2026-10-05 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** a program built on the toolkit's widget tree could not be told
that a button was clicked or a box ticked -- it had to look at every widget
after every event to see what had changed. Now each control says what its
user did (a *signal*: clicked, toggled, chosen, edited, submitted, moved),
and the program hears it whichever way suits it: by taking the signals after
each event, on a channel, or through a callback connected to one widget or
to all. Only what the user does is signalled, never a change the program
makes itself.

**Where:** `gui/toolkit/src/widget/signal.rs` (`Signal`, `SignalKind`,
`SlotId`), `WidgetTree::take_signals`, `connect`, `disconnect`,
`connect_channel`. `roadmap-detailed.md` §3.5, *Signals and Slots*: "maps to
Rust channels or callback registration".

| Choice | Instead of | For | Against |
|---|---|---|---|
| **A typed signal per control** -- `Clicked`, `Toggled(state)`, `Chosen`, `Edited`, `Submitted`, `Moved(value)` -- from the widget's id | a generic "changed" | A `match` on the kind is the program's whole handler; a toggle carries the state it is in now, a slider its value, so a program does not read the widget back. | A new kind of control adds a kind. |
| **Three ways to hear, all fed alike**: a queue taken after each event, channels, callbacks | one of them | The queue is the idiom for a program that owns its state -- a Rust closure cannot borrow it; a channel is for state on another thread; a callback is what "slots" means elsewhere and suits a program whose state is shared already. | Three APIs to learn; each is a few lines. |
| **The queue is bounded** (`MAX_QUEUED_SIGNALS`, 1024), its oldest dropped | an unbounded queue | A program that listens on a channel or a callback and never takes the queue must not keep every signal it was ever sent. | One that takes the queue only now and then can miss the oldest of a long burst. |
| **Only the user's actions are signalled** | signalling every change of state | A program that ticks a box knows it did; hearing its own change back is how a program ends up answering itself in a loop. | A program that wants to hear its own changes must do so itself. |
| **A callback cannot reach the tree** -- it gets the signal, and what it captured | handing it `&mut WidgetTree` | Handing the tree to a callback while the tree is handing out signals is a second mutable borrow; a program that must change the tree takes the queue instead. | A callback that would change a widget must go through the program's state. |
| **Space or Enter clicks a focused button; a press released off a button is no click, and lets it go** | the old toggle | Space and Enter toggled the button's pressed look and left it stuck -- releases are not seen -- and a press dragged off and released stayed pressed until a later release anywhere over it, which then clicked it. | -- |
