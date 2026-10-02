## TD-C-THE-DESKTOP-AND-THE-COMPOSITOR-BOTH-DRAW-WINDOW-TITLE-BARS -- FIXED 2026-09-13

**In short:** Two different parts of the system each know how to draw a
window's title bar, borders and close/maximise/minimise buttons, and they
disagree about what one looks like. The compositor's version is the one that
actually runs. The desktop shell's version has never drawn a pixel, because
until today the shell could not be called by anything. Wiring the shell to the
compositor — the next task — would put the second one on screen *on top of*
the first, which is how this was found.

**Where:**

| | Compositor | Desktop shell |
|---|---|---|
| Entry point | `Compositor::render_title_bar` (`gui/compositor/src/lib.rs`), called from `render_window` | `DesktopShell::render_window_decorations` (`gui/desktop/src/lib.rs:2367`, 92 lines) |
| Geometry | `Window::title_bar_layout` → `TitleBarLayout` | `DesktopShell::window_chrome` → `WindowChrome` |
| Button size | `TITLE_BUTTON_SIZE = 20` | `WINDOW_BUTTON_SIZE = 16.0` |
| Title bar height | `TITLE_BAR_HEIGHT = 30` | `TITLE_BAR_HEIGHT = 30.0` |
| Display scaling | none | every dimension via `self.scale(…)` |
| Rounded corners | none | `corner_radii()`, from the user's `WindowCorners` setting |
| Drop shadow | `render_shadow`, `SHADOW_SIZE = 8`, 8 layers | yes, its own |
| Colours | `self.theme.title_bar_focused` / `close_button` / … | its own constants |
| Is it on screen? | **yes** | no — no caller but the demo |

*(Corrected: an earlier revision of this table claimed the compositor drew no
shadow. It does — `Window::shadow_extent` and `Compositor::render_shadow`. The
error is worth leaving visible here because it was made the same way the bugs
in this entry were: by reading one side carefully and asserting something about
the other without checking.)*

**This is the sweep's recurring shape once more**, and the third instance
found in two days: the tree held one correct answer that callers could not
reach, so it grew a wrong copy. Here the copy is not merely unused — it has
*drifted*, and the drift is invisible precisely because only one of the two
ever runs. Nothing compares 16 against 20.

**Which one is right: the compositor.** This is not a close call and is not an
open question. `gui/compositor/src/lib.rs`'s own module doc states the policy —
"Window decorations drawn server-side (consistent look, secure close button)" —
and the security half of that is the real argument: the close button must be
drawn and hit-tested by something the client cannot lie to. The compositor also
already owns everything decoration *does* as opposed to looks like: drag-to-move
(`title_bar_rect().contains`), resize edges, button hit testing, damage
tracking, and fullscreen's decoration suppression. Moving those to the shell
would mean moving input routing to the shell, which contradicts the
architecture. The shell's copy is the one to delete.

**But deleting it plainly would lose two real features**, which is the actual
work in this entry:

1. **Display scaling.** ✅ **Done 2026-08-21.** *(Original text kept below; the
   note at the end of this entry records what was built.)* The shell scales
   every decoration dimension by the user's scale factor; the compositor
   hardcodes pixels. On a 2× display the compositor's title bar is 30 physical
   pixels — half the height it should be, with 20px buttons inside it.

   The sharp part: **the compositor already knows the scale factor and does not
   use it.** `Display::scale_factor` exists, is set per display, and is reported
   out to clients over the wire (`wire.rs:357`) — and the only reads in the
   whole crate are that report. The compositor tells every client how to scale
   and then does not scale itself. So this is not "thread a setting in from the
   shell"; the value is already in the struct next to the code that ignores it.

   There is one chokepoint to change, which is why this is tractable:
   `Window::frame_insets` is documented as the single place the geometry derives
   from, precisely so decoration cannot be right in one place and wrong in
   another — "hit testing, damage and drag detection included". Scaling there
   carries the whole system. The one piece of design needed is how a `Window`
   learns *which* display it is on, since `frame_insets` takes only `&self`.
2. **Corner radius.** The user's `WindowCorners` setting (`Square`/`Rounded`/…)
   is honoured only by the shell's version. Unlike the scale factor this genuinely
   is not in the compositor yet and has to arrive from `AppearanceSettings`.

So the sequence is: scale the compositor's decorations off the value it already
holds, bring the corner-radius setting across, *then* delete
`render_window_decorations` and `window_chrome` and the geometry fields on
`ManagedWindow` that exist only to feed them. Not the other order — deleting
first ships a visible regression on every HiDPI display.

