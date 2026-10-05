## 1327. A colour font paints from the palette it marks for the theme's background

**Date:** 2026-09-26
**Lane:** F
**Decided by:** Claude (autonomous).

**In short:** a colour font -- emoji, mostly -- can carry several sets of
colours ("palettes") and label which suit a light background and which a
dark one. The desktop now uses the font's dark-background colours when the
theme is dark and its light-background ones when it is light. Almost every
font carries one palette and labels nothing, and those look exactly as
before; the few that label a dark palette (a black outline drawn white, say)
stay legible on a dark theme.

**Decision.** `osfont::colr::ColourPalette` names the choice CSS's
`font-palette` names -- `Normal`, `Light`, `Dark`, or an index -- resolved as
browsers resolve it: `Light` and `Dark` take the first palette `CPAL` version
1 marks usable on that background, and fall back to the first palette where
none is marked (so a version-0 `CPAL` is always palette 0). The compositor,
which draws every process's text, maps its resolved theme to `Light` or
`Dark` in `font_rendering`; a caller that asks for nothing gets `Normal`.

**Alternatives.**

| | For | Against |
|---|---|---|
| Follow the theme (chosen) | what the font's own labels are for; a dark palette is legible where the default may not be | differs from a browser's default, which paints palette 0 unless a page asks otherwise |
| Always palette 0 | what Chrome does by default | a font's dark palette is never used, on the one desktop that knows its background |
| A user setting | the user decides | a setting for something almost no font has, whose right answer the theme already gives |

**Why not the browser's default.** A web page chooses its own background
and can say `font-palette: dark` when it wants the other one; a desktop's
text sits on the theme's background, which the compositor knows, and there
is no page author to say anything. A font that labels no palette -- nearly
all of them -- is painted as a browser paints it either way.

**How to reverse.** `font_rendering`'s `palette` line; `ColourPalette::Normal`
restores palette 0 everywhere.
