### TD-C-THE-TOOLKIT-HOLDS-A-THIRD-COPY-OF-THE-PALETTE-AND-DISAGREES-WITH-ITSELF-ABOUT-IT — 2026-08-22 — **FIXED 2026-09-03**

**In short.** Widgets (buttons, text fields, scrollbars — the controls apps are
built from) get their colours from `gui/toolkit`, which keeps its own
hand-written table of the same Catppuccin values the shell was keeping 549
copies of. That is a third copy, after the shell's and `gui/appearance`'s. It
is not currently *wrong* on screen, but it is already drifting in the one way
that matters: the comments in it name roles that do not match the values beside
them, so the next person to edit it by role will pick the wrong hex.

**Where:** `gui/toolkit/src/theme.rs` — `Theme::catppuccin_mocha` (~:190) and
`Theme::catppuccin_latte` (:232). The Latte table is the clearer case:

| Line | Value | Comment says | Latte actually calls it |
|---|---|---|---|
| :238 | `0xE6E9EF` | `Surface0` | `mantle` |
| :239 | `0xDCE0E8` | `Surface1 (Crust)` | `crust` |
| :258 | `0xBCC0CC` | `Surface2` | `surface1` |
| :260 | `0xCCD0DA` | `Surface1` | `surface0` |
| :262 | `0xE6E9EF` | `Surface0 (Mantle)` | `mantle` |

Two comments (`:239`, `:262`) name *two* roles for one value, which is a
comment author noticing the disagreement and recording both rather than
resolving it. The ladder is shifted by one rung throughout, so "Surface1"
means three different colours in this one function.

**The concrete defect inside it:** `:243` `text_secondary: 0x6C6F85`. That is
upstream Latte `subtext0`, and it measures **4.37:1** on `background`
(`0xEFF1F5`) — under the 4.5:1 WCAG floor for body text, which is what
secondary label text is. `gui/appearance` rejected exactly this value when the
light palette was built and uses `0x686B80` (4.64:1) instead; see
design-decisions §525. So the toolkit currently ships a light theme whose
secondary text is measurably illegible, and the crate next door already knows
the right answer.

**Reproduce:** compute the WCAG contrast of `0x6C6F85` against `0xEFF1F5`
(relative luminance, `(L1+0.05)/(L2+0.05)`) — 4.37. Or read `:238`–`:239`
against Catppuccin Latte's published palette and note the roles named in the
comments are not the roles the values are.

**Why it shipped:** the toolkit predates `appearance::Palette` and could not
have depended on it — and it still has a genuine reason to keep a type of its
own, because `Theme` carries widget-level roles (`primary_hover`,
`scrollbar_track`, `border_focus`) that are not palette roles and should not
become them. What it does not need is its own *values*.

**Proper fix:** keep `Theme` as the widget-role type, and derive both
constructors from `appearance::Palette` — `catppuccin_latte()` becomes
`Theme::from_palette(&Palette::for_mode(true))`, mapping each widget role onto
a palette role once, the same shape `DesktopTheme::from_palette` now has. That
deletes the hand-written hex, makes the ladder-off-by-one impossible to write,
and makes the legibility fix propagate rather than needing to be applied twice.
The dependency direction is right: `appearance` is below `toolkit` and already
in its graph.

**Trigger:** best done as part 2 of
`TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE` or
immediately after it, while the mapping is fresh. Not urgent on its own, with
one exception: the `text_secondary` contrast is a shipped accessibility defect
and can be fixed on its own in one line without waiting for the refactor.

**If never fixed:** a user on the light theme reads secondary labels at 4.37:1,
and the next edit to the toolkit's ladder lands one rung off because the
comments say it should.

### Fixed 2026-09-03 — by deleting the copy, not by de-duplicating it

**Both halves of the "proper fix" above were written on false premises, and
checking them is what changed the answer.** They are left in place above rather
than corrected, because the mistakes are the interesting part of this entry.

