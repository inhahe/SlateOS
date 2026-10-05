## TD-C-THE-ACCESSIBILITY-CONFIG-IS-A-DEAD-PARALLEL-COPY -- DONE 2026-09-13

**Date:** 2026-09-09. **Lane:** C.
**Where:** `gui/desktop/src/a11y.rs` — 1,360 lines, referenced by nothing.

**In short:** there are two sets of accessibility settings. One is wired up and
works. The other is a 1,360-line module that looks like the real one — it has a
config-file format, range checking and passing tests — and nothing anywhere
reads it. Three settings exist *only* in the dead copy, so those three do
nothing at all: a wider text cursor, a visible focus ring, and the screen-reader
switch.

**The evidence.** `grep -rn "a11y::" gui apps`, excluding the file itself,
returns **one** hit, and it is inside a doc comment in
`gui/inputsettings/src/lib.rs` describing this module as superseded. Nothing
constructs `AccessibilityConfig`, nothing loads it, nothing saves it. The
module is `pub mod a11y;` in `gui/desktop/src/lib.rs`, so it compiles and is
public API, and no caller exists.

**Correcting this entry's own first version, because the mistake is instructive.**
It originally carried a table of "uses outside `a11y.rs`" — `high_contrast` 117,
`color_filter` 26, `reduced_motion` 17 — and concluded that those settings were
wired while three others were forgotten. That was wrong. Those are counts of a
*name*, and the names collide with fields on entirely different structs:
`high_contrast` and `color_filter` are declared on
`appearance::AppearanceSettings`, and `reduced_motion` is declared twice, once
here and once on `animations.rs`'s own config. The live features read the other
structs. Counting a name cannot distinguish two structs that share a field, and
the giveaway was in the same survey: `cursor` scored 3,595, which is just the
English word.

**What each dead field duplicates, and what is genuinely missing:**

| `AccessibilityConfig` field | live equivalent |
|---|---|
| `high_contrast` | `appearance::AppearanceSettings::high_contrast` |
| `color_filter` | `appearance::AppearanceSettings::color_filter` |
| `cursor` | `AppearanceSettings::cursor_size` / `cursor_scheme` |
| `reduced_motion` | `AppearanceSettings::animation_speed`, `animations.rs` |
| `magnifier` | none — `MagnifierConfig` is declared only here |
| `visual_alerts` | a separate field of the same name in `apps/settings` |
| `text_scale` | partial: `FontSettings::ui_size` is absolute, not a multiplier |
| `caret_width` | `AppearanceSettings::caret_width_scale`, added 2026-09-13 by 839 and *read* since the `appearance_changed` hook landed -- see below |
| `focus_indicator` | `AppearanceSettings::focus_ring_scale`, built 2026-09-13 and *read* by `guitk::modal` -- and the dead copy here is deleted |
| `screen_reader` | **none** — the feature does not exist |

**Update 2026-09-13 (later): two rows moved, and one of them was made stale by
this lane an hour earlier.**

`focus_indicator` no longer has "none" as its live equivalent. It is
`AppearanceSettings::focus_ring_scale`, read through `guitk::style::FOCUS_RING_WIDTH`
by the one widget that draws a ring, and `desktop::a11y`'s `FocusIndicator` --
146 lines including its own tests -- is deleted. `caret_width` went the same way
under 839. Both were built because *this table* recorded them as wanted, which
is the case for keeping a dead field rather than deleting it silently; both are
now removed from here because the record is no longer the only copy.

The module is 1 239 lines, down from 1 360, and the only reference to `a11y::`
anywhere outside it is still the doc comment in `gui/inputsettings` calling it
superseded.

**What is left in it, and none of it is the same kind of thing:**

| still here | what it is |
|---|---|
| `MagnifierConfig`, `MagnifierShape`, `Magnifier` | a complete, tested screen magnifier that nothing constructs |
| `CursorSettings` | duplicates `AppearanceSettings::cursor_size`/`cursor_scheme`, and nothing draws a pointer at all (C-Q18) |
| `AccessibilityConfig` | the parallel config this entry is named for |

