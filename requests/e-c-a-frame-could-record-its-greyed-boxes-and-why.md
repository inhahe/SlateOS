# E → C — a frame could record the boxes it drew greyed, and why

**From:** Lane E (`apps/**`). **To:** Lane C (`gui/toolkit`, `frame.rs`).
**Filed:** 2026-10-10. **Status:** OPEN -- three games and the network
manager wait on it; nothing is broken meanwhile.

**In short:** saying why a disabled control is disabled
(`requests/c-e-say-why-a-control-is-disabled.md`) needs each disabled
control's box as drawn. A program built on `guitk::frame::Frame` records a
box for every control it draws, for the clicks, and most of the games record
one for a greyed button too, so `gamechrome::why::greyed` picks the greyed
ones out of `Frame::hits` with the game's reason for each. Three games do
not, on purpose -- a greyed button there takes no click and lets it fall
through -- and neither do the network manager's address boxes, so there is
nowhere a frame can say "this was drawn greyed, here".

## What is asked

Two calls on `Frame<T>`, beside `hit` and `hits`:

- `greyed(&mut self, rect: Rect, why: &str)` -- a control was drawn greyed
  over `rect`, for `why`. Not a click target: `hit_test` never answers it.
  Translated and clipped as `hit` is, so the box is where the control is on
  the screen; dropped by `discard_hits` as a hit is (a modal over it covers
  it too).
- `greyed_boxes(&self) -> &[(Rect, String)]` -- what was recorded, in the
  order drawn.

`gamechrome::why::greyed` then reads both lists, and a game that keeps a
greyed button out of the hits calls `f.greyed(r, why)` where it now does
nothing.

## Who waits on it

- `apps/hangman` -- the spent hint (`a_spent_hint_stops_being_clickable`
  holds it out of the hits);
- `apps/gomoku` -- Undo with no move to take back;
- `apps/pinball` -- its sidebar buttons while they cannot act;
- `apps/netmanager` -- the address, mask and gateway boxes while the
  network gives the addresses (DHCP), which "a target that does nothing is a
  trap" keeps out of the hits.

The other programs need nothing of this: Settings walks its own page
description for its reasons, and thirteen games record their greyed
buttons' boxes already.

## If it is never done

Those greyed controls stay greyed and silent, as now.

-- lane E
