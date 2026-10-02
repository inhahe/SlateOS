## 806. The file chooser stores no size of its own, and lets an explicit scroll leave the selection off screen

**Date:** 2026-09-03
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** making `guitk::dialog::FileDialog` clickable also forced it to
scroll, since a list you can click but cannot scroll still cannot reach most of
a directory. Two choices inside that work had a real case on both sides. The
first: the widget does *not* remember how big it is, so every method that needs
to know is handed the width and height — which is why three applications had to
change a function signature they were otherwise not touching. The second: the
list follows the selection when a *key* moves it, but the mouse wheel and the
scrollbar are allowed to scroll the selection right off the screen and leave it
there.

### The first decision: the widget is told its size, it does not remember it

`render(&self, width, height)` takes the size and `&self`, so it cannot write
anything back. If the widget also kept a `size` field, there would be two
answers on file to "how big is this dialog" — the one the renderer was just
handed, and the one the widget remembers from some earlier frame — and nothing
would keep them equal. A window resized between two frames would leave the
stale one behind, and the scrolling arithmetic reads that size to decide how
many rows fit.

| | Store the size at render time | Pass the size to every method that needs it |
|---|---|---|
| *What changes:* | `handle_event(key)` keeps its old one-argument shape; after a resize the first click or keystroke can scroll to the wrong row | `handle_event(key, height)` — every caller has to say how tall it drew the dialog |
| Cost | a divergence that only appears after a resize, i.e. the case nobody tests | three call sites in two applications had to be edited, and every future host must pass a size it already knows |
| Failure mode | silent and wrong | a compile error |

This is the same class of bug as recomputing a hit box in the caller, which
`C-NETMANAGER-CLICKED-ROWS-THAT-WERE-NOT-ON-SCREEN` records: two copies of one
fact, and the bug lives in whichever copy you are not reading. The toolkit
already settled that argument once for geometry; a stored size is the same
argument about a smaller number. The cost — an extra parameter — is paid at
compile time by a caller that has the number in hand anyway.

Against it: a widget that cannot answer "how tall am I" is slightly awkward to
build tooling around, and the parameter is a wart on an otherwise tidy
signature. Both are real; neither is a wrong answer at runtime.

### The second decision: the scroll follows the keyboard, not the selection

The obvious implementation of "keep the selected row visible" is to enforce it
in `render`: before drawing, scroll so the selection is on screen. That is
wrong, and not subtly — it makes the wheel useless. Every notch the user
scrolls is undone by the very next frame, because the selection has not moved
and render puts the window back on top of it. A scrollbar drawn under that rule
would be draggable and immovable at once.

So reveal is *stateful*: `move_selection` scrolls to bring the new selection
into view, and nothing else does. The wheel, a track click and a thumb drag
change the offset and leave the selection wherever it was.

| | Reveal at render time | Reveal only when a key moves the selection |
|---|---|---|
| *What changes:* | the wheel appears to do nothing on a list with a selection | the user can scroll away from the highlighted row and see no highlight at all |
| Cost | the mouse cannot scroll, which is the whole point of the change | Enter/Open acts on a row that may be off screen |

The second cost is the one to weigh, and it is what every file manager and text
editor already does — scroll away from the cursor and the cursor stays put.
Confirming still opens the selected file, which is the row the user last
*chose*, not the row that happens to be under the scroll. The alternative
reading — that Open should act on nothing when the selection is off screen — is
worse: it makes a working command fail for a reason the user cannot see.

`the_wheel_may_scroll_away_from_the_selection` and
`keyboard_selection_scrolls_the_list_to_follow_it` pin the two halves against
each other, so neither can be "fixed" into the other without a failure.

### Reversing either

The first is mechanical to reverse — add the field, drop the parameters — and
should only be done if some caller genuinely cannot know the size it drew at,
which none does today. The second is a behaviour change a user would notice; if
it is ever reversed, the reversal must not be "reveal in `render`", but a
scrollbar-aware reveal that distinguishes a scroll the user asked for from one
the widget imposed.
