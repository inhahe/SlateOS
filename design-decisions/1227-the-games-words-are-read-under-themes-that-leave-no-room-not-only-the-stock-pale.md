## 1227. The games' words are read under themes that leave no room, not only the stock palettes

**Date:** 2026-09-28
**Lane:** E
**Decided by:** Claude (autonomous)

**In short:** each game's test that every word is readable used to check the
four stock looks (light and dark, bordered and card). Those palettes' colours
read on every panel a game draws for itself with room to spare, so a word the
game forgot to adjust for its panel still passed. The test now also checks
four made-up themes of the kind a user can build -- one whose text is a
softer grey, one whose accents are pale -- which the palette makes readable on
the page and no more. Under them a forgotten word shows up.

| Call | Chosen | The other way, and why not |
|---|---|---|
| What the words are read under | the stock looks, and four named themes at the palette's floor, each in both surface looks (`gamechrome::legibility::looks()`) | the stock looks only: every "moved for its ground" line in a game is untestable under them, and reversi's score bar was a mutation nothing could catch |
| Which themes | soft text, light and dark (Solarized's greys); pale hues on light; deep hues on dark | random palettes: a failure that comes and goes with the seed is a failure nobody can reproduce. A synthetic worst case (every colour exactly at the floor): not a theme anyone can make, and a test that fails on no real configuration argues against itself |
| Where the list lives | once, in `gamechrome`, read by all 42 games' tests | a copy per game, as tic-tac-toe's pale palette was: 42 lists drift |
| What it checks | a theme leaves a raised ground no room (`a_theme_without_room_leaves_a_raised_ground_none`), so the list cannot quietly go soft | trusting the choice of colours: a palette change could give them back their room and every game's test would pass for nothing |
