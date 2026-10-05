## TD-C-THE-TOOLKIT-S-WIDGETS-STILL-PAINT-THEMSELVES-DARK -- FIXED 2026-09-13

**In short:** the shared widgets -- dialogs, menus, tab strips, the grid, the
modal overlay -- each keep their own private table of dark-theme colours. A
user on a light theme who opens a folder in the file manager gets a *dark*
modal dialog on a light window. Twelve applications were converted to the
user's palette this week; the widgets they are built from were not, because
the widgets cannot reach the palette without a dependency cycle.

**Where:** `gui/toolkit/src/`, 122 constants over eleven files --
`modal.rs` 17, `menu.rs` 14, `menubar.rs` 13, `grid.rs` 12, `textview.rs` 12,
`colorpicker.rs` 10, `dialog.rs` 10, `pathbar.rs` 10, `tabs.rs` 10,
`color.rs` 9, `disabled.rs` 3 -- plus 148 inline literals.

**It is not dead code.** Four of those widgets are used by real windows:
`modal` by `explorer`, `diskcleanup`, `diskimager` and `partmanager`; `dialog`
by the desktop shell's run dialog and session panel and by three apps; `grid`
by `charmap`, `colorpicker`, `slides` and `worldclock`; `tabs` by `editor` and
`markdowneditor`. The colours reach the screen.

**Why it was not done with the other fifty-five crates.** `gui/appearance`
depends on `guitk`, not the reverse, so a widget cannot name `Palette` at all.
That is the same cycle recorded when the toolkit's own `Theme` was deleted
(design-decisions.md 810): 32 widget-role colours, four built-in themes and a
manager, none of it ever constructed, and its hand-written table already
disagreed with itself. Re-introducing a second table in the toolkit would
re-create exactly what 810 removed.

**Proper fix — invert the dependency, do not duplicate the data.** Move the
`Palette` type itself (and `Surface`, `SurfaceStyle`, `StripStyle`, `ink`,
`ink_on`, `push_surface`) down into `guitk`, where `Color`, `RenderCommand`
and the WCAG arithmetic already live, and leave the *settings* half in
`gui/appearance`: reading `appearance.yaml`, the accent list, the
high-contrast schemes. `appearance` then re-exports the type, so every
`use appearance::Palette;` in 153 files keeps working untouched.

The one friction is `Palette::from_settings(&AppearanceSettings)`, an inherent
method needing a type that stays in `appearance` -- and it is called at 458
sites, so its spelling must not change. The answer is a trait declared in
`guitk` and implemented in `appearance`:

```rust
// guitk
pub trait PaletteSource {
    fn is_light(&self) -> bool;
    fn accent(&self) -> Color;
    fn surface_style(&self) -> SurfaceStyle;
    fn strip_style(&self) -> StripStyle;
    fn panel_alpha(&self) -> u8;
    fn high_contrast(&self) -> Option<HighContrast>;   // the resolved colours,
}                                                      // not the scheme enum
impl Palette { pub fn from_settings<S: PaletteSource>(s: &S) -> Self { .. } }
```

`Palette::from_settings(&settings)` then compiles verbatim at all 458 sites,
with no cycle and no second table. The toolkit declares what it needs from a
settings source; `appearance` supplies it and keeps sole ownership of the file
format, the accent list and the schemes.

**Then** each widget's `render` takes `&Palette` and its constants go, exactly
as the fifty-five crates' did. The two sweeps -- `convert-fills.py` and
`ink-text.py` -- can do most of the bodies once the palette is reachable.

**Until it is done:** every window built from these widgets is dark whatever
the user chose. It is the largest remaining gap in design-decisions.md 822.



**Fixed 2026-09-13.** All 122 constants across eleven widget files are gone.
Every colour the widget layer draws is now either a role of the user's
palette or black at an alpha. design-decisions 838 is the enabling change --
`Palette` moved down into `guitk` so a widget could name one -- and the
widgets followed it in five commits: `dialog`, `modal`, `menu`+`menubar`,
`tabs`+`grid`+`pathbar`+`colorpicker`, and `textview`+`textedit`+`disabled`.

**Four of them were decisions rather than mappings**, and each is a comment
in the file it belongs to:

1. `ansi_color` takes no palette. An ANSI colour is fixed by the escape
   sequence, so threading one through its single fallback pushed the user's
   colours into `parse_csi`, `apply_sgr_params`, `set_text` and `append` --
   a parser learning about themes.
2. `RichSpan::link` carries no colour at all now. A constructor has no
   palette to ask and no notion of when the theme last changed, so the span
   says it *is* a link and the renderer picks the `link` role (832).
3. A text field's selection was `#0078D7` and white -- Windows' blue, the
   one colour here that was never Catppuccin. It is the accent with
   `readable_on` over it, carried as two fields of `SingleLine` beside the
   `color` already there. `widget.rs` has no palette, so `Style` grew the
   same pair with today's values as defaults.
4. `BORDER_COLOR` in the menus became `surface1`, not the palette's `border`
   role. That role is `text` -- a far heavier line than the pale grey a menu
   outlines itself with -- so mapping to it would be a redesign wearing the
   clothes of a conversion.

**What the surveys kept missing, stated once because it is the lesson.**
`disabled.rs` held three constants written `Color::rgb(49, 50, 68)` in
decimal, and every survey of this toolkit searched for `from_hex`. They
outlasted the other hundred-odd for no reason except spelling. A sweep
answers about the population it can see, and the *shape of the question*
decides that population -- the same lesson as the `#[cfg(test)]` latch that
hid 52 text sites and the bare-name key that gave `apps/weather` 270 call
sites.

**A corrected count for what is left**, taken with every spelling rather than
just `from_hex`: **741 colour constants across 55 app crates**. Ten crates
have five or fewer; the ten largest are all games (checkers 24, gomoku 23,
mahjong 23, rush 21, freecell 20, reversi 20, spades 20, flood 19, memory 19,
chess 17). Twelve of those are aliases of other constants in the same file --
`PLAYER1_COLOR = BLUE`, `CARD_BG = TEXT_COLOR` -- which no value-based survey
can see at all, because their value is a name. That work belongs to
`TD-C-SIXTY-EIGHT-APPS-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE` and to C-Q16,
which asks whether the games should follow the theme in the first place.
