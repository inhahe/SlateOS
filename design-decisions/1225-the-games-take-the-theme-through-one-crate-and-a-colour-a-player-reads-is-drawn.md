## 1225. The games take the theme through one crate, and a colour a player reads is drawn so it can be read

**Date:** 2026-09-28
**Lane:** E
**Decided by:** Claude (operator-approved scope) -- the operator decided the
rule (§1422, C-Q16: "C, and I'll let Claude decide which games get which
treatment ... try to make the games look as polished as possible"); how the
thirty-five games carry it out is decided here.

**In short:** every game draws its window, menus, panels, dialogs and
buttons from the user's palette through one small crate, `apps/gamechrome`,
so they follow the theme together and look alike. On the board, a colour a
player reads -- a tetromino, a minesweeper digit, a wordle square -- keeps
its identity: the same hue for the same thing in every theme. Where such a
colour is text on a tile, it is drawn in the shade of that hue that can be
read on the tile, light or dark, rather than a pastel that vanishes on a
light tile.

| Call | Chosen | The other way, and why not |
|---|---|---|
| Where the chrome's colours come from | `gamechrome::Chrome::of(&palette)`: fifteen roles named for what they are in a game (page, well, raised, lit, text, dim, off, good, bad, even, title, ring, key, scrim, veil) | each game mapping palette entries itself: thirty-five mappings of the same fifteen roles would drift the way the thirty-five Mocha copies did |
| The buttons | the toolkit's push button's colours (`guitk::button::paint`) with the label at the game's own size (`gamechrome::button`) | `guitk::button::draw`: its label is a fixed 13 pixels, and a game's chrome scales with its window |
| A game whose two sides both follow the theme (tic-tac-toe, nim) | the first side the accent, the second `gamechrome::apart_from_accent` -- the first of red, peach, green, mauve, yellow, teal that `hard_to_tell_apart` does not call too close to it | two fixed hues: a user whose accent is one of them could not tell the sides apart |
| A colour a player reads, drawn as a fill (a piece, a disc, a gem) | its own value, in either mode | the palette's hue of the same name: a theme could make two pieces alike, and the pieces are how a player tells them apart |
| A colour a player reads, drawn as text on a tile (minesweeper's digits, 2048's numbers) | its hue in the shade that reads on the tile, chosen by contrast (`gamechrome::legible_on`): a light shade on a dark tile, a deep one on a light tile -- for minesweeper, the classic colours themselves | one value: the dark theme's pastel yellow is unreadable on a light tile, and "read at a glance" is the reason it keeps its colour at all. Catppuccin Latte's shades were tried for the light tile and are too pale for digits (green 2.9:1, yellow 2.2:1) |
| The guard | each game renders its states in both modes under `appearance::palette_check::assert_drawn_from`, naming its own colours as the ones not drawn from the palette | counting constants: a game can have none and still draw a literal in a hover branch (the twelve applications' lesson) |