**Why this blocks the event loop** (`TD-C-THE-SHELL-CAN-DRAW-ITSELF-AND-NOBODY-CAN-ASK-IT-TO`):
that entry says the loop should "re-render the five trees and submit them". Four
of the five are genuine shell surfaces — taskbar, Alt-Tab, start menu, calendar.
The fifth is `render_window_decorations`, and submitting it would double-draw
every title bar in the desktop, at the wrong size. The loop must render four,
not five, and this entry is why.

*(Resolved: the decorator is deleted, so the shell now has exactly four public
`render_*` methods and the qualification is no longer needed — the loop
renders all of them. The note is left standing because it is the reason the
count in that entry changed, and a future reader comparing the two entries
would otherwise have to rediscover it.)*

**It also explains a hole in the window-list protocol that is not a hole.**
`WindowInfo` (`gui/remote/src/window_list.rs`) carries no geometry — no x, y,
width or height. That looks like an oversight the moment you try to decorate a
window from a shell, and it is not one: a shell that does not decorate does not
need window geometry, and a taskbar, Alt-Tab list, start menu and calendar
between them need none of it. Do not add geometry to `WindowInfo` to make the
shell's decorator work. That is the copy, not the original.

**Trigger:** do this before the shell event loop, or at minimum render only the
four shell surfaces in that loop and leave this entry open. Nothing is blocked
on another lane — `gui/compositor` and `gui/desktop` are both lane C's.

**If never fixed:** the shell keeps a 92-line renderer that cannot be correct
because nothing exercises it, and the first person to wire the loop up
naively gets doubled title bars and reasonably concludes the compositor is
broken.


**Progress 2026-08-21 — prerequisite 1 (display scaling) is done.** The
compositor now scales its own decorations. See `design-decisions.md` §497 for
the three decisions this needed; the mechanics, in brief:

- `Window::scale_factor` (decorations only — never the client area, which is the
  client's own pixels) is applied inside `Window::frame_insets`, the documented
  chokepoint, so hit testing, damage and drag detection all moved with the
  drawing rather than lagging it.
- `DisplayManager::display_for` / `scale_for` answer "which display is this
  window on" by largest intersection, not by which display holds the top-left
  corner.
- `Compositor::refresh_window_scales` recomputes every window's scale at the top
  of `compose_frame` and of `handle_input`, rather than the six placement sites
  each remembering to bump it.
- Four things sat *outside* `frame_insets` and each was its own bug:
  `render_shadow`'s layer count and its alpha falloff, `window_drawn_extent`'s
  damage allowance, and the title font size. All four are now scaled; 1×
  rendering is bit-identical to before.

Nine new tests, each proved a real regression test by reintroducing the bug it
guards (ten reintroductions in all, every one failing deterministically and
naming the right test).

**What is left in this entry is now prerequisite 2 and the deletion**: bring
`WindowCorners` across from `AppearanceSettings` into the compositor, then
delete `DesktopShell::render_window_decorations`, `window_chrome`, and the
`ManagedWindow` geometry fields that exist only to feed them. The ordering
argument above is unchanged — corner radius first, deletion second, because
deleting while the compositor still draws square corners ships a visible
regression for anyone who set `Rounded`.

**Correction 2026-08-21 — there is a *third* feature, and the count above was
wrong.** The entry has said since it was written that deleting the shell's
renderer "would lose two real features". Re-reading
`render_window_decorations` line by line before deleting it — which is the only
way to be sure — turned up a third, and the same reading found that
prerequisite 2 is not the narrow job the name "corner radius" suggests.

3. **Two shadow behaviours the compositor does not have.**
   - **The `drop_shadows` user toggle.** `AppearanceSettings::drop_shadows`
     (`gui/appearance/src/lib.rs:621`) is a real, YAML-persisted, settings-UI
     exposed switch (`gui/desktop/src/appearance_settings.rs:715`), and the
     shell honours it in four places, window decorations among them
     (`gui/desktop/src/lib.rs:2384`). The compositor's `render_window` calls
     `render_shadow` unconditionally for every window that has a title bar. So
     a user who turns drop shadows off today would see them turn off
     everywhere the shell draws and stay on around every window — and after
     the deletion, stay on everywhere, with the setting doing nothing at all.
   - **No shadow on a maximized window.** The shell suppresses it because a
     maximized window "has no edge to cast from — there is nothing beside it
     for a shadow to fall on, and one drawn anyway would bleed over the screen
     border". The compositor keeps `maximized` and `fullscreen` deliberately
     distinct (`gui/compositor/src/lib.rs:562`): fullscreen drops the title bar
     and so drops the shadow with it, but **maximized keeps its decorations**,
     and therefore gets a shadow drawn into the screen edge. Half of this is
     already right by accident; the maximized half is not.

**Consequently prerequisite 2 is "the appearance settings the compositor
ignores", not "corner radius".** Both remaining items — `WindowCorners` and
`drop_shadows` — are fields of the same `AppearanceSettings` struct that the
compositor has no connection to at all. The work is one channel carrying both,
not two unrelated features, and building the channel for corner radius alone
and then discovering `drop_shadows` needs the same channel is exactly the
band-aid accumulation `CLAUDE.md` warns about. Do them together.

**And the corner-radius half is not plumbing.** The compositor's render engine
*already receives* corner radii and throws them away: `execute_command`
destructures `corner_radii: _` on both `RenderCommand::FillRect` and
`StrokeRect` (`gui/compositor/src/lib.rs:2923`, `2938`, and again at `3055`).
Every rounded rectangle any client or the shell has ever submitted has been
rasterised square. So this needs rounded-rectangle fill and stroke implemented
in the rasteriser first — which also fixes a silent, tree-wide bug of its own,
independent of window decorations.

**Progress 2026-08-21 — the rasteriser now draws them.** ✅ The tree-wide half
of the paragraph above is fixed: `RenderEngine::fill_round_rect` and
`stroke_round_rect` rasterise rounded rectangles as scanline spans, and
`execute_command` passes each command's radii to them instead of discarding
them — `FillRect`, `StrokeRect` and `BoxShadow` alike. That restores rounding
for every producer in the tree at once, not only window frames: toolkit widget
borders, the SVG renderer's `rx`/`ry`, the tab strip, the launcher's search
field, the power menu and the resource monitor were all being drawn square.
See `design-decisions.md` §498. Twelve tests, each proved a real regression
test by reintroducing the bug it guards (eleven reintroductions, all caught —
and three tests had to be *repaired* first, because the first pass proved they
were guarding nothing; §498 records which and why).

**What is left of prerequisite 2 is now the settings channel**, not the
drawing: the compositor still has no connection to `AppearanceSettings`, so it
does not know the user's `WindowCorners` choice (its own decorations are still
drawn with the flat primitives) nor the `drop_shadows` toggle, and it still
draws a shadow on maximized windows. Note that `gui/appearance` depends only on
`guitk` and `yamldoc`, so `compositor` can depend on it without a cycle.

