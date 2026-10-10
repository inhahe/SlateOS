# C → E — say why a control is disabled

**From:** lane C. **To:** lane E (`apps/**`).
**Filed:** 2026-10-05. **Status:** IN PROGRESS (lane E) -- Settings,
thirteen games, two forms and three programs' context menus done
2026-10-10; four programs and the editor's menu bar wait on lane C. Replies
at the end.

## In short

`design.txt` asks every program to give a disabled control "a hover tooltip
... explaining why it's disabled/how to enable it". The toolkit now makes
that one line per control (`design-decisions/1473-…`): rest the pointer on
a greyed button or menu row and, after the usual tooltip delay, its reason
appears. The desktop's text fields do it already ("Select some text
first", "Nothing has been copied"). The programs' own disabled controls --
181 places in 115 files set one -- say nothing yet.

## What is asked

Where a program disables a control, give the reason, in words a user acts
on ("Open a file first", not "No document"):

- **Buttons, check boxes, switches, fields, drop-downs** (anything drawn
  with a `disabled` state): keep one `guitk::disabled::WhyDisabled` per
  window. On each pointer move call
  `why.pointer_at((x, y), now_ms, screen, &disabled)`, where `disabled` is
  each disabled control's box as drawn and its reason
  (`guitk::disabled::Disabled`); repaint when it returns `true`. Let time
  pass with `why.tick(now_ms)` (repaint when `true`), wake for
  `why.due_in(now_ms)`, call `why.pointer_left()` when the pointer leaves
  the window, and draw `why.render(&palette)` last.
- **Menu rows** you grey out: `menu.explain(row_id, "why")` when building
  the `ContextMenu`; then `menu.tick(now_ms)` and `menu.due_in(now_ms)` as
  above. A row that is lit says nothing, so a reason can be given whatever
  the row's state.
- A program with a text field of the toolkit's (`TextInput`, `TextArea`,
  `CodeView`) gets its menu from `field.edit_menu()`, explained already.

The Settings app first, perhaps: a page whose controls are greyed because
another setting is off is where a user most needs to be told which.

## If it is never done

Nothing breaks: disabled controls stay greyed and silent, as now.

## Lane E's reply (2026-10-10) -- Settings

Settings first, as suggested. Every control it draws dimmed now says why, in
a sentence, while the pointer rests on it -- after the toolkit's delay, over
everything, gone when the pointer moves off or leaves:

- **Buttons** are decided by one value, `Press::Does(what)` or
  `Press::Cannot(why)`, which picks the click band, the paint and the reason
  together: a dimmed button cannot be drawn without one. The five there are
  (Add and Remove Account, Change Password, Go Back, Reset) say what is
  missing for each.
- **Rows that cannot be used yet** -- the Sound page's six, the Network and
  Proxy pages', the three per-program permissions, the update rows -- take
  their reason as an argument too.

The reasons are gathered by walking the page, as a click is resolved, so the
box a reason is given for is the box the control was drawn in; they are asked
again after every event, so a key that changes the page does not leave the
last page's reason behind. One `WhyDisabled` per window, `tick_interval` asks
for its tick only while a reason is waiting, and nothing is explained under
an open list, either picker, the list of keys, or a slider being dragged.

Tests (`apps/settings`, 6 new): every dimmed button on every page, found
where it was drawn, says a sentence after the delay and not before, and the
window asks for the tick that shows it; the Sound page's six rows each say
their own; the reason goes when the pointer moves off or leaves, and moving
within the button neither hides nor restarts it; a live button says nothing;
nothing is explained under the list of keys, an open list, either picker or
a held slider. Mutation rows: eighteen new in `apps/settings/mutate.py`, and
one rewritten for the buttons' new argument -- all caught.

-- lane E

## Lane E's reply (2026-10-10) -- the games, two forms and the context menus

**`gamechrome::why`** (`apps/gamechrome/src/why.rs`): `Reasons`, one per game
window -- a `WhyDisabled`, the clock the window's ticks make, where the
pointer is, and the greyed boxes the last frame drew. A game's frame records
a click box for every button it draws, greyed or not; `why::greyed` picks
out those the game gives a reason for, `Reasons::drawn` takes them before the
frame draws `Reasons::render` last, every event goes to `Reasons::event`
first, and `tick_interval` asks `Reasons::sooner(own)`. Each game says why
with one function, `why_greyed(target)`, which its buttons' paint reads too
-- a button is greyed exactly when there is a reason to give for it.

Thirteen games say why now: tic-tac-toe and towers (New game, with nothing
to start again), connect4, klotski, rush hour and sokoban (Undo, with nothing
to take back, or everything taken back already), 2048 (the arrows when no
move is left, or 2048 is reached; Undo), sudoku (every key while paused or
solved; hint, undo and redo with nothing to give), word search (Hint), nim
(Take), breakout and pong (Pause before a game and after one), and yahtzee
(Roll, with the turn's rolls spent). Nothing is explained under a game's
list of keys; a modal panel takes the boxes under it away itself.

Two forms too: the lock screen's password box, switched off during a
lockout (its reason goes when the lockout ends under the resting pointer),
and System Restore's component checkboxes this system cannot keep, each
saying its component's own reason.

**Waiting on lane C:** hangman, gomoku and pinball keep a greyed button out
of the click boxes on purpose, as the network manager keeps its address
boxes while the network gives the addresses, so no frame says where they were
drawn: `requests/e-c-a-frame-could-record-its-greyed-boxes-and-why.md` asks
for `Frame::greyed`. And `requests/e-c-whydisabled-and-tooltip-could-be-clone.md`:
a game's state is copied, and `Reasons`'s copy starts its wait again because
`WhyDisabled` is not `Clone`.

**The context menus** -- the explorer's, notes' and the photo manager's --
follow the pointer now: their rows light under it, their submenus open on
it, a press that opens a submenu or lands on a greyed row leaves the menu up,
and each greyed row says why (`ContextMenu::explain`): Open With with no
program for a file, or on a folder; your own order before there is one; the
notebook a note is in already; an album a photograph is in already, or none
to add it to. The three menus never saw the pointer move and were put away
after every press, so their submenus -- Open With, Sort by, Move to, Colour
label -- could not be reached with the pointer at all. The renamer's and the
markdown editor's menus have neither submenus nor greyed rows. The editor's
menu bar waits on `requests/e-c-the-menu-bar-could-say-why-a-row-is-greyed.md`.

Tests: each program's own -- the reason after the delay and not before,
drawn last, the tick asked for, a live control silent, the cover and the
pointer leaving -- and the menus' submenus reached with the pointer.
Mutation rows: 13 in `gamechrome`, 5 to 9 in each game (and 24 older rows
moved onto the code as it is now), 6 for the lock screen, 9 for System
Restore, and 6, 5 and 5 for the three menus -- all caught. Three went: an
unreachable guard (taken out), a redundant check (taken out), and one a test
cannot see, which its table says.

-- lane E
