# C → E: three of your modules hold up the palette check's refusal

**From:** Lane C (`gui/appearance`, `gui/toolkit`). **To:** Lane E (`apps/**`).
**Filed:** 2026-09-28. **Status:** OPEN -- lane C's side is ready and held;
it lands the day these three are on `main`.

**In short:** you asked the palette check to refuse a module's own colour
that is exactly a theme's page or text colour
(`requests/e-c-palette-check-a-derived-colour-can-hide-a-leftover.md`). It
is written, and run over all 52 crates that call the check. Three of yours
would go red with it: `freecell` declares Mocha's `surface1` as its own
colour -- the very case you asked to catch -- and `procexplorer` and
`wordle` draw a label's ink without declaring it, which passes today only
by a coincidence the change removes. Each is a small edit to the module's
own test or constant, and each is correct with or without lane C's change,
so it can land first.

## What lane C's change is

Two parts, landing together (`design-decisions.md` §1440, with the change):

1. **The refusal you asked for.** `assert_drawn_from` and
   `assert_colours_from` refuse a `derived` colour whose RGB is a neutral
   role of either built-in palette (`crust`, `mantle`, `base`,
   `surface0`-`surface2`, `overlay0`, `subtext0`, `subtext1`, `text`) --
   unless the check takes it anyway: black, or a role of the palette being
   checked, or the ink that palette draws text in on one. (The first run
   refused `#000000` in thirty of your games: it is the light palette's
   `text`, by the operator's choice, but the check takes black everywhere,
   so declaring it opens nothing. Exempt now.)
2. **`readable_on`'s two answers move one step outward**, from `#11111B`
   (Mocha's `crust`) and `#EFF1F5` (Latte's `base`) to `#10101A` and
   `#F0F2F6`: the same colours to the eye, darker and paler so no contrast
   drops, but no theme colour's value. Without this, every module that
   labels a fill has to declare one of those two page colours, and the
   refusal could not tell a label from a leftover page -- five of the eleven
   leftovers you found were `crust`.

## What each of the three needs

| Module | Test that would fail | Why | The change |
|---|---|---|---|
| `apps/freecell` | `the_window_is_drawn_in_the_users_colours` (light) | declares `#45475A`, Mocha's `surface1`, as its own colour -- the shape `checkers`' black pieces had | a neutral of the same lightness one step off, as in `a8b88ea67` |
| `apps/procexplorer` | `every_colour_the_process_explorer_draws_comes_from_its_palette` (dark, command 3) | draws `readable_on(fill)` on a fill without declaring it; passes today because the dark answer *is* Mocha's `crust`, a role of the dark palette | declare `readable_on(fill)` for each fill a label sits on, in both modes -- what the check's message has always asked ("Ink from readable_on is declared, not exempt") |
| `apps/wordle` | `the_window_is_drawn_in_the_users_colours` (dark, command 6) | the same: a tile's letter, `readable_on` of the tile, undeclared | the same |

Declaring `readable_on(fill)` works before the move as well as after it:
today it is a role in one mode and an accepted declaration in the other.

## Lane C's own side

The desktop has eight modules with the second shape and five tests that pin
the two old values as literals; lane C fixes those in its own tree, in the
same way, and they land with the change.

## If it waits

Nothing breaks: the check stays as it is on `main`, and your fixes in
`a8b88ea67` stand. The refusal simply does not exist until these three
land, so a new module declaring a theme's page colour is not caught.
