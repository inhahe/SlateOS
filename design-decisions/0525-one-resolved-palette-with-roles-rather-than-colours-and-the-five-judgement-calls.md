## 525. One resolved `Palette`, with roles rather than colours — and the five judgement calls inside it

**Date:** 2026-08-22
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The desktop's settings page lets a user pick light or dark mode,
an accent colour, and how see-through panels should be. Almost nothing obeyed
it, because 49 of the shell's modules each carried their own private list of
colour constants — 549 of them — that happened to match the dark theme. The fix
needs one place that answers "what colour is a window background right now",
and this records the five decisions inside building that place, each of which
had a defensible answer on both sides. Part 1 (the type, and the two consumers
rewritten onto it) is done; part 2 (the other 49 modules) is not.

Terms used below, once each: a *role* is what a colour is *for* — "the window's
background", "text on it" — as opposed to a hex value like `0x1E1E2E`. *Latte*
and *Mocha* are the light and dark variants of Catppuccin, the colour scheme
the shell already used. *Alpha* is how opaque a colour is, 0 fully transparent
to 255 fully solid. *Contrast ratio* is the WCAG measure of how legible one
colour is on another; 4.5:1 is the published floor for body text.

### Why a resolved struct, and not a lookup function

The alternative was `fn color(role: Role, mode: ThemeMode, accent: AccentColor)
-> Color` — no new type, nothing to keep in sync. It was rejected because every
call site would then have to carry the mode and the accent in order to ask,
which is three arguments threaded through the render functions instead of one,
and because a renderer that holds a `&Palette` cannot ask for a colour in the
wrong mode by accident. The struct is 21 fields resolved once at the top of a
frame; the function is a question asked several hundred times a frame, each
time with a fresh opportunity to pass a stale mode.

The cost is that adding a role means adding a field, and a field can be added
and then not used. That is the failure the tests are shaped against: `roles()`
in the test module is a hand-written array of all 19 colour fields, deliberately
*not* derived by macro, because the point of several tests is that a field added
later is not silently skipped — and a macro would skip it for exactly the reason
a renderer would.

### Decision 1: the named hues stay separate from the accent

The shell used `BLUE` for two jobs that look identical and are not: "the colour
this desktop is themed around" and "the blue one" of a fixed categorical set —
CPU drawn next to memory in green and temperature in peach on a resource graph.
Collapsing both onto `accent` is tempting, one field instead of nine, and it is
wrong: a user who themes their desktop Red gets a graph whose CPU line and
temperature line are the same colour, and no amount of care at the call site
recovers the distinction once the values are equal.

So `Palette` carries `blue`, `green`, `red`, `yellow`, `peach`, `lavender`,
`mauve`, `sapphire` *and* `accent`. `accent` moves with the user's choice; the
eight hues never do. The test
`the_accent_setting_moves_the_accent_and_leaves_the_categorical_hues_alone`
states exactly this, and the reintroduction harness confirms it catches the
collapse (defect F).

**Against:** nine colour fields where one theme-aware one would do is more
surface, and a call site can still reach for `blue` when it meant `accent`.
That is true, but it is a mistake with a visible symptom in one place, whereas
the collapse is invisible until someone changes their accent.

### Decision 2: a scrim is black, not the palette's own base

A *scrim* is the dimming laid over the desktop behind a modal dialog. The shell
dimmed with its own `BASE` at alpha, which is fine in Mocha because Mocha's base
is nearly black. Latte's base is `#EFF1F5` — so carried over literally, "dim the
desktop" in light mode would have *brightened* it.

`scrim()` therefore returns `Color::rgba(0, 0, 0, 140)` in both modes and does
not read the palette at all. The reasoning is that a scrim is not a colour the
theme chooses; it is an absence of light, and absence of light is black wherever
you are. The same holds for `shadow()` and `text_shadow()`.

**Against:** a method on `Palette` that ignores `self` looks like a mistake, and
someone will eventually "fix" it. Mitigated by the doc comment saying why, and
by `the_scrim_and_the_shadows_darken_whichever_palette_they_fall_on`, which
fails if any of the three starts varying by mode (defect J).

### Decision 3: one shadow value, but two shadow *roles*

The shell declared `SHADOW` at three different alphas — 100, 120 and 160 — plus
a separate `LABEL_SHADOW` at 180. The three `SHADOW`s are one role whose value
drifted, and they collapse to `shadow()` at 120.

`LABEL_SHADOW` does **not** collapse into it, and this is the call worth
recording, because 180-versus-120 looks exactly like more drift. It is not: a
panel's shadow exists to suggest the panel floats above a surface the shell
chose, so it can be subtle. A label's shadow exists to keep white text readable
over a wallpaper the *user* chose, which may be anything, including white. One
is depth, the other is legibility, and only one of them has a worst case. So
`text_shadow()` is 180 and stays its own method. Defect K — quietly making them
equal — is caught.

### Decision 4: `LIGHT_SUBTEXT0` departs from upstream Catppuccin

Measured on the Latte base, upstream's Latte `subtext0` (`#6C6F85`) gives
**4.37:1** — below the 4.5:1 floor — while Mocha's counterpart gives 7.37:1. The
role is body text: secondary labels, the unfocused window title. Shipping it
means the light theme is measurably less legible than the dark one at the exact
place a user reads most.

`LIGHT_SUBTEXT0` is therefore `#686B80`: each channel scaled to 96.5%, which
holds the hue and reaches 4.64:1. This follows a precedent already in the crate
— the `LIGHT_*` accent constants are likewise darkened from upstream, for the
same reason.

