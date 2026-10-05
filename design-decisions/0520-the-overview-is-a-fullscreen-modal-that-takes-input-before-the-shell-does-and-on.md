## 520. The overview is a fullscreen modal that takes input before the shell does, and one layout pass answers both "where is it drawn" and "what did I click"

**Date:** 2026-08-22
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The desktop has an Exposé screen — press a key, see a card for
every open window, click one to switch to it. It was written months ago,
complete and tested, and nothing ever showed it (`known-issues.md`,
`TD-C-THE-OVERVIEW-SCREEN-IS-1856-LINES-NOBODY-CALLS`). §519 put each window's
rectangle on the wire, which was the missing ingredient; this entry is about the
four choices made in connecting it up. The short version of all four: while the
overview is on screen it is the only thing on screen, it works out where
everything goes exactly once, and two features that could not possibly work were
deleted rather than kept.

### 1. One layout pass, shared by drawing and hit-testing

`overview::overview_layout(state, config, screen_w, screen_h) -> Vec<ThumbnailLayout>`
is the single answer to "where does each card go". `render_overview` draws those
rectangles; `DesktopShell::press_on_overview` hit-tests the same ones. Neither
works the layout out for itself.

*Alternative rejected:* let each side compute what it needs, which is how the
module was written — the grid maths lived inline in `render_overview`, and
`on_mouse_click` took `screen_w`/`screen_h` so it could redo the parts it
needed. That is cheap, and it is wrong in a specific way: the two computations
drift, and the symptom is that a click selects the window *next to* the one that
was lit. The snap overlay already made this call for the same reason —
`render_zone_overlay`'s doc says "what is lit and what a press would place are
the same answer to the same question" — and there is no argument for the
overview being different.

*Cost:* the hit-test path recomputes the layout on every press rather than
caching it. That is a few dozen rectangle divisions once per click, against a
class of bug that is invisible in every unit test and obvious to every user.
Caching would reintroduce the same hazard in a third form — a cache that is
stale rather than a computation that disagrees — so it was not done.

### 2. Modal: presses before `hit_test`, keys before the shortcut table

`DesktopShell::handle_press` routes to `press_on_overview` **before**
`sync_snap_area()` and `hit_test`. `handle_hotkey` routes to `key_on_overview`
**before** `DesktopAction::for_chord`. Both orderings are load-bearing:

| Without it | What the user sees |
|---|---|
| press routed after `hit_test` | Clicking the bottom strip presses a taskbar button, through an opaque fullscreen overlay |
| keys routed after the chord table | Typing `d` into the search box runs Show Desktop |

*Alternative rejected:* leave the overview as one more thing `hit_test` knows
about, ranked above the taskbar. That is the right shape for the start menu and
the calendar, which are *panels* — they occupy a rectangle, and everything
outside it still belongs to whoever was there. The overview is not a panel; it
covers the screen and dims it. A hit-test that has to be told "…and this one
covers everything" is expressing modality in the vocabulary of layering, which
works right up until something is added above it.

*The exception that proves it is modal, not deaf:* the chord that opened it
still reaches the table, so `Super+Tab` closes it. `key_on_overview` checks
`for_chord(...) == Some(ToggleOverview)` first and hands that one case back. A
toggle you can only press once is not a toggle, and a user who does not remember
which chord they hit is otherwise left looking at a screen they cannot dismiss —
which is also why `dismiss_popups` closes it, so `Esc` works even from a state
the key handler did not anticipate.

### 3. The "+" add-desktop button: deleted, not documented

The overview drew a "+" in the bottom-right corner and returned
`OverviewAction::AddDesktop` when it was clicked. Nothing could act on it:
`DesktopShell::num_desktops` is fixed at construction and is the only bound
either side enforces (§518). Making the desktop count mutable is a real decision
with a real design behind it, and it is not this task.

*Alternative rejected:* keep the button, wire it to a no-op, note it in
`todo.txt`. That is precisely the defect this whole task exists to remove, one
button large — a control that draws, hit-tests and reaches nothing. Shipping a
fresh instance of a bug while fixing the old one is not a trade worth making for
a corner glyph.

It was also, incidentally, the only thing in the overview positioned
independently of the layout pass — placed relative to the bottom-right corner
rather than taken from `layouts` — and therefore the only thing that could be
drawn in one place and clicked in another. Deleting it let `on_mouse_click` drop
its `screen_w`/`screen_h` parameters, which is decision 1 becoming true of the
whole module rather than most of it.

### 4. The open animation: deleted, because it had never run and could not

`OverviewState` carried `animation_progress: f32`, stepped by `tick_animation`,
and `show()` set it to `0.0`. Every draw path began
`if progress <= 0.0 { return }`. **There is no clock in the shell.**
`oswindow::EventLoop::run` blocks in `Connection::wait()` when there is no input,
and `wait` takes no timeout; there is no timer, no deadline and no frame callback
anywhere in the stack. So `tick_animation` had no caller and could not have one,
progress never left zero, and the first genuinely working build of this feature
drew a blank, un-clickable, fullscreen overlay.

