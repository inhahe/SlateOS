### [E] Text on a panel an application fills itself may read under 4.5:1 in the light bordered look -- 2026-09-28

**Status:** Open -- fixed in the games as the legibility pass reaches each;
the applications beyond them are unchecked.

**In short:** the palette makes its text colours readable on the grounds the
theme itself draws text on. Under the default bordered look a card has no
fill, so the raised surface (`surface0`) is not one of those grounds -- but an
application that fills a panel with `surface0` itself, rather than through
`push_surface`, writes on it anyway. In the light theme the palette's
secondary grey reads at 4.1:1 there, and a hue at about 3.6:1: under WCAG's
4.5:1 for ordinary text.

**Where:** any `fill(..., p.surface0 / surface1, ...)` with text on it under
`SurfaceStyle::Borders`. The games' legibility tests found it in game after
game (score boxes, side panels, help sheets, game-over cards); the card look
passes, because there `surface0` is a text ground and the palette's floor
covers it.

**How it was found:** `gamechrome::legibility`, which reads every run of text
against the fills drawn under it, over each game's states in both themes and
both surface looks.

**The proper fix:** in a game, a sheet or a banner becomes the toolkit's
panel (`Surface::Panel`), and words that stay on a raised ground are moved
only as far as they must be (`gamechrome::Ink::on`, or `Chrome::on(ground)`
for a whole panel's roles). On the toolkit's panel the palette's roles read
as they are: `Palette::ink` holds every text colour to 4.5:1 on each ground
the toolkit paints text on, and a panel's fill is one of them in either look
(the page itself under borders, `mantle` under cards). Moving a role for a
panel moves nothing -- every "unmoved for its panel" mutation survived the
sweeps in tictactoe, 2048, Connect Four and Simon -- so a panel's words are
written in the roles as they are, and only a ground the game fills itself is
a reason to move them.

Since 2026-09-28 the games' tests read that under more than the stock
palettes (`gamechrome::legibility::looks()`): four themes a user can put
together that sit at the palette's 4.5:1 floor on the page -- soft text,
light and dark, pale hues on light, deep hues on dark -- where the stock
palettes' text and hues have room to spare on a game's own grounds. They
found five more games' words, now moved. For the other applications: the same reader over
each one's states -- the reader is in `apps/gamechrome` today, and an
application that is not a game would need it moved or re-exported where it
can reach it -- then the same two remedies.