**Against:** diverging from a published palette means the desktop is no longer
"Catppuccin Latte" in the strict sense, and someone comparing against upstream
will find a value that does not match and may take it for a bug. What it buys is
that the legibility invariant in the tests
(`every_role_a_user_reads_is_legible_on_the_base_of_its_own_palette`) can be a
flat rule over both modes instead of carrying an exception — and an exception in
a legibility floor is how the floor stops being one. Defect A restores upstream's
value and the test rejects it, which is that property made concrete.

### Decision 5: `panel_alpha` is carried, not applied

Transparency arrives as a field on the palette and is applied by exactly two
methods, `panel_bg()` and `panel_hover()`. The rejected alternative was to apply
it during construction, so that `base` itself is translucent and everything
drawn from it inherits the setting for free.

That is wrong because transparency belongs to the surface that *floats*, not to
the palette. A list row inside a translucent menu must stay opaque: if it is
not, the wallpaper shows through the row rather than through the menu containing
it, and the row stops looking like part of the menu. Applying alpha at
construction makes that impossible to express, because there is no longer an
opaque base to reach for. Defect I does precisely this, and
`transparency_reaches_panels_and_nothing_behind_them` catches it.

### What the reintroduction proof changed about the design

`scripts/reintro-palette.py` reintroduces 19 defects one at a time and checks
each is caught by the test that names it. Defect Q — a window's close button
following the accent instead of staying red — came back **uncaught**, and the
reason was not the test. `DecorationColors::from_settings` built its frame from
`Palette::for_mode` and then painted the accent on top afterwards, so the
palette a frame was assembled from always carried the *default* accent:
`p.accent` was blue whatever the user had chosen, and the defect produced blue
on both sides of the comparison.

The fix was structural rather than a patched line. `from_palette(&Palette)` is
now the single body that says which role each part of a frame is, and the two
entry points each pass the palette they actually mean — `for_mode` passes the
mode's, `from_settings` passes the user's. `DesktopTheme` was restructured the
same way for the same reason. This is the design's answer to "how do we know the
right palette was used": there is exactly one place where the wrong one could be
passed, and it is three lines long.

Worth stating because it recurs: the harness's value here was not confirming
that the tests work. It was that a test which *should* have failed and did not
pointed at a defect in the code beneath it, rather than at itself.

### Addendum, same day: two fields more, one constant wrong, and a sweep that was half a sweep

Surveying `apps/` to size the next stage turned up three things that belong in
this decision rather than in the one that will consume them.

**`teal` and `sky` were added to `Palette` immediately, not when `apps/` is
converted.** The shell declares neither; the applications declare `TEAL` 86
times and `SKY` 18. The argument for waiting is that a field with no caller is
dead weight and this crate's own tests exist to catch exactly that. The
argument for adding them now, which won, is that every mode-flip and legibility
sweep in the test module iterates a hand-written list of the palette's fields —
so a hue added later has to be threaded back through all of them, and a sweep
that silently skips a field is the precise failure those sweeps were written to
detect. Paying that cost once, while the light ladder is being written and
every value is being checked anyway, is cheaper and safer than paying it under
a large mechanical conversion where a missed row would look like noise.

**`SKY` was wrong, in the crate that owns the answer.** Catppuccin Mocha's Sky
is `#89DCEB`; `gui/appearance/src/lib.rs` said `0x89DCFE` — a transposed byte
pair, present since the constant was written — so `AccentColor::Sky.color()`
returned a colour the scheme does not contain, and the wrong value had already
been copied into `apps/alarmclock` and `apps/emojipicker`. Six other
applications and `gui/toolkit` had the right one.

The fix is one character. The interesting part is that **nothing in the tree
could have found it**, and the reason generalises past this bug: every test in
this crate compares one part of the tree against another, which by construction
cannot see an error the whole tree shares — and 2,258 duplicated declarations
agreeing with each other is not evidence, since agreement is what copying
produces whether or not the original was right. So
`every_dark_constant_is_the_published_catppuccin_mocha_value` transcribes the
published palette *independently* and pins all 24 declared values against it.
That is the only test in the file whose expected values do not come from the
tree, and it is the only one that could have caught this.

Two things it deliberately does not do. It does not pin the `LIGHT_*` accents,
because those depart from published Latte on purpose — they are darkened to
carry text, per Decision 4 above — so pinning them would assert the negation of
a decision recorded twenty lines earlier. And it does not list `overlay1` and
`overlay2`: they are published but not declared here, and an entry pairing
`Color::from_hex(0x7F849C)` with `0x7F849C` asserts a value against itself,
which is the vacuous shape the reintroduction harness for this work exists to
rule out. A constant the crate does not have is not a constant a test can check.

**The categorical-hue sweep was only testing half of what it claimed**, and the
harness is what said so. `the_accent_setting_moves_the_accent_and_leaves_the_categorical_hues_alone`
compared against `Palette::for_mode(false)` using the default settings — which
are dark — so the entire light arm was unexercised. A reintroduced defect that
collapsed a hue onto the accent *only in light mode* was reported as
`NO TEST FAILED`. It now sweeps both modes and all ten named hues.

Worth recording as a rule rather than an anecdote: a mode-dependent type wants
its tests written as a loop over the modes from the start, because the arm a
reader checks less is also the arm a test forgets, and both defaults point the
same way — the dark arm is older, is what the tree already agreed with, and is
what `Default` returns. Every unswept test in a two-mode type will be unswept
on the same side.