**Deleted 2026-09-13 -- all 1 239 lines, magnifier included. The paragraph
that used to stand here argued for keeping the magnifier, and it was wrong
on its facts.** It is kept in outline because the argument was a good one
and only the facts under it failed.

It said the magnifier was *"not a duplicate -- the only implementation of a
feature the roadmap lists as done"*, and that deleting it would misread
`design-decisions.md` 815: a magnifier is the desktop showing you
something, not a screen you open. Three things were checked before acting:

1. **It is not the only implementation.** `apps/magnifier/src/main.rs` is
   **5 769 lines**, has a real `fn main` running through `app::launch`, and
   opens *"Screen Magnifier -- the accessibility zoom for SlateOS, in a real
   window"*. It carries lens mode, docked modes, colour filters, a
   crosshair, a ruler and a pixel colour readout.
2. **It is not even the better one.** The dead `MagnifierShape` conflated
   two concepts in one enum -- `Circle`, `Rectangle`, `DockedTop`,
   `FullScreen` -- where the application separates `MagnifyMode` (with
   `DockedBottom` as well) from `LensShape`. Every `MagnifierConfig` field
   has an equivalent there.
3. **It did not magnify anything.** Its own doc says so: *"The magnified
   content itself is the compositor's; what is here is the frame around it
   and the placeholder the content lands on."* It drew a lens outline and
   crosshairs over an opaque rectangle. 131 lines of chrome for a
   magnification that nothing performed.

**The 815 argument survives the deletion and is recorded here instead,
because it is a design question and not a reason to keep placeholder
code.** It is genuinely unsettled where this feature ends up:

| | |
|---|---|
| A lens the *shell* composites over everything | what 815 implies, and what the deleted code was the frame for |
| A *window* that shows a magnified view | what `apps/magnifier` is, and what ships today |

**Neither can magnify the screen, and both are blocked on the same missing
thing.** `apps/magnifier`'s `sample_pixel` is labelled *"a stub for a
compositor capture that does not exist yet"* and returns a procedural
pattern; the shell copy left the content to a compositor that was never
asked. Until the compositor can hand out screen contents, the question of
*where* the magnifier lives cannot be answered by either codebase, and
keeping the smaller one alive did not move it any closer.

**Nothing else was lost, checked type by type.**

| what was in it | where the live one is |
|---|---|
| `HighContrastTheme` | a `pub use` alias of `appearance::HighContrastScheme`, whose own docs are fuller and cite §816 |
| `ColorFilter` | a `pub use` alias of `appearance::ColorFilter` |
| `MagnifierConfig`, `MagnifierShape`, `Magnifier` | `apps/magnifier` |
| `CursorSettings` | `AppearanceSettings::cursor_size` / `cursor_scheme` |
| `AccessibilityConfig` | `appearance::AppearanceSettings` and `gui/inputsettings` |

Deleting a file deletes its reasoning, so the prose was read before the
code. Two passages were load-bearing -- why high contrast is exempt from
palette conversion, and why the lens ink stopped being white. The first is
already stated where the real type lives, with the §816 reference this copy
lacked; the second is in this file.

`cargo test -p desktop` goes from 2 954 to 2 922, which is exactly the 32
tests that were in the file and nothing else.

**What the deletion does lose the only record of, so it is written here.**
Three fields had no live equivalent, confirmed by grep across `gui` and
`apps`:

| wanted | state |
|---|---|
| `screen_reader` | **no implementation anywhere** -- zero hits outside the deleted file |
| `text_scale` | wanted as a *multiplier*; `FontSettings::ui_size` is absolute, which is a different control |
| `visual_alerts` | a field of the same name exists in `apps/settings` and reaches nothing |

That table is the record now, which is this entry's own standing rule: a
dead field may go once what it recorded lives somewhere else, and somewhere
else may be this file. `caret_width` and `focus_indicator` left the same
way -- and both were *built* first, because this table had recorded them.

