## 1470. A module's own colour may not be another theme's page colour, and a label's ink is no page's

**Date:** 2026-10-05 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** the palette check (`appearance::palette_check`) is the test
that catches a program still drawing in the dark theme's colours after the
user chose the light one. A program lists the few colours that are its own
-- a game's piece colours -- and the check lets those through. Lane E found
that eleven games listed a colour that was *exactly* one of the dark theme's
page colours, which let a leftover of that page through too, unnoticed. The
check now refuses such a listing and says which page colour it is. And the
two colours the toolkit picks for a label on a coloured button (`readable_on`,
near-black or near-white) were exactly the dark theme's darkest page colour
and the light theme's page, which forced the same blind spot on every
program that labels a button. They are now one step further out -- the same
to the eye, a different value to the check.

**Where:** `gui/appearance/src/palette_check.rs` (`NEUTRAL_ROLES`,
`shadowed_neutral`, `assert_derived_shadows_no_neutral`, and its module
docs), `gui/toolkit/src/palette.rs` (`DARK_EXTREME`, `LIGHT_EXTREME`); asked
for in `requests/e-c-palette-check-a-derived-colour-can-hide-a-leftover.md`.

### The choices, and what they cost

| Choice | Instead of | For | Against |
|---|---|---|---|
| **Refuse a declared colour equal to a built-in palette's neutral** (`crust` .. `text`), unless the check would take it anyway -- a role of the palette checked, its ink on one, black | warning; or refusing every declared colour that matches any role | A declaration is accepted anywhere in the window, so a declared page colour is a hole exactly where the check exists to look. Neutrals only: by the operator's C-Q16 answer (§1422) a game keeps Mocha's pale hues as colours of its own, so a hue coincidence is by design. | A module that chose a page colour as its own fails its test until it moves one step off -- which lane E did for its eleven games before this landed. |
| **`readable_on`'s answers one step further out:** `#10101A` (was Mocha's `crust`, `#11111B`) and `#F0F2F6` (was Latte's `base`, `#EFF1F5`) | keeping them equal to the roles and exempting them | Equal, every module that labels a fill had to declare a page colour, and the check could neither refuse a leftover crust in a light window nor tell it from a label. One step off is the same colour to the eye; paler and darker respectively, so no contrast either answers for drops. | Every label on a coloured fill changes by one level per channel; tests that pinned the old values change with them. |

**Held, then landed.** Written on 2026-09-28 and held on
`lane-c-held-palette-refusal` until three of lane E's modules stopped
declaring page colours (freecell, procexplorer, wordle,
`requests/c-e-three-modules-hold-up-the-palette-checks-refusal.md`); lane E's
fix reached `main` before this did. A test that exempted Mocha's crust
because it was `readable_on`'s answer (the login screen's) checks it again.
