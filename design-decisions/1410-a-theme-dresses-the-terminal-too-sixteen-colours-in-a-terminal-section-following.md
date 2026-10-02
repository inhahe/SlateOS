## 1410. A theme dresses the terminal too: sixteen colours in a `terminal` section, following the theme's hues

**Date:** 2026-09-27 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** A theme set the desktop's colours and stopped at the terminal,
which drew in fixed colours of its own. Now the palette every program is handed
carries a terminal's colours as well (`Palette::terminal`): its background,
foreground and cursor, and the sixteen colours programs name by number. By
default they are the theme's own hues in the slots every terminal gives them --
red for red -- so a theme that retints its hues retints its terminal; a
`terminal` section in the theme file can set any of them outright. The terminal
application moves onto them next (`requests/c-e-the-terminal-draws-in-the-themes-terminal-colours.md`).

| Question | Chosen | The alternative | Why |
|---|---|---|---|
| Whether a theme sets the sixteen at all | yes, as every terminal's themes do | no: "colour 1 is red because the escape sequence says so; retinting would corrupt what programs print" (the terminal application's comment) | a theme chooses the *shade* of red, not whether slot 1 is red; the defaults put each hue in its own slot, and a theme author who puts green in red's slot has made a theme nobody will use |
| The defaults | the palette's hues in their slots, as Catppuccin's terminal themes place them: pink as magenta, teal as cyan | fixed xterm colours | a theme that changes `red` changes the terminal's red with it, which is the point of one theme for the whole desktop |
| The four greys in light mode | the text as black, the raised surfaces as white and bright white, and the faintest mark made legible as bright black | Latte's mapping, the subtexts | this palette's light subtexts are a deepened blue ink (`#00688b`), not greys; mapped as Latte maps them, "black" would have been blue |
| Contrast | the foreground held to the text floor on the background; the sixteen not | all nineteen held | a program may mean black on black; the foreground is what everything unmarked is written in |
| One-mode themes | the terminal goes with the colours' mode (`ThemeColors::variant`) | read the asked mode's section regardless | a theme's terminal was chosen against its own grounds |
| High contrast | the scheme's ink on its page, the hues kept | the ordinary palette's terminal | the scheme exists for exactly the text a terminal writes |

The shipped theme file writes both sections out in full, as a template, and a
test holds them equal to what the palette derives.