**This is the unfinished remainder of a cleanup that already happened.**
`TD-C-STICKY-FILTER-AND-MOUSE-KEYS-ARE-BUILT-TESTED-AND-CONNECTED-TO-NOTHING`
found the same defect in the *keyboard* third of these settings — sticky keys,
filter keys, mouse keys existing three times over with no two copies connected —
and fixed it by moving one definition into `gui/inputsettings`, a crate the
compositor, the Settings app and the shell can all see. That fix is the
precedent; what is left in `a11y.rs` is the visual third, not yet done.

**2026-09-13: three of these rows had one cause, and it was a missing hook.**
`caret_width`, `cursor` and the desktop's `icon_size` were each logged
separately as a setting with a working control and no reader. The cause is
shared: `oswindow::app::App` handed an application a **`Palette`** and nothing
else, and a palette is colours. There was no route by which a program could
learn a non-colour appearance setting, so each was stored, clamped, persisted,
round-trip-tested and inert.

`App::appearance_changed(&mut self, &AppearanceSettings)` is that route, called
immediately before `theme_changed` from the same two places. Its default does
nothing, for the reason `theme_changed`'s own doc gives: 94 applications
implement that hook and should adopt this one at their own pace rather than in
a single commit touching every program in the tree.

`caret_width` is read end to end now. `apps/launcher` takes it in
`appearance_changed` and draws its caret at that width, with a test that asks
for 3x and asserts the drawn line is three times wider; the shell's run dialog
takes it through `DesktopShell::set_appearance`.

**The test written to prove this had the very defect it was written against.**
`the_caret_width_scale_reaches_a_width_in_pixels` asserted
`CARET_WIDTH * s.caret_width_scale` -- it performed the multiplication a
caller would have to perform, so it was a test of `*`, and it would have passed
with no caller and no helper anywhere in the tree. Its own doc comment says
the test that matters is *that a caller can get from the settings to a width in
pixels*. It calls `AppearanceSettings::caret_width()` now.

**One more unreachable module, found on the way.**
`gui/desktop/src/launcher.rs`'s `LauncherState` is constructed only in its own
tests -- the shell imports `AppEntry` and `Category` from that module and
nothing else, and the launcher that runs is `apps/launcher`, which has its own.
Wiring the caret into the shell's copy was the first thing tried and would have
been theatre. It belongs on **C-Q17**'s list.

**Why the tests did not catch it.** `text_scale` and `caret_width` each have
passing tests that round-trip them through the config file and check clamping
(`text_scale=100` clamps to 3.0). They prove the setting is *stored*, not that
anything reads it. A dead setting with green tests looks maintained.

**A latent defect in the same code, should any of it be revived:** the parse
does `v.clamp(0.5, 5.0)` on a value from `str::parse::<f32>`, which accepts
`"nan"`. `f32::clamp` returns NaN for a NaN input, so `caret_width=nan` in the
config yields a NaN width rather than being rejected. Any revival wants an
`is_finite` guard, not just a clamp.

**Proper fix:** delete `a11y.rs`, and add the three genuinely-missing settings
to `appearance::AppearanceSettings` alongside `high_contrast`, following the
`inputsettings` precedent. No new channel is needed and none is possible in the
obvious direction — `gui/appearance` **depends on** `gui/toolkit` (`Palette` is
built from the toolkit's `Color`), so the toolkit cannot name `Palette` and
cannot depend on `appearance` without a cycle. The way a per-user value already
crosses that boundary is that the *caller* passes it in: `textedit::SingleLine`
takes a `color` field. A caret width travels the same way, as one more field.

**A smaller real inconsistency to fix with it:** the carets that are drawn
disagree about width for no recorded reason — `textedit::push_caret` draws a
1.0-wide `Line` (3 callers), `pathbar.rs` a 2.0-wide `FillRect`
(`CURSOR_WIDTH`), `launcher.rs` 2.0 inline, `run_dialog.rs` 1.0 inline. Six
sites, three widths. A multiplier means nothing until they share a base.

**If never fixed:** 1,360 lines that read as the accessibility subsystem, and
are not. The next person to wire an accessibility feature will find this module
first — it is the one that is *named* for the job — and add to the copy nobody
reads. That is precisely what happened with sticky keys.
