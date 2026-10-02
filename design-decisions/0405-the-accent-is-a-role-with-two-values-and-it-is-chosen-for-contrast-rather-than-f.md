## §405 — The accent is a role with two values, and it is chosen for contrast rather than fidelity to Catppuccin

**Date:** 2026-08-14
**Lane:** C
**Decided by:** Claude (autonomous)

**Context.** Wiring the saved appearance settings into the shell meant the
desktop finally had to paint a *light* theme — it had only ever had the
hard-coded Catppuccin Mocha one. An accent colour then stops being a single
constant: `AccentColor::Blue` has to mean one thing on a near-black base and
another on a near-white one, because the shell draws the accent as **text**
(the start glyph on the taskbar, the start-menu heading), not only as a fill.

**Decision.** `AccentColor` carries two values per name — `color()` for dark
backgrounds and `color_light()` for light ones — and `effective_accent()`
resolves between them from `theme_mode`. The light values are Catppuccin
Latte's hues **darkened until they clear 4.6:1 on the Latte base**, not Latte's
published values.

**Why the deviation.** Measured against Latte base `#EFF1F5`, the upstream
Latte accents are decorative, not text-safe: yellow 2.31:1, pink 2.34:1,
rosewater 2.34:1, sky 2.47:1, flamingo 2.64:1, peach 2.64:1, sapphire 2.78:1,
lavender 2.81:1, green 2.96:1, teal 3.31:1, maroon 3.48:1 — only blue (4.34, so
also nudged), mauve (4.79) and red (4.80) approach or clear the 4.5:1 that body
text needs. Shipping them unmodified would have meant a light mode whose start
menu heading is illegible for eleven of the fourteen accents. Each value is
therefore its Latte hue with all three channels scaled toward black by the
smallest factor reaching 4.6:1, which holds the hue — these still read as the
colours Catppuccin named. Blue, mauve and red are barely touched. The dark
palette needed no treatment at all: every Mocha accent is 7:1–13:1 on Mocha.

**Alternatives rejected.**

- *Ship Latte as published and accept the contrast.* The honest form of "match
  the upstream palette", and rejected because a heading nobody can read is not
  a style choice. Catppuccin publishes accents for syntax highlighting and
  decoration, where a 2.3:1 accent sits next to a label that carries the
  meaning; here the accent *is* the label.
- *Keep one accent value and never draw the accent as text.* Structurally
  cleaner — one constant per name — but it forbids the two places the shell
  already uses the accent to mean "this is the thing you pressed", and would
  push every accent onto a fill with a contrasting foreground, which is a much
  heavier visual language for a start-menu heading.
- *Derive the light value at runtime by darkening.* Rejected: the same loop
  that produced these constants would then run on every theme change, and a
  colour a user sees would be the output of a search rather than a value anyone
  can look up, diff, or override. The search was run once, offline; its results
  are the table.

**Consequences.** `every_accent_is_readable_as_text_in_both_modes` asserts the
4.5:1 bar for all fourteen accents in both modes, so the palette cannot regress
to the upstream values without a test failing. A **custom** accent is exempt —
`effective_accent()` returns `custom_accent` verbatim in either mode, because
the user named a specific colour rather than a role, and silently darkening it
would override the one choice that was stated in full. That exemption is the
known hole in the contrast guarantee.

**Companion decision — `ThemeMode::System` resolves to dark.** `System` means
"follow the system's light/dark schedule", and nothing in this tree computes
sunrise or watches a time-of-day trigger yet. It answers dark, because dark is
what the shell has always painted and what every other default is tuned
against; answering light would flip the whole desktop for a user who asked only
to be left on automatic. `ThemeMode::is_light()` is the single place that has
to change when a schedule exists.

**Companion decision — the resolved palette is a separate type from the
settings.** Render functions read `DesktopTheme` fields and never consult
`AppearanceSettings`. A renderer that derived colours itself would re-derive
them every frame and, worse, would be free to derive them slightly differently
in each of the dozen places a colour is drawn. `DesktopShell::set_appearance()`
is the single door: it re-derives the theme and stores the settings together,
so `theme` can never be a stale derivation of an older `appearance`. Each
surface also carries its own foreground, because a shared one is only correct
while every surface has a similar brightness — which "accent on the taskbar"
ends the moment it is switched on.

**Where it lives.** The light-accent palette and `AccentColor::color_light`,
`ThemeMode::is_light`, `AppearanceSettings::effective_accent` in
`gui/desktop/src/appearance_settings.rs`; `DesktopTheme::{dark, light,
from_settings}`, `readable_on`, `emphasized`, `taskbar_alpha` and
`DesktopShell::{set_appearance, load_appearance}` in `gui/desktop/src/main.rs`.
