## TD-C-SIXTY-EIGHT-APPS-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE

**RE-MEASURED 2026-09-13: the applications are done. What is left is the
games, and they are blocked on C-Q16.**

All twelve applications this entry named as "the real defect" now carry the
`appearance` dependency and declare **zero** private `Color` constants --
`procexplorer`, `sysinfo`, `imageviewer`, `pdfviewer`, `musicplayer`,
`speedtest`, `explorer`, `devicemanager`, `pomodoro`, `screenshot`,
`benchmark`, `mixer`. The five stragglers in crates that already had a
palette were finished the same day:

| crate | was | now |
|---|---|---|
| `sysmonitor` | `PINK = 0xF5C2E7` | `palette.pink` -- the role exists, it was a duplicate |
| `partmanager` | `COLOR_FLAMINGO`, dead | deleted; `palette.flamingo` exists |
| `launcher` | `BASE = rgba(30,30,46,240)` | `with_alpha(palette.base, 240)`. It was a hardcoded Mocha base, so on a light theme this dialog was the one dark rectangle on the screen. |
| `launcher` | `SHADOW = rgba(0,0,0,100)` | `palette.shadow()` |
| `lockscreen` | `OVERLAY`, dead | deleted -- kept "so the palette is complete" when the palette had already moved into `guitk` |

`apps/screenshot`'s four stay, and their comments already say why: a
screenshot's dimming scrim is not the theme's to tint, and an annotation the
user draws onto the picture is content that must still be red when the file
is opened on another machine.

**The remaining population is exactly the 43 games** -- 686 constants, none of
them with an `appearance` dependency -- plus six crates with **no colours at
all** (`backup`, `diffcore`, `globmatch`, `indexer`, `installer`, `safeio`),
which have no dependency because they have no interface. Those six were
inside the original count of 68 and are not a defect.

So this entry is now a duplicate of the games question. **See C-Q16**, which
asks exactly the thing the section below anticipated: a themed chessboard is
not obviously better than a chessboard, and the sweep that converts an
application would recolour the board. Nothing here is actionable until that
is answered.


**Date:** 2026-09-12. **Lane:** C.
**Where:** `apps/**` — 987 `const NAME: Color` declarations across 68 crates.

**It is an unfinished migration, not a new defect — which is the useful
framing.** §822 (2026-09-08) decided how applications receive the user's
colours and says in its own preamble that it is *"the seam 135 applications
will be converted against"*. **79 crates were converted. 55 were not, and
nothing anywhere listed which.** A converted app's `Cargo.toml` says

    # The user's colours, per design-decisions 822.
    appearance = { path = "../../gui/appearance" }

and an unconverted one simply has no such line — `explorer` and `procexplorer`
both depend on `guitk` and `oswindow` and nothing else. So the per-crate recipe
is known and already exercised 79 times: add the dependency, thread the
`Palette` the trait hands over, replace the constants with roles, adopt
`assert_drawn_from`. This entry is the missing list.

**In short:** the desktop shell was cured, in September, of a defect where every
one of its 49 modules declared its own private copy of the colour scheme —
which meant a user who chose the light theme got a light taskbar and dark
everything-else. The applications have the same defect, untouched, and at
nearly twice the size. On a light desktop, 68 of them will still be dark.

**The measurement.** `const NAME: Color = ...` in production code under
`apps/`:

| | shell (fixed) | apps (open) |
|---|---|---|
| constants | 549 | **987** |
| crates/modules | 49 | **68** |

And the same giveaway as last time — **the names collide**, which is what
proves they are copies rather than each app's own considered choices:

| name | declared in |
|---|---|
| `BASE` | 40 crates |
| `SUBTEXT0` | 40 crates |
| `BLUE`, `GREEN`, `RED` | 37 crates each |
| `YELLOW`, `LAVENDER` | 38 crates each |

`procexplorer` is the clearest single instance: `COLOR_TOOLBAR_BG`,
`COLOR_TAB_BG`, `COLOR_TAB_ACTIVE`, `COLOR_CONTENT_BG`, `COLOR_HEADER_BG`,
`COLOR_ROW_EVEN` — a complete hardcoded dark theme, 45 constants, with no
reference to the user's setting anywhere.

**Twelve crates are the real defect; the rest is a weaker question.** Splitting
the 68 by whether the crate has a `Palette` in scope at all turns out to split
it almost exactly along "is this an application or a game":

| | crates | constants | what to do |
|---|---|---|---|
| **applications with no palette** | **12** | **~246** | the defect. Thread a `Palette` and convert |
| games with no palette | ~43 | ~741 | see below — a weaker case |
| already have a palette | 13 | 57 | cheap: the roles are already reachable |

