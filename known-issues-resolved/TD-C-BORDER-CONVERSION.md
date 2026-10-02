## TD-C-BORDER-CONVERSION — APPLICATIONS DONE, SHELL AND 211 STRIPS WAITING ON A DECISION -- FIXED 2026-09-13

**Date:** 2026-09-11. **Lane:** C.
**Where:** run `python gui/appearance/survey-fills.py` for the live count;
`gui/appearance/convert-fills.py` is the converter and the record of what it
refuses to touch.

**In short:** every application now follows the theme — switch between outlined
and filled boxes in Settings and they change. The desktop shell does not, and
neither do toolbars and status bars anywhere. Both are waiting on a decision
rather than on work.

**CLOSED 2026-09-13. Both things it waits on were decided the day after it
was written, and both were then done.**

| this entry says | actually |
|---|---|
| 158 `gui/desktop` sites blocked on C-Q13 | C-Q13 answered 2026-09-12 (834); the shell converted the same day in `fc2e22bef`, *"The shell draws through the theme: 2915 tests green"*. The converter now reports **0 convertible** across all 58 shell files |
| 211 strips blocked on C-Q14 | C-Q14 answered 2026-09-12 (835); `Surface::Strip` exists and is used in 53 files |

**What was actually left was somewhere neither entry looked: `gui/toolkit`.**
The sweep was scoped to applications, then the shell, and the shared widget
library both of them draw through was in neither list. Twenty sites. Fifteen
are now converted; the five that remain are declined on purpose and each
carries its reason in the code beside it — four buttons and one column
header row. Running the converter again reports exactly those five, so the
number is a list of open questions rather than of work.

**The classifier was wrong three times in twenty, always the same way.** It
offered `Strip(Edge::Bottom)` for two menu *entries* (`MenuBarEntry::Check`
and `::SubMenu` — it matched "Bar" in the type name) and for a file
dialog's address bar (an input well — it matched "bar" in the comment).
That is the same shape as the three misclassifications this entry already
records, and it is the whole reason it says to read site by site rather than
sweep. The tool did the typing; reading caught the three it got wrong.

**Eight tests across four helpers had to change, all for one reason.** Each
found a highlight or a pill by looking for a `FillRect` of a particular
colour. Under a role the paint is the theme's decision, so the match stops
firing — and reads as "nothing was painted", which sends the reader to the
hover logic rather than to the paint. They match by *position* now, and each
undoes the half-pixel inset a 1px stroke is drawn with (`x + 0.5`,
`width - 1.0`) so both styles answer in the same coordinates. The general
lesson is worth more than the fix: **a test that locates something by its
colour breaks the moment the colour stops belonging to the call site.**

**Done:** 436 draw sites across roughly 60 applications, plus the model
(`appearance::surface`), the theme setting, and the Settings page with its
preview. All green, all on `main`.

**Remaining, and why each waits:**

| count | what | blocked on |
|---|---|---|
| 211 | toolbars, status bars, tab strips, data bars | **C-Q14** |
| 158 | `gui/desktop` — the shell | **C-Q13** |
| ~25 | sites inside test modules, and a handful of odd shapes | nothing; they are noise |
| 2 | hairline separator rules | nothing; correctly left alone |

**C-Q13 is the one that matters.** The applications follow the theme and the
shell around them does not — the taskbar, launcher, notification pane and every
settings panel keep their fills whichever style is chosen. That is visible, and
the shell is the half a user looks at most. The question is whether every
selected row takes the accent outline; the shell already has a stricter rule
(`the_accent_marks_where_you_are_and_never_what_a_thing_is`) with tests
enforcing it, and converting the shell breaks 28 of those tests — most
mechanically, two of them substantively. See
`TD-C-THE-SHELL-NEEDS-CONVERTING-BY-HAND-NOT-BY-SWEEP`.

**C-Q14 is safe to leave.** Doing nothing selects the option that keeps today's
appearance for those 211 sites.

**What the conversion learned not to convert,** each from a case that broke
something, and each now a commented rule in `convert-fills.py`:

| category | what happened |
|---|---|
| control tracks | a switch track outlined reads as an empty box |
| data bars | a statistics bar says what it says by the area it fills; jsonviewer's tests caught it in a minute |
| structural strips | a box around a full-width toolbar reads as a box that failed to fit |
| hairline rules | a 1px fill outlined is a rectangle of zero height; the tray's calendar rule broke the test that measures whether a six-row month fits |
| `if let Some(x) = …selected…` | a *lookup* of the selected item to draw details about it, not a test that this box is selected |
| bare `current` | matched `self.current_utc`, naming what a card shows rather than what is chosen |

**The recurring test failure had one cause** and is worth knowing before touching
the shell: a helper that finds a box by matching `FillRect` on a colour stops
finding it when the box becomes an outline. `appearance::logical_rect` returns
the rectangle the caller asked for either way. The better-shaped fix, where a
test can take it, is to ask the palette — `palette.surface_paint(Surface::ControlTrack).fill`
— rather than naming a shade, which is what the system tray's slider test now
does.
