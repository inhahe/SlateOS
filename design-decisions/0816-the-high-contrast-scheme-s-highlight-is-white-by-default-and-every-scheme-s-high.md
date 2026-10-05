## 816. The high-contrast scheme's highlight is white by default, and every scheme's highlight is user-configurable

**Date:** 2026-09-07
**Lane:** C
**Decided by:** Operator for the configurability requirement, which was theirs and is binding; Claude for the white-over-cyan default, on the operator's explicit invitation to decide it

**In short:** in the "green on black" high-contrast scheme the highlight was
about three times dimmer than in the other schemes, so the selected item was
hard to pick out. It becomes white. Separately, and regardless of that pick,
the highlight colour becomes something the user can change in any scheme.

**The question.** `open-questions.md` -> C-Q7. Whether to brighten it, and to
what.

**What the operator said.** That they have no problem with a white highlight
and asked why it has to be a colour at all; that cyan should survive red-green
colour-blindness because its blue component is the distinguishing one, as it
is for magenta; that another option might be equally good; that this is
theory and I may know the practice better; and -- the part that is a
requirement rather than a preference -- that whatever the default is, **the
user should be able to configure it.**

**The operator's colour-vision reasoning is correct.** Protanopia and
deuteranopia (jointly around 8% of men) confuse red against green while the
blue channel stays intact, which is exactly why cyan and magenta remain
distinguishable from both. Nothing to correct.

**But the first sentence is the stronger argument, and it decides it.** A
highlight does not have to carry its meaning in hue at all. *Luminance*
contrast is read identically by every form of colour vision including
monochromacy, and by anyone on a failing panel or in direct sunlight. White on
black is the maximum available and is hue-free. Cyan is defensible; white is
unimprovable -- and "high contrast" is what the scheme is called.

**Amended the same day, on trying to implement it.** Both halves of this
decision meant something other than they appear to, and the entry would be
misleading without saying so.

The **configurability requirement is already satisfied** for everything on
screen: the live highlight is `Palette::highlight_fill`, which is the user's
accent with an alpha applied, and the accent is already an Appearance setting.
Nothing had to be built for it.

The **white** half lands in a module nothing calls. `HighContrastTheme` in
`gui/desktop/src/a11y.rs` is a pinned orphan island, and there is a second,
equally unreferenced accessibility module beside it modelling the same feature
differently. So high contrast cannot be switched on at all, and this entry
changes no pixel until that is fixed. The colour was changed regardless -- the
decision stands and the value should be right when the feature is wired -- and
the gap is written up as `TD-C-HIGH-CONTRAST-MODE-IS-NOT-CONNECTED-TO-ANYTHING`,
which also carried the one design point this decision did *not* settle:
whether, in high contrast, the accent follows the user's setting or the
scheme's.

**Settled later the same day, and the mode is now wired.** The accent
**follows the user's setting**, because a scheme-fixed accent would make the
highlight the one colour this mode does not let you change -- which
contradicts the requirement above. The contrast risk that argued the other way
is handled without overriding anyone: for a named accent the *hue* is kept and
the better-contrasting of its two existing values is used, which is exactly
what `for_mode` already does for every other role, so it is a choice between
two spellings of the user's colour rather than a substitution of it. A
`Custom` accent is used verbatim -- an exact colour is an exact request, and
there is no second value to choose between.

The scheme now lives in `gui/appearance` as `HighContrastScheme`, and
`Palette::from_settings` branches on an `AppearanceSettings` field, so the mode
applies to every surface at once.

**Complete as of 2026-09-07; this entry said otherwise until then.** It used to
end "What remains is that no Settings control sets it yet", which stopped being
true and was not updated. The chain is whole and checked end to end:
Settings → Accessibility → High Contrast (`DropdownId::HighContrast`, offering
"Off" plus every scheme) writes `appearance.settings.high_contrast`, which
`AppearanceFile` persists under `theme.high_contrast`, which
`Palette::from_settings` and the compositor's `DecorationTheme::from_settings`
both read. `apps/settings` covers the control with
`the_list_offers_off_and_every_scheme` and
`choosing_a_scheme_sets_it_and_choosing_off_clears_it`; its 213 tests pass.

Recorded because a stale "what remains" line is worse than no line: it is the
same failure as the seven stale `known-issues.md` entries closed on 2026-09-07,
and it invites someone to build a control that already exists.

**The one real argument against white**, and the reason the configurability
requirement matters more than the default: a user who picks "green on black"
may want it to *look* like a green terminal, and a white highlight breaks that
identity. That is a taste the system should not be legislating, which is
precisely what a setting is for. Cyan stays available.
