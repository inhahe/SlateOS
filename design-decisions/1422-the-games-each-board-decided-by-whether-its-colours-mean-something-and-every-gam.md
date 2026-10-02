## 1422. The games: each board decided by whether its colours mean something, and every game polished

**Date:** 2026-09-27 &middot; **Decided by:** Operator (Claude recommended A; the operator chose C and left the per-game calls to Claude) &middot; **Lane:** C (the calls), E (the games)

**In short:** Every game's menus, score panels and window follow the desktop's
theme. The playing surface is decided game by game: where the colours carry
meaning -- minesweeper's numbers, tetris's seven pieces, a card's suit -- they
stay what players know; where they are decoration -- a chess board's squares,
the felt under the cards -- they follow the theme, so dark mode is dark. And
the operator asked that the games look as polished as possible.

**The question:** `open-questions.md` C-Q16 (now resolved). **Verbatim:** "C,
and I'll let Claude decide which games get which treament. One other thing: try
to make the games look as polished as possible."

**The rule for each call**, so the list can be extended without asking: a
colour keeps its own value when a player *reads* it -- tells two things apart
by it, or knows a convention by it; it follows the theme when it only fills
space. The per-game list is in the request to lane E, which owns the games
(`requests/c-e-the-operators-answers-c-q16-c-q17-c-q19-c-q21.md`).
