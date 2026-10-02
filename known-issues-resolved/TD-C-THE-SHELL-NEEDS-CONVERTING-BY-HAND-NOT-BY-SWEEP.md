## TD-C-THE-SHELL-NEEDS-CONVERTING-BY-HAND-NOT-BY-SWEEP -- FIXED 2026-09-12

**Date:** 2026-09-11. **Lane:** C.
**Where:** `gui/desktop/**` — about 106 convertible draw sites across 30 files.

**CLOSED: the shell was converted on 2026-09-12, the day after this was
written**, in `fc2e22bef` — *"The shell draws through the theme: 2915
tests green"*. The converter reports 0 convertible sites across all 58 files
of `gui/desktop` today.

This entry's method was right and is worth keeping for the next sweep: read
each site, decide per test whether the role claim still holds. The two
accent-policy tests it singles out were superseded by 834 rather than
rewritten to fit — the operator decided that selection takes the accent
everywhere, which is what those tests were the record of not yet knowing.

**In short:** every application has been converted to the new bordered theme.
The desktop shell itself has not, and should not be done the same way. Running
the converter over it works and compiles, and breaks **28 tests** — not
because the conversion is wrong, but because those 28 are the tests that keep
the shell's colours honest, and they state things the conversion changes on
purpose.

**What they are.** The `every_site_draws_the_role_it_claims` family, plus
`the_panels_own_surfaces_come_from_the_palette` and several accent-policy tests.
They assert specific role assignments — "the content well is `p.crust`", "the
filter field is `p.surface0`" — by finding a `FillRect` of that colour. Under
`SurfaceStyle::Borders` those boxes are outlines, so the fill is not there to
find.

**Why that is not simply a test update.** Two of them are about the accent:
`the_accent_marks_where_you_are_and_never_what_a_thing_is` (launcher) and
`only_the_active_filter_tab_follows_the_accent` (clipboard viewer). The
conversion draws **every** `Surface::Selected` as an accent outline, which is a
direct statement about accent policy — and the shell already has one, written
down and enforced. Whether "selected row" everywhere should take the accent, or
only the places the shell currently gives it to, is a design question those
tests are the record of. Rewriting them in bulk so a sweep passes is changing
the test to fit the code.

**Three misclassifications the shell produced before it was reverted,** kept
here because they are the shape to expect when it is done properly:

| site | classified | actually |
|---|---|---|
| `datetime_settings.rs:625` | Selected | the "Current time" *display card* — the guard matched `self.current_utc` |
| `language_settings.rs:610` | Selected | the "Current language" display card, same cause |
| `notif_pane.rs:1720` | Selected | a dismiss button shown on hover; an accent outline over-signals it |

The classifier was narrowed afterwards so bare `current` no longer qualifies —
it named what a card was *showing*, not a selection. `hover` was kept, because
`context_ext`'s two hovered menu rows are genuine selection and were right.

**How to do it:** file by file, reading each site, and deciding per test
whether the role claim it encodes still holds under borders or has genuinely
changed. Roughly 30 files; the mechanical part still works and can do the
typing.

**If never done:** the applications follow the theme and the shell around them
does not — the taskbar, launcher, notification pane and every settings panel
keep their fills whichever style is chosen. That is visibly inconsistent, and
it is the half the user looks at most.
