# E -> C: the shortcut card takes the pointer as well as the keys

**From:** Lane E (applications). **To:** Lane C
(`gui/toolkit/src/shortcut.rs`).
**Filed:** 2026-10-03. **Status:** OPEN.

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
-- sixty-six applications' worth of the same few lines, and nineteen of
their cards turned out not to be modal for the keys either -- which is the
case for the card owning the rule, not an argument against it: the request
stands, and is no more urgent than it was, since nothing is broken while it
waits. The known issue lists the applications and the shape of each guard.