**Progress 2026-08-21 — prerequisite 2 is done, except for live reload.** ✅
The compositor now depends on `gui/appearance` and holds an
`AppearanceSettings` (the whole record, not the two fields it can act on — see
`design-decisions.md` §499 for why that is the point rather than an oversight).
All three items above are closed:

- **`WindowCorners` reaches the frame.** `Compositor::decoration_radius` scales
  the user's radius by the display factor and feeds the title bar's fill, the
  border's stroke, the title-bar buttons and every ring of the shadow. All four
  corner choices now produce four different windows; before this they produced
  one. Note the radius deliberately does *not* inherit `scale_dimension`'s
  non-zero floor: a radius of 0 is the user having chosen `Square`, not a
  rounding accident, and must survive scaling as a square corner.
- **`drop_shadows` is honoured.** `render_window` consults it before calling
  `render_shadow`.
- **A maximized window casts no shadow.** The compositor's reason is *not* the
  shell's, and §499 records the difference: the shell suppresses a fill-based
  `BoxShadow` that would bleed over the screen border; the compositor's
  concentric rings cannot bleed — a maximized frame is fitted to the display
  exactly, so every ring is either clipped off-display or drawn underneath the
  window's own frame. It is pure overdraw on an opaque window and a dark smear
  along the top and left on a translucent one.

Ten tests, each proved a real regression test by reintroducing the bug it names
— 10/10 caught on the first pass. Two of the ten were wrong before they were
right, and §499 records both, because neither error would have announced
itself: one asserted an absolute pixel count that is simply false (a "square"
20×20 corner block also contains the border stroke and the first letters of the
title), and one was reading `compose_frame`'s vsync gate instead of its damage
state, so its middle assertion passed for entirely the wrong reason.

