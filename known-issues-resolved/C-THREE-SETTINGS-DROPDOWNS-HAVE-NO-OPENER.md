## `C-THREE-SETTINGS-DROPDOWNS-HAVE-NO-OPENER` — **fixed**

**Fixed** by collapsing every Settings page into a single description walked by
two interpreters, so the row drawn at a given position *is* the row clicked
there. See `design-decisions.md` §475. The three dropdowns turned out to be a
symptom rather than three omissions: the same duplication had also left every
per-app permission switch outside the Location list inert, and all five
pointer-size buttons on the Interaction page. `DropdownId::ALL` now exists so
`test_every_dropdown_has_something_that_opens_it` can walk the enum, and eight
further tests click controls at the coordinates the page itself reports. A
one-row drift injected into the hit test fails nine of them.

**In short:** the Settings app draws three rows that look like the other
dropdowns and show a current value — "Input Device", "Diagnostic data
collection" and "Verbosity" — but clicking them does nothing. Both halves of
each one are fully written; what is missing is the single line that opens the
popup when the row is clicked.

**Where:** `apps/settings/src/main.rs`. `DropdownId` (line 793) has ten
variants. `dropdown_layout()` has a complete arm for all ten
(`InputDevice` at 2864, `DiagnosticLevel` at 2891, `NarratorVerbosity` at
2939) and `apply_dropdown_selection` has a complete arm for all ten (3740,
3752, 3767). But `show_dropdown(DropdownId::…)` has only **seven** call sites
outside tests — Resolution, RefreshRate, Scale, OutputDevice, IpConfig,
CursorSize, ColorFilter. The three above have none.

The rows themselves *are* drawn: `render_setting_row(…, "Input Device", …)` at
1678, `"Diagnostic data collection"` at 2345, `"Verbosity"` at 2406 with
`self.narrator_verbosity.label()` beside it. So the user sees a control with a
value and no way to change it.

**How to confirm:** `grep -n 'show_dropdown(DropdownId::' apps/settings/src/main.rs`
and compare the variants found against the ten in the enum.

**The proper fix:** add the hit-test and `show_dropdown` call for each of the
three, next to the row that draws it, exactly as the seven working ones do.
Small and self-contained. Worth doing together with a test that asserts every
`DropdownId` variant has an opener, so an eleventh dropdown cannot be added
half-wired — the enum is the list to iterate, which makes that test cheap and
exhaustive.
