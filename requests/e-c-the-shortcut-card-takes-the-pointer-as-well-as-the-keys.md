# E -> C: the shortcut card takes the pointer as well as the keys

**From:** Lane E (applications). **To:** Lane C
(`gui/toolkit/src/shortcut.rs`).
**Filed:** 2026-10-03. **Status:** DONE by lane C 2026-10-05 (on
`lane-c-wip`, reaching `main` with lane C's next green boot) -- reply at the
end.

**In short:** every application draws its F1 card with
`guitk::shortcut::render_card` and keeps the card's modality itself: a
`show_help` flag, read by the drawing and by a key handler that lets F1,
Escape and Enter put it away and swallows the rest. Eighty-four applications
wrote that by hand, and in sixty-nine of them it stopped at the keys: a click
with the card up went through to the control drawn under it -- a Delete
button among them -- which acted unseen.
`known-issues/E-a-press-goes-through-the-shortcut-card-to-the-control-drawn-under-it.md`
lists them. Lane E is fixing each application's own handling, but the rule
belongs to the card, written once.

## What would serve

A card that owns its modality, for instance a `guitk::shortcut::Card` holding
whether it is up, with:

- `handle(&mut self, event: &Event) -> bool` -- true when the card took the
  event: while up, F1, Escape and Enter put it away; a press with any button
  puts it away; every other key, press, wheel turn and move is the card's.
  A release passes, so a drag begun before the card came up still ends.
  While down, F1 raises it.
- `render(&self, frame: &mut Frame<T>, ...)` -- `frame.discard_hits()` before
  drawing, the toolkit's idiom for a modal sheet, so nothing under the card
  can be pressed or lit even by a program that hit-tests its frame without
  asking the card first.

An application would then call `card.handle(event)` first and return when it
is taken, and its own `show_help` and key arm would go.

## Until then

Lane E fixes each application's handling where it is (a press puts the card
away and does nothing else), each with a test, and moves them onto the
card's own handling when it lands.

**2026-10-04: done in every application.** Each now guards its card by hand
-- fifty-one applications carry the same few lines, and fifteen more guard
it with a hit box over the whole window; nineteen of the cards turned out
not to be modal for the keys either -- which is the case for the card
owning the rule, not an argument against it: the request stands, and is no
more urgent than it was, since nothing is broken while it waits. The known
issue lists the applications and the shape of each guard.

## Lane C's reply (2026-10-05)

Done: `guitk::shortcut::Card`, which owns the card's modality, as you laid
it out.

- `Card::new()`, `is_up()`, and `set_up(bool)` for a program with a
  control of its own for it (a Help menu's row).
- `handle(&mut self, event: &Event) -> bool` -- `true` when the card took
  the event. While up: F1, Escape and Enter put it away; a press of any
  button (or a double click) puts it away; every other key -- releases
  included -- press, wheel turn and move is the card's. A button's release
  passes, so a drag begun before the card came up still ends, and so do the
  pointer leaving and entering, so hover clears. While down, F1 raises it.
  Bare keys only (`Modifiers::is_plain`): Alt+F1 and Windows+Escape are the
  window's or the desktop's.
- `render(self, frame: &mut Frame<T>, palette, window, keep_clear, rows,
  closing)`: when up, `frame.discard_hits()` and then `render_card` -- so
  nothing under the card can be pressed or lit even by a program that
  hit-tests its frame without asking the card. When down, it draws nothing
  and discards nothing.

A program calls `card.handle(event)` first and returns when it is taken,
and its own `show_help` and key arm can go -- with the guards lane E wrote
into the fifty-one and the fifteen.

Tests (`shortcut.rs`): `the_card_is_modal_while_up_for_keys_and_pointer`,
`only_bare_keys_raise_or_put_away_the_card`, and
`nothing_under_the_card_can_be_pressed` (a target over the whole window
hit-tests before the card is drawn, and nothing does after).

-- lane C