**Progress 2026-08-21 — the startup-only channel is now a live one.** ✅ The
`ReloadAppearance` verb described here as "the last piece before the deletion"
is built, exactly to the shape this entry specified: tag `0x0F` in
`gui/remote/src/control.rs`, **no payload**, mapped in `to_compositor_request`
with no `link.resolve` (it names no window, so a client that has never opened
one may still send it — which matters, because Settings is such a client), and
handled by `Compositor::reload_appearance`, which re-reads the user's file.
`main` now calls the same method at startup instead of loading the file itself,
so startup and reload cannot disagree about where the settings live. Adopting
settings that turn out to be identical does *nothing*, so an unlimited-rate
notification cannot become an unlimited-rate full-screen repaint. The
application-facing sender is `oswindow::EventLoop::appearance_changed()`.
Rationale in `design-decisions.md` §500; seven tests, each proved a real
regression test by reinstating its bug and watching the named test fail.

**What remains before the deletion is a caller, and it belongs to another
entry.** `apps/settings` cannot send the request: it has no compositor
connection at all, which is `TD-NO-APP-CONNECTS-TO-THE-COMPOSITOR` (noted there
under "One of the 137 now has something specific it cannot call"). Nothing in
*this* entry's scope is blocked by that — the compositor draws the user's
corners and shadows, and adopts a change the moment anyone tells it to — so the
deletion below is unblocked.

The deletion: `DesktopShell::render_window_decorations`
(`gui/desktop/src/lib.rs:2367`), `window_chrome` (`:1238`), and the
`ManagedWindow` geometry fields that exist only to feed them.


**Correction 2026-08-21 — there are five prerequisites, not two, and the entry
undercounted the deletion as well.** Found by reading the shell's decorator line
by line before deleting it, rather than trusting this entry's own summary of
what it does. Two findings, both the same shape as everything else here:

1. **The shell's decorator honours three more user settings than the
   compositor does.** `render_window_decorations` draws its title bars from
   `DesktopTheme::from_settings` (`gui/desktop/src/lib.rs:745`) and its title
   text at `self.font_size(TextRole::Body)`, so it follows:
   - `theme_mode` — the light palette gives a `0xCCD0DA` title bar, the dark one
     `0x313244`. The compositor's `DecorationTheme` is one hardcoded set of
     twelve Catppuccin Mocha colours, so a user in light mode gets dark title
     bars.
   - `accent_color` + **`accent_titlebars`** — a setting whose *entire subject*
     is window title bars, ignored by the process that draws window title bars.
     `from_settings` paints the focused bar in the accent and picks a readable
     foreground for it; the inactive bar deliberately keeps the base palette.
   - `fonts.ui_size` and `fonts.ui_font` — the compositor's title text is
     `DEFAULT_FONT_SIZE * scale` in `Family::Ui`, a constant and a hardcoded
     family. A user who enlarged the UI font for readability keeps small title
     bars, which is an accessibility setting rather than a cosmetic one.

   So the ordering argument this entry already makes for scaling and corners
   applies unchanged to these three: **deleting first ships a visible
   regression** for anyone in light mode, anyone with an accent colour, and
   anyone who changed the UI font. They are prerequisites 3, 4 and 5.

   The fix is *not* to copy `DesktopTheme::from_settings` into the compositor —
   the light/dark table and the accent derivation would then exist twice, which
   is `TD-THREE-INDEPENDENT-APPEARANCE-MODELS` reappearing in the place this
   entry is trying to remove a duplicate from. It is to resolve the decoration
   colours **in `gui/appearance`**, where the settings live, and have both
   `DesktopTheme` and `DecorationTheme` read that one answer.

2. **`window_chrome` has a second caller this entry does not mention:
   `DesktopShell::hit_test` (`gui/desktop/src/lib.rs:1418`).** The shell does not
   merely *draw* a duplicate title bar; it hit-tests one, resolving clicks into
   `Hit::WindowClose` / `WindowMaximize` / `WindowMinimize` / `WindowTitleBar`
   against button rectangles built from `WINDOW_BUTTON_SIZE = 16.0` while the
   compositor hit-tests the same buttons at `TITLE_BUTTON_SIZE = 20`. That is
   the same drift as the drawing and is invisible for the same reason. So the
   deletion is: the renderer, `window_chrome`, `WindowChrome`, those four `Hit`
   variants and their `handle_mouse` arms.

   What **stays** is `ManagedWindow::frame_rect`, and the line is worth naming
   because it is not arbitrary: the frame rect is the window's own outer
   rectangle, arrives from the compositor in physical pixels and is not scaled
   by the shell — its own doc says so. *Where* a window is may be known to a
   shell (the taskbar and Alt-Tab need to say which window is which); *what its
   title bar looks like* may not. Everything the shell puts through `self.scale`
   here is chrome and goes.

   Roughly fifteen tests in `gui/desktop/src/pointer_tests.rs` sit on the
   deleted surface. Those asserting a *duplicate* (button rects, chrome radii,
   chrome shadows) go with it; those asserting real shell behaviour reached
   *through* a chrome click — `maximizing_and_restoring_returns_the_window_to_
   where_it_was` and its neighbours, which pin `ManagedWindow::restored` — are
   rewritten to call the API directly rather than deleted, because the
   behaviour survives and only its trigger moves.