The twelve are `procexplorer` (45), `sysinfo` (26), `imageviewer` (25),
`pdfviewer` (21), `musicplayer` (18), `speedtest` (17), `explorer` (17),
`devicemanager` (17), `pomodoro` (16), `screenshot` (15), `benchmark` (15),
`mixer` (14). A **file manager** and a **process explorer** that ignore the
user's theme are the visible failure; start there.

**The games are a different question and should not be swept with them.** A
chess board's light and dark squares are the game's own art, in the same sense
that `paint`'s swatch row is the document — a themed chessboard is not
obviously better than a chessboard. What *should* follow the theme in a game is
its **chrome**: the menu, the score panel, the dialogs, the window background
behind the board. So the games want a narrower conversion, and doing it with
the same sweep as the applications would recolour the boards, which nobody
asked for. This is the part worth putting to the operator before starting.

**Not all 987 are the defect, and the distinction matters.** Three categories,
and only the first is wrong:

1. **A private copy of the theme.** `BASE`, `SUBTEXT0`, `COLOR_TOOLBAR_BG`.
   These should read from `Palette`. This is the bulk of the 68 crates.
2. **A protocol.** `apps/terminal` and `apps/tmux` carry ANSI colour tables.
   An ANSI palette is defined by the escape-sequence standard, not by the
   desktop theme, and a terminal traditionally has its own scheme. These stay,
   but the *chrome* around them (tab bar, status line) does not.
3. **User data.** `apps/paint`'s swatch row, `apps/imageviewer`'s pixel
   handling. A drawing program's colours are the document, not the interface.
   These stay.

**Why it is not visible today.** There is no guard. **47 shell modules use
`appearance::palette_check`; zero apps do** — and only one app asserts a
palette colour at all. `palette_check::assert_drawn_from` is the function that
found the shell's copies, it is already a shared crate, and it takes a
`derived` list precisely so categories 2 and 3 can be declared rather than
excused.

**The recipe, spelled out, because §822 already built the seam and 79 crates
have been through it.** For each crate:

1. `appearance = { path = "../../gui/appearance" }` in `Cargo.toml`, with the
   comment the converted ones carry: *"The user's colours, per
   design-decisions 822."*
2. A `palette: Palette` field on the application struct, initialised
   `Palette::from_settings(&AppearanceSettings::default())` — so the first
   frame has *a* palette even on a machine with no settings file.
3. `fn theme_changed(&mut self, palette: &Palette) { self.palette = *palette; }`
   — the `oswindow::app::App` method §822 added for exactly this. It is
   already called by `drive`; an app that does not override it silently keeps
   its defaults, which is what all 55 of these are doing.
4. Replace the constants: a background becomes a role, a box becomes
   `push_surface`, and *text* in a categorical hue becomes `p.ink(hue)`
   (§837).
5. Adopt `palette_check::assert_drawn_from` in the crate's render test, with
   the genuinely non-theme colours declared in `derived`. **This needs a
   *dev*-dependency of its own** — `palette_check` is behind `appearance`'s
   `testing` feature, because `cfg(test)` is set only when `appearance` itself
   is under test:

   ```toml
   [dev-dependencies]
   appearance = { path = "../../gui/appearance", features = ["testing"] }
   ```

   Cargo unifies it with the ordinary dependency when building tests.

**A correction to every count in this entry, and to how to take one.** The
figures here were produced by treating everything before the first
`#[cfg(test)]` as production code. That is wrong: a `#[cfg(test)] use
guitk::probe;` at the top of a file cuts the slice at line 42, and
`apps/pdfviewer` was therefore measured over **0.9%** of itself and reported
as having no colour literals at all. It has thirty.

Slicing at the first `#[cfg(test)]` *followed by* `mod` instead raises the
tree-wide count from 1,313 to **1,535** — 17% of the population was invisible.
Take the correction as the method rather than the number: a "production code
only" filter needs to name what it is excluding, because one that silently
excludes 99% of a file reports zero and looks like good news.