**False premise 1: the dependency direction.** The entry ends "`appearance` is
below `toolkit` and already in its graph." It is the other way round —
`gui/appearance/Cargo.toml` depends on `guitk` (for `Color`, and it re-exports
`guitk::theme::contrast_ratio`), so `Theme::from_palette(&appearance::Palette)`
is a dependency cycle and cargo would refuse to build it. The proposed fix was
not merely awkward; it does not compile.

**False premise 2: that anything used the type.** `Theme`, `ThemeColors`,
`ThemeManager` and `ThemeMode` had **zero constructors anywhere in the tree**
outside their own tests. Every consumer of `guitk::theme` — three files —
imports a colour *function* (`with_alpha`, `contrast_ratio`,
`relative_luminance`); not one imports the type. The whole apparatus was held
alive by a `#![allow(dead_code)]` at the top of the file, which is also why
nothing had ever pointed at it. The `theme::BLUE`-style references that a grep
turns up in `apps/` are those apps' own local `theme` modules, not this one.

So the fix is deletion. De-duplicating unreachable code leaves unreachable
code, and rewriting it against `appearance::Palette` would have produced a
*second* live-looking theming API next to the real one — the trap being that
the next author to reach for "the toolkit's theme" gets the one the light/dark
switch cannot reach. See design-decisions.md §810 for the alternatives weighed.

**What went**, all from `gui/toolkit/src/theme.rs`, 1152 lines → 476:

| Removed | Why it was safe |
|---|---|
| `Theme` (32 colour fields, 4 constructors) | never constructed outside its own tests |
| `ThemeManager` (+ change callbacks) | no callback was ever registered |
| `ThemeColors` (24 accessors) | no widget ever held one |
| `ThemeMode` | a duplicate of `appearance::ThemeMode` as well |
| 18 tests exercising the above | they tested only each other |
| `#![allow(dead_code)]` | nothing is dead any more |

**And one more duplicate found on the way out: `theme::mix`.** It was
`Color::lerp` written a second time — same formula, same clamp — except that
`lerp` reads a NaN factor as "no progress" and `mix` passed it through, because
`f32::clamp` does. `NaN as u8` is 0 in every channel, so `mix(a, b, NAN)`
returned *transparent black* rather than either end colour. An animation gets a
NaN factor the moment it divides elapsed time by a zero duration, which is the
case `Color::lerp`'s doc comment already calls out. `mix` had one non-test
caller, `ThemeColors::disabled_state`, which went with the rest; `lerp`'s NaN
behaviour is pinned by `color.rs`'s own test at :245. A comment stands where
`mix` was, so the next author reaches for the method rather than writing the
free function again.

**What is left in the module** is what the tree actually uses:
`relative_luminance`, `contrast_ratio`, `contrast_text`, `is_dark`, `lighten`,
`darken`, `with_alpha` — the colour arithmetic every legibility question on
this desktop is asked in, and the functions `appearance` re-exports rather than
copies. The module doc now says so, and says what used to be there.

**The 4.37:1 defect is gone with it, and the surviving value is guarded.**
`0x6C6F85` no longer appears anywhere in the tree. The role it filled is
`appearance::LIGHT_SUBTEXT0` = `0x686B80` (4.64:1), and
`gui/appearance/src/lib.rs`'s sweep at :2727 already asserts every text role
clears 4.5:1 on its own base in *both* modes — so the value that was wrong here
cannot be reintroduced there without a red test.

**A test now guards the deletion itself:**
`this_module_names_no_colours_of_its_own` reads the module's own source via
`include_str!` and fails if `from_hex(` reappears in it. "Gone" is otherwise
not a property anything checks, and the impulse that put the palette here —
*the toolkit ought to know what a button looks like* — will occur to the next
author as readily as it did to the last. Mutation-checked: reinstating a single
`pub const BLUE: Color = Color::from_hex(0x89B4FA);` turns it red, and nothing
else.

**Still open, and unaffected:** `TD-C-EVERY-APPLICATION-CARRIES-ITS-OWN-COPY-OF-THE-PALETTE-TOO`.
That entry's account of the `SKY` transposition names `gui/toolkit/src/theme.rs`
among the files spelling `0x89DCEB`; as of today it spells nothing.
