## 838. The palette moves down into the toolkit; the settings stay above it

**Date:** 2026-09-13. **Lane:** C. **Decided by:** Claude (autonomous).

**In short:** the shared widgets -- dialogs, menus, tab strips -- each keep a
private table of dark colours, so a light-theme user gets a dark dialog on a
light window. They were left out of the conversion because the crate that owns
the user's colours sits *above* the toolkit and a widget cannot name it. The
decision is to move the palette *down* into the toolkit rather than give the
toolkit a second table of its own.

**The constraint.** `gui/appearance` depends on `guitk`. A widget reaching for
`appearance::Palette` is a dependency cycle, which is why 122 constants are
still sitting in eleven widget files.

**What was rejected, and why it is not a fresh idea.** Giving the toolkit its
own colour table was tried and deleted: 810 removed a `Theme` of 32
widget-role colours, four built-in themes and a `ThemeManager`, none of it ever
constructed, whose hand-written Catppuccin Latte table had comments naming
roles one rung off the values beside them and shipped a `text_secondary` at
4.37:1. Two theming systems is one too many, and the unused one was the wrong
one. Re-introducing a `WidgetPalette` -- even a small one, even with no
defaults -- puts the tree back where 810 found it.

**The decision.** `Palette`, `Surface`, `SurfaceStyle`, `StripStyle`, `ink`,
`ink_on` and `push_surface` move into `guitk`, beside `Color`,
`RenderCommand` and the WCAG arithmetic they already use. `gui/appearance`
keeps everything that makes a palette out of a *preference*: the YAML file,
the fourteen accents, the high-contrast schemes, the transparency levels. It
re-exports the moved types, so `use appearance::Palette;` in 153 files is
untouched.

`Palette::from_settings(&AppearanceSettings)` is the awkward seam -- an
inherent method over a type that must stay above -- and it is called at 458
sites, so its spelling cannot change. `guitk` declares a `PaletteSource` trait
(is it light, which accent, which styles, what alpha, which high-contrast
colours) and `appearance::AppearanceSettings` implements it. The call sites
compile verbatim.

**The cost, stated plainly.** The *definition* of a palette -- which roles
exist, what the 4.5:1 floor is, what `Surface::Selected` means -- becomes the
toolkit's. That is a widening of the toolkit's remit: it was a widget library
and is now also the owner of the desktop's colour vocabulary. The alternative
readings are that the vocabulary always belonged there (every widget needs it,
and the contrast arithmetic is already there) or that it belongs in a third
crate below both. A third crate was not chosen because it would be a crate
containing one type used by its only two neighbours, and because `guitk` is
already the place a widget asks a colour question.

**What it unblocks.** Each widget's `render` takes `&Palette` and its
constants go, exactly as fifty-five other crates' did -- and the two sweeps
can do most of the bodies once the type is reachable. Logged as
`TD-C-THE-TOOLKIT-S-WIDGETS-STILL-PAINT-THEMSELVES-DARK`.

**Reversible?** Yes, and cheaply, because the re-export means callers never
learn where the type lives.