*Alternative rejected:* keep the field, initialise it to `1.0`, step it if a
clock ever appears. That reads like prudence and is not. It preserves the
*possibility* of a fade at the cost of a live branch on the drawing path with
exactly one correct value, and leaves the next reader to discover for themselves
that `tick_animation` is unreachable. The animation is not deferred polish — with
no clock it is a feature that cannot exist, and code that cannot run is not an
asset.

*What was kept:* the reasoning, in a comment where the function was, pointing at
`known-issues.md` → `TD-C-THE-SHELL-HAS-NO-FRAME-CLOCK`. That entry is the real
finding here and is much larger than one fade: `gui/desktop/src/animations.rs` is
1036 lines with six `tick()` methods and **no caller anywhere in the tree**, for
the same reason. The fade comes back when there is a clock to run it from.

### What made this findable

Every test in `overview.rs` drove the module directly, and all of them passed
against an overlay that rendered nothing. That is not a gap in the tests'
coverage of the module — it is the module's own tests being unable, in principle,
to ask whether anything calls it. Both defects found here surfaced within an hour
of something real being plugged in, and neither was visible before. See §519's
closing note and `TD-C-VIRTUAL-DESKTOPS-HIDE-NOTHING` for the same shape twice
more.

### Verification

Eleven defects reintroduced one at a time into the real tree with
`D:/tmp/reintro.py`, each reverted afterwards and every touched file re-checked
byte-for-byte by SHA-256 (`restored: True`). A test that does not fail against
the marker naming its own defect is not a regression test for it, whatever it is
called.

| Marker | Defect put back | Tests that failed |
|---|---|---|
| `shellnooverviewrefresh` | the window list refreshes the taskbar but not the overview | 6 |
| `overviewdropsgeometry` | the projection files a zero rectangle, as before §519 | 2 |
| `overviewlanesfromwindows` | lanes derived from the windows that exist, so an empty desktop vanishes | 1 |
| `overviewkeepsstalehover` | a hover outlives the window it named | 1 |
| `overviewpressfallsthrough` | presses reach `hit_test` from behind the overlay | 2 |
| `overviewkeysfallthrough` | keys reach the shortcut table before the search box | 1 |
| `overviewtogglenotreentrant` | the opening chord arrives as a bare `Tab` and no longer closes it | 1 |
| `overviewclickrelayout` | hit-testing computes its own layout, against a transposed screen | 1 |
| `overviewsurvivesdismiss` | `dismiss_popups` clears everything except the fullscreen overlay | 1 |
| `overviewneedsasecondstep` | drawing gated on state `show()` clears — the animation defect, respelled | 3 |
| `overviewaddsdesktopbutton` | the "+" glyph comes back | 1 |

**One marker caught nothing on the first run, and the test was the thing at
fault.** `overviewclickrelayout` transposes width and height inside the hit-test
path, and `a_click_selects_the_window_whose_card_is_under_it` — the test written
specifically to guard decision 1 — passed against it unchanged. The reason is
worth recording, because it generalises: the test asked `overview_layout` where
the middle card was and then clicked there. Transposing the screen inside that
function moved the question and the answer together, so the assertion held just
as well against a layout with no relationship to what was on the glass. A test
that recomputes the layout it is checking cannot fail when the layout is wrong,
however precisely its name states the property.

It now reads the click coordinate back out of the **render tree** —
`render_thumbnail_card` emits a card's background `FillRect` immediately before
the `Text` holding its title, so the drawn geometry is recoverable from the
output — and asks the question the user asks: I clicked the middle of the card I
can see, did that select the window whose title is written on it? It fails
against the marker now. *This is the second time on this task that a passing test
turned out to be answering a question nobody asks; the first was the whole
module.*

**Three tests are honest additional coverage, not proved regression tests:**

- `a_query_matching_no_title_matches_no_window` — guards the `app_name` deletion.
  No stage-2 marker touches search, because search is internal to the overview
  and the seam being wired is the one around it.
- `escape_leaves_the_overview` — survives every marker because **two independent
  paths close it**: `key_on_overview` turns `Esc` into `OverviewAction::Close`,
  and `dismiss_popups` hides it regardless. That redundancy is deliberate (see
  decision 2), so no single marker can break it, and a marker that broke both at
  once would be testing my patch rather than the code.
- `the_bottom_right_corner_is_not_a_button` — the click region **cannot be
  reintroduced** without restoring the `screen_w`/`screen_h` parameters that
  decision 3 deleted from `on_mouse_click`, which is a signature change across
  every call site. That unrepresentability is a stronger guarantee than the test:
  you cannot write this bug back without the compiler making you change the
  function's contract first. (`nothing_draws_an_add_desktop_button` guards the
  *drawing* half and is proved, by `overviewaddsdesktopbutton`.)
