### TD-C-SWITCH-KNOBS-ARE-LOW-CONTRAST-ON-THE-ON-PILL — 2026-08-22 — FIXED 2026-08-24 (`66ec4d983`)

**In short.** Every on/off switch in the desktop settings panels draws a small
round knob on a coloured pill. When the switch is **on**, the pill is the
user's accent colour and the knob is drawn in the ordinary text colour — which
on the stock dark theme is a light grey on a light blue, about **1.35:1**
contrast. (Contrast ratio: how far apart two colours are in lightness; 1:1 is
invisible, and readable text wants 4.5:1.) The knob is the part that tells you
*which side the switch is on*, so a user glancing at a settings page cannot
reliably tell an enabled row from a disabled one without reading the label.
The question is not what the fix is — it is `readable_on(pill)`, the same
helper already used for button labels — but that the wrong pattern is copied
into all 49 shell modules, so it has to be fixed in all of them at once or the
desktop ends up inconsistent with itself.

**Where:** every `toggle_bg` / `*_toggle_bg` / `auth_bg` site in
`gui/desktop/src/*_settings.rs` and friends. The shape is always:

```rust
let toggle_bg = if enabled { p.accent } else { p.surface2 };
// ... pill filled with toggle_bg ...
// ... knob filled with p.text        <-- this is the bug
```

`network_settings.rs` alone has four such sites; the pattern recurs in every
settings panel that has a switch. The correct expression is
`appearance::readable_on(toggle_bg)`, which yields near-black on a pale accent
and near-white on a deep one, exactly as it already does for `p.on_accent()`
on button labels.

**Why it was not fixed when it was found.** It was noticed during the
`network_settings.rs` palette conversion (module 16 of 49). Changing it there
and nowhere else would have left one settings panel whose switches look
different from every other panel's — a visible inconsistency introduced by a
conversion that is supposed to be behaviour-preserving. A redesign hidden
inside a mechanical substitution is a redesign nobody reviewed. It waits for
`TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE` part 2 to
finish, at which point every switch is reachable from one grep and the change
is one sweep with one test.

**Trigger:** when part 2 of the 49-module palette conversion is complete. Do
it as a single commit across all modules, with a shared test asserting that
every switch knob equals `readable_on` of the pill under it in both modes and
under every selectable accent.

**If never fixed:** the switches stay legible-but-mushy on the stock dark
theme and get worse on a pale accent (Yellow, Peach, Rosewater), where knob
and pill converge further. Nothing breaks; the settings pages are just harder
to read at a glance than they should be, and the defect is duplicated once
more every time a new panel is written.

**How it was fixed (2026-08-24, `66ec4d983`).** Not by substituting
`readable_on(toggle_bg)` at each site, which is what this entry proposed, but
by *removing the sites*: `gui/desktop/src/switch.rs` now draws the control and
derives the knob, and seventeen modules call it. The substitution would have
fixed the seventeen copies that existed and left the shape open for the
eighteenth panel; the trigger note above ("one sweep with one test") already
implied the sweep would have to be repeated by hand for every new panel.

Two corrections to the scope recorded above, both found by measuring rather
than grepping for `toggle_bg`:

- It was **seventeen** modules, not "`network_settings.rs` alone has four."
  Several panels use `p.green` rather than `p.accent` for *on*, so a grep for
  the accent missed them, and they had the identical defect: `p.text` on
  `green` is no better than `p.text` on `blue`.
- The proposed shared test — "every knob equals `readable_on` of the pill in
  both modes and under every accent" — is now `switch.rs`'s
  `the_knob_is_legible_on_every_track_a_panel_can_choose`, but it could not be
  written as a floor over *every role*: `readable_on` picks between exactly two
  inks, and on a mid-grey (`overlay0`, `#6C7086`) the better of the two is
  4.32:1, so 4.5 is unreachable without a third ink. The floor is asserted over
  the colours a switch track can actually be, and a separate test asserts the
  weaker but universal claim (the choice between the two inks is the right way
  round) on all 42 role-mode pairs.

**The measured headroom is nine hundredths.** The tightest switch track in the
shell is light-mode `Maroon` at **4.60:1**. An accent added to
`AccentColor::presets()` that is a shade paler fails that test. That is the
intended behaviour — the accent is the thing to change, not the threshold —
but it is worth knowing before adding a hue.
