## 527. The logo is the product's colour and the tab strip is the user's, so two sites that drew the same blue now mean different things

**Date:** 2026-08-23
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The About dialog draws blue in two places: the square logo tile
next to the product name, and the label of whichever tab you have selected.
Until now both read the same hard-coded blue, so they were indistinguishable —
and when the shell gained a user-chosen accent colour, the obvious move was to
point both at it. That would have meant a user who picks pink gets a pink
SlateOS logo. The decision is to split them: the **logo stays `blue`** whatever
accent is chosen, and only the **tab strip follows the accent**. Two sites that
used to be one constant now answer to two different owners.

The rule underneath it is that a palette role and a *brand* are not the same
kind of thing even when they hold the same bytes. `p.accent` means "the colour
this user chose to mark the thing they are pointing at"; `p.blue` means "the
colour SlateOS is". The accent is a preference and is expected to move. The
logo is an identity and is not — an operating system whose logo changes colour
with a settings toggle has no logo, it has a swatch. Both are still read from
the palette, so both still flip correctly between Mocha and Latte; what differs
is *which* palette entry each site is entitled to.

The counter-argument, and it is not weak: users who set an accent generally
expect the shell to look like it, and a stubbornly blue tile in an otherwise
pink dialog reads as a site that was missed by the theming rather than one that
was excluded on purpose. Some shells do exactly that — recolour the mark along
with everything else. It was rejected because the About dialog is precisely the
place where the product identifies itself, and a version screen that reports
the wrong colours for the thing it is reporting on is a small lie in the one
dialog whose entire job is to be factual.

There is a testing consequence, which is why this is worth writing down rather
than leaving as a two-line comment. Because the sites drew the same value, a
single `blue` assertion used to cover both, and any test that stayed at that
level would keep passing if the two were re-merged in either direction. So the
tests pin them **separately and by position**: the logo is checked at its own
index against `p.blue` in a fixture whose accent is deliberately *off-palette*
magenta, and the tab strip is checked at the active tab's index against
`p.accent`, with the accent's total occurrence count asserted to be exactly one.
Either half of the split failing — the logo drifting to the accent, or the tab
falling back to blue — moves a colour the other assertion is watching. A fixture
that left the accent at its stock value could not have made this claim at all,
because the stock accent *is* `blue`.
