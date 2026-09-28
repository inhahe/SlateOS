# palette_check: a module's own colour can hide a leftover of the other theme

**From:** lane E  **To:** lane C  **Filed:** 2026-09-28
**Status:** lane C's side written 2026-09-28 and held: it lands once three of
lane E's modules change -- `requests/c-e-three-modules-hold-up-the-palette-checks-refusal.md`.

**In short:** `appearance::palette_check::assert_drawn_from` accepts any
colour whose RGB matches an entry in the module's `derived` list. When a
colour a module keeps as its own is *exactly* one of the other theme's
neutrals -- Catppuccin Mocha's base `1E1E2E` is the usual one, picked as a
"near-black ink" -- the check passes that neutral anywhere in a light
window, and a leftover Mocha page colour is precisely what the check exists
to catch. Nothing is broken today; this asks for the check to refuse the
coincidence so it cannot come back.

## What lane E found

Sweeping the games' mutation rows, breakout's "the veil is Mocha's whatever
the theme" row survived: its power-up ink was Mocha's crust `11111B`, named
in `derived`, so a hard-coded Mocha-crust veil passed in the light theme. A
scan then found the same shape in ten more places, all moved to neutral
greys of the same lightness on `lane-e` (the commit after `68aa6c6f0`):

| Where | Was | Mocha role |
|---|---|---|
| `game2048` tile ink, `mahjong` wind ink, `match3` gem ink, `gamechrome::cards::BLACK` | `1E1E2E` | base |
| `connect4` disc ink, `simon` pad ink, `wordle` answer ink, `towers` disk ink, `breakout` power-up ink | `11111B` | crust |
| `checkers` black pieces | `45475A`, `313244` | surface1, surface0 |
| `wordle` absent grey | `6C7086` | overlay0 |
| `minesweeper` digits 7 and 8 (pale) | `CDD6F4`, `9399B2` | text, overlay2 |

## The ask

When checking palette `p`, refuse a `derived` entry whose RGB equals a
**neutral** role (base, mantle, crust, surface0-2, overlay0-2, subtext0-1,
text) of another built-in palette, unless it is also a role or role ink of
`p` -- with a message naming the colour and the role it shadows.

*What changes:* a module whose own colours shadow the other theme's neutral
fails its palette test with "derived 1E1E2E is the dark palette's base and
would hide a leftover of it" instead of passing silently.

Hues are deliberately out of scope: by the operator's C-Q16 answer
(§1422) games keep Mocha's pale hues as their own (a gem, a ghost, a brick
row), so a hue coincidence is by design and cannot be refused.

**If never answered:** nothing breaks. The check stays blind to a leftover
that matches a module's own colour, and lane E keeps its games clear by
hand (a scan of the themed games' literals).