(`Color::TRANSPARENT` is counted by that pattern and should not be. It is used
as a sentinel — `if row_bg != Color::TRANSPARENT` — meaning "no background was
set", not as a colour anyone chose.)

**Two things a survey of `const` cannot tell you, found while converting:**

* **Some constants are function-local.** A pattern anchored at `^const` misses
  them entirely. 32 of them across six crates — `explorer` (17), `filediff`
  (6), `emojipicker` (3), `unitconverter` (3), `launcher` (2), `lockscreen`
  (1). `explorer` has *no* module-level colour constants at all, so an anchored
  survey reports it as clean.
* **Some colour literals are not interface at all.** `explorer`'s `thumbs.rs`
  has nine file-type colours — green for images, amber for folders, red for
  PDF — which look exactly like theme candidates. They feed
  `canvas.fill_rect` into a **pixel buffer** that becomes a thumbnail image,
  not a `RenderCommand`. They are generating *content*, in the same sense as
  `apps/paint`'s swatch row, and converting them would theme the pictures
  rather than the window. The tell is the destination, not the name: follow
  the colour to whether it reaches the renderer or a raster.

  (`explorer` is also the mirror of `procexplorer`: it hardcodes a **light**
  theme — `rgb(224, 224, 224)`, white chooser backgrounds — so on a dark
  desktop it is a white file manager. Both directions of the same defect exist
  in the tree.)

**Step 5 is not the paperwork; it is where the rest of the work is found.**
`procexplorer` had 22 `const COLOR_*` values — and *also* 26 inline
`Color::rgb(..)` literals that were never constants, plus two labels drawn in
`Color::WHITE` on themed fills. A survey grepping for `const COLOR_` cannot see
either class, so **the 987 figure above is an undercount**, and the only thing
that reveals the remainder is adopting the guard and watching it fail. It
failed twice within seconds of existing, on defects nobody would have found by
reading.

`procexplorer` shows how mechanical step 4 usually is — its thirteen named
constants map almost one-to-one:

| its constant | the role |
|---|---|
| `COLOR_CONTENT_BG` | `base` |
| `COLOR_TOOLBAR_BG`, `COLOR_STATUS_BG` | `Surface::Strip` |
| `COLOR_ROW_ODD`, `COLOR_ROW_HOVER` | `surface0` |
| `COLOR_ROW_SELECTED` | `Surface::Selected` |
| `COLOR_TAB_ACTIVE`, `COLOR_ACCENT` | `accent` |
| `COLOR_TEXT`, `COLOR_TEXT_DIM` | `text`, `subtext0` |
| `COLOR_DANGER` | `red` |

**What the proper fix looks like.** Crate by crate, in the order the table
above suggests: thread the `Palette` the app already receives (or add it),
replace the category-1 constants with roles, adopt `assert_drawn_from` in the
crate's existing render test, and declare categories 2 and 3 in `derived`. The
shell's own conversion is the worked example, including the part that is not
mechanical: `BASE` was opaque in 26 modules and translucent in 2, and those two
are panels rather than pages.

**How urgent.** Not a crash, and it does not worsen on its own — but it is
straightforwardly user-visible the moment anyone selects the light theme, and
it is the single largest remaining piece of the appearance work. It is also the
reason the app half of the border conversion looked cheap: those crates were
converted where they *did* use the palette, and the constants were never in
scope.


---

**Update 2026-09-13 — every application outside the games is done, and the
games are genuinely blocked.**

The twelve applications this entry was written for are converted, and so is
everything else that is not a game: calendar, emojipicker, unitconverter,
diagram, launcher, stopwatch, mandelbrot, alarmclock, filediff. The widget
layer under them went too --
`TD-C-THE-TOOLKIT-S-WIDGETS-STILL-PAINT-THEMSELVES-DARK`.

**Counting it properly took four tries, and that is the lesson.** A colour
constant hid from a sweep in four different spellings:

| spelling | example | what missed it |
|---|---|---|
| six-digit hex | `from_hex(0x1E1E2E)` | nothing — this is what every survey looked for |
| decimal | `Color::rgb(49, 50, 68)` | `disabled.rs`, three constants, outlasted 119 others |
| padded hex | `from_hex(0x001E_1E2E)` | `stopwatch`, fourteen — the crate read as having none |
| an alias | `PLAYER1_COLOR = BLUE` | twelve, and no *value*-based survey can ever see them |

The last one is the interesting case: its value is a name, so a sweep asking
"which constants hold a Mocha value" is asking the wrong question of it.

**What is left is the games, and they wait on C-Q16.** The measured split, so
whoever picks this up does not have to re-measure:

- **324 constants are Mocha neutrals** (base, mantle, surface0-2, text,
  subtext0, overlay0) across 43 crates;
- **335 are hues**, named `BLUE`, `YELLOW`, `LAVENDER` and so on -- the ramp
  copied wholesale, not semantic names like an I-piece or a mine count;
- **43 constants across 9 crates** are named for the play surface itself
  (`BOARD_LIGHT`, `FELT`, `CARD_BG`, `SQUARE_DARK`).

C-Q16 promises the games' *chrome* is fixed either way, and I looked for a
slice that delivers that without pre-empting the answer. There is not one.
The games do not separate chrome from board at the constant level: they copy
the ramp and then use `SURFACE2` for a light square and `TEXT` for a label.
Converting the ramp themes the board too, which is exactly what C-Q16 has not
decided -- and hand-slicing it across 43 crates would be making the operator's
decision forty-three times in private.