**Progress 2026-08-21 — prerequisites 3, 4 and 5 are done; only the deletion is
left.** ✅ `gui/appearance` now owns `DecorationColors`: eleven resolved frame
colours with `for_mode(light)` and `from_settings`. Both renderers read it —
`DesktopTheme::window_fields_from` takes the shell's six window fields from it,
and the compositor's `DecorationTheme` is now `from_settings` plus the ARGB
packing, its twelve hardcoded Catppuccin-ish constants gone along with the
`#[allow(dead_code)]` that had been hiding a field nothing read. The
compositor's title font follows `appearance.fonts.ui_size` instead of a
constant 16. See `design-decisions.md` §501.

So a user in light mode, a user with accented title bars and a user who
enlarged the interface font now get the same frame from the compositor that the
shell's duplicate has been drawing all along — which is precisely the condition
this entry set for the duplicate to be removable without shipping a regression.

Two notes for whoever does the deletion:

- **`the_shells_window_colours_are_the_compositors_window_colours`
  (`gui/desktop/src/lib.rs`) is a scaffold, not a keeper.** It exists to fail
  the moment someone recolours a window in `DesktopTheme`, which is the natural
  place to do it and the wrong one. It is meaningful only while the shell has a
  decorator at all; delete it with the decorator.

- **`fonts.ui_font` is still unreachable and is not a sixth prerequisite.**
  `osfont::Family` is `Ui` or `Mono` with no lookup by name, so no renderer in
  this tree honours a font *family* — the shell's decorator does not either, and
  `RenderTree::text` takes no family argument. Deleting the duplicate therefore
  loses nothing on this axis. Tracked as a missing capability rather than a
  missing wire.

What remains for this entry is the deletion itself, exactly as scoped in the
correction above: `render_window_decorations`, `window_chrome`, `WindowChrome`,
the four `Hit::Window{Close,Maximize,Minimize,TitleBar}` variants and their
`handle_mouse` arms, the `gui/desktop/src/main.rs:45` demo caller, and the
`pointer_tests.rs` tests that assert the duplicate. `ManagedWindow::frame_rect`
stays.


**CLOSED 2026-09-13. Both prerequisites and the deletion are done; the entry
had simply not been re-read since the last of them landed.** Verified against
the code rather than the prose above:

| what the entry required | state |
|---|---|
| scale the compositor's decorations off `Display::scale_factor` | done -- `render_title_bar` takes `bar.scale`, and `frame_insets` carries it |
| bring `WindowCorners` across from `AppearanceSettings` | done -- `decoration_radius` is `self.appearance.corner_radius() * scale` |
| delete the shell's `render_window_decorations` and `window_chrome` | done -- neither name exists in `gui/desktop/**` |
| remove the geometry fields that only fed them | done -- no `WINDOW_BUTTON_SIZE` or `TITLE_BAR_HEIGHT` anywhere in the shell |

The shell's public surface is now the nine `render_*` methods a shell should
have -- taskbar, Alt-Tab, start menu, desktop menu, widgets, calendar, OSD, run
dialog -- and not one of them draws a window frame. The compositor is the only
thing that decorates, which is what the module doc always said the policy was.

**Worth noting how it was found, because it is this file's own recurring
failure and this is the fourth instance today.** The entry reads as open: its
last section still says *what is left in this entry is now prerequisite 2 and
the deletion*. That sentence was true when written and each subsequent piece
of work updated the code and a parenthetical, never the conclusion. The other
three today were `TD-NO-APP-CONNECTS-TO-THE-COMPOSITOR` carrying "135 to go"
when the true figure was two, `TD-C-FOUR-APPEARANCE-SETTINGS-...` presenting
three operator-blocked rows as unstarted work, and three entries counted twice
by a triage grep for quoting their own headings. An entry that overstates what
is left costs exactly what an entry that understates it does: it sends the next
reader somewhere that does not need them.
