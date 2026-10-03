## TD-C-THE-SHELL-CAN-DRAW-ITSELF-AND-NOBODY-CAN-ASK-IT-TO — RESOLVED 2026-08-21 (see the final note)

**In short:** `DesktopShell` has five public methods that turn the desktop into
draw commands — the taskbar, the window decorations, the Alt-Tab switcher, the
start menu and now the calendar popup. Nothing outside the crate calls any of
them, and nothing outside the crate *can*: `gui/desktop/Cargo.toml` declares no
`[lib]` target, so `desktop` is a binary and `DesktopShell` is not importable at
all. Every one of those methods is reached only from the crate's own `main()`
demo and its own tests.

**Where:** `gui/desktop/src/main.rs` — `render_taskbar` (:2303),
`render_window_decorations` (:2402), `render_alt_tab` (:2493),
`render_start_menu` (:2564), `render_calendar` (:2948).
`gui/desktop/Cargo.toml` — no `[lib]`.

*(Both paragraphs above are as-written and now partly historical. The `[lib]`
exists — see progress note (3) below — and the file is `src/lib.rs`, not
`src/main.rs`. **There are four render methods, not five:**
`render_window_decorations` was deleted with the shell's duplicate decorator
(`TD-C-THE-DESKTOP-AND-THE-COMPOSITOR-BOTH-DRAW-WINDOW-TITLE-BARS`, resolved),
which is a simplification for this entry rather than a complication: the loop
below can submit everything `DesktopShell` renders, with no carve-out for a
surface the compositor also draws. The remaining four are `render_taskbar`,
`render_alt_tab`, `render_start_menu` and `render_calendar`.)*

**Why this is logged and not fixed (2026-08-21):** it is the render half of
`TD-SHELL-HAS-NOWHERE-TO-SEND-A-LAUNCH`, and it has the same single cause —
the shell has no compositor/IPC event loop yet. That entry covers the outbound
half (a `ShellAction::Launch` nobody carries out); this one covers the inbound
half (a `RenderTree` nobody paints). Fixing either one properly means building
the loop, which is a task, not a cleanup, and adding a `[lib]` target on its own
would produce an importable type with still no importer.

**Why it is worth having written down anyway.** This is the exact defect the
round-4 sweep exists to find — "the tree holds one correct answer that callers
cannot reach, and grows wrong copies of it" — and it is currently invisible,
because none of the five methods trips `dead_code`: they are `pub` on a `pub`
type, which suppresses the lint even though the crate is a binary and the
`pub` therefore reaches nobody. `apps/systray/src/main.rs` already has its own
`render_calendar_popup` and its own `render_power_menu`, which is the copy-
growing half of the pattern starting.

**Proper fix:** the same event loop `TD-SHELL-HAS-NOWHERE-TO-SEND-A-LAUNCH`
waits on. When it lands it composes the five trees in z-order — decorations,
taskbar, then whichever popups are open — and submits the result to the
compositor. At that point `desktop` gains a `[lib]` so the loop can live in a
separate binary and the tests can exercise the composition, and the popup
render methods stop being the only surfaces in the shell whose output has
never been on a screen.

**If never fixed:** the shell remains a very well-tested model of a desktop
that cannot be displayed. Nothing rots — the tests hold the geometry and the
hit testing to each other — but every surface added to it inherits the same
condition, and the systray's parallel implementations keep diverging from the
shell's with nothing to notice that they have.

**Progress 2026-08-21 — one of the two stated blockers was already stale, and
the other now has its first piece.** The entry above says the shell "has no
compositor/IPC event loop yet" and treats that as one obstacle. It is two, and
they were in different states:

- *A transport to run a loop over.* *Already existed when this was written.*
  `gui/compositor/src/server.rs` listens, accepts and paces; `oswindow` really
  connects, blocks and submits; `apps/editor` is a working client end to end
  over the real protocol. The claim was inherited from an earlier entry and not
  rechecked. Recorded here rather than quietly corrected, because it is the same
  failure `design-decisions.md` §305 exists for: a "blocked on X" note that
  outlives X.
- *A way for a shell surface to be a shell surface.* Genuinely missing, and the
  real blocker. The compositor had one flat `z_stack` and every raise went to
  the top of it, so a taskbar would have gone behind the first application
  window the user clicked. **Fixed today:** `Layer::{Background, Normal,
  Overlay}` on `WindowSpec`, a band each window is created in and cannot leave,
  with raising confined to the band. `design-decisions.md` §494; 7 tests, all
  verified to fail when the flat push is reintroduced.

**Still open**, and still the reason this entry is not closed: the shell has no
way to *learn about other windows*. `CompositorRequest` has no window-list query
and no notification, so a taskbar could now stay in front of the windows it is
supposed to list while having no idea what they are. That protocol surface plus
the loop itself are what remain.

**Progress 2026-08-21 (2) — the window-list protocol above now exists, so the
loop is the only thing left.** `guiremote::window_list` adds a `WLST` frame
carrying id, pid, band, title and the visible/minimized/maximized/focused flags
for every window on the desktop; `RequestBody::SubscribeWindowList` turns the
stream on; the compositor pushes a fresh snapshot whenever the encoded list
differs from the bytes it last sent that link; `oswindow` exposes it as
`watch_desktop` / `desktop_windows` / `desktop_revision`. `design-decisions.md`
§495; 26 tests across the three crates, each verified to fail when the
corresponding bug is reintroduced. A taskbar can now be told what to list.

**What remains is exactly one thing: the loop itself.** `desktop` gains a
`[lib]`, a binary opens three `Layer`-banded windows (wallpaper, taskbar,
popups) through `oswindow`, calls `watch_desktop(true)`, and on each
`desktop_revision` change re-renders the four trees and submits them. Both
named blockers are gone; nothing is waiting on another lane.

*(Was "five trees" when written. It is four: the fifth was
`render_window_decorations`, and submitting it would have double-drawn every
title bar in the desktop. It has since been deleted — see
`TD-C-THE-DESKTOP-AND-THE-COMPOSITOR-BOTH-DRAW-WINDOW-TITLE-BARS`, resolved —
so the loop can now render everything the shell has without qualification.)*

**Progress 2026-08-21 (3) — `desktop` is a library.** The `[lib]` above now
exists: `src/main.rs` became `src/lib.rs`, the crate declares both a `[lib]`
and a `[[bin]]`, and the scripted demo that used to *be* the crate is now a
135-line binary beside it. `DesktopShell` is reachable by name from outside for
the first time. Removing the 54 module-level `#[allow(dead_code)]` this
required surfaced 119 concealed warnings and, in them, three live defects and
one very large one — see `TD-C-DEAD-CODE-IS-ALLOWED-WHOLESALE` (progress note)
and `TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE`.

**The one design question the loop still has to answer**, noted here so it is
not discovered halfway through: `DesktopShell.windows` is a
`BTreeMap<WindowId, ManagedWindow>` the shell maintains itself, with ids it
mints itself in `take_window_id` — while `WindowId`'s own doc says the id is
"assigned by compositor". That is two answers to one question, which is the
shape this whole sweep exists to remove. The loop must not paper over it by
keeping both and mapping between them. The shell's map has to become a
*projection* of `desktop_windows()`, retaining only state that is genuinely the
shell's own (virtual-desktop assignment being the real one). Doing that first
is what stops the loop from being written against a model that disagrees with
the compositor.

**Progress 2026-08-21 (4) — the second design question the loop has to answer,
found by reading rather than by writing the loop and hitting it: the shell
hit-tests in screen coordinates, and a client is only ever told window-local
ones.** Three facts, each checked in the code rather than assumed:

- *The shell's rects are screen-space.* `taskbar_rect` (`gui/desktop/src/lib.rs`
  :979) computes its `y` as `(self.screen_height as f32 - height).max(0.0)`, and
  `start_button_rect` (:1001) and `clock_rect` (:2592) both derive from it. On a
  1080-tall screen the start button is at y ≈ 1040.
- *A client is told window-local ones.* `guiremote::InputEvent`'s own doc
  (`gui/remote/src/input.rs`:71-73): "Mouse coordinates inside `event` are
  already window-local, so the client needs no knowledge of where it sits on
  screen to interpret them." The compositor subtracts the client rect's origin
  before it sends (`gui/compositor/src/lib.rs`:4881, and `wire_event` at :2499
  only widens `i32`→`f32`). A press on the start button arrives at y ≈ 8.
- *So a naive loop mis-routes every click it receives.* Feeding the delivered
  coordinates straight into `DesktopShell::hit_test` tests y ≈ 8 against a
  taskbar that believes it starts at y ≈ 1040, falls through every chrome arm,
  and lands on `Hit::Desktop`. The discrepancy is exactly
  `screen_height - taskbar_height`, i.e. it is invisible on a screen the same
  height as the taskbar and grows with the display — the shape of bug that
  passes every test written on a small fixture.

**The fix is an explicit per-surface origin, applied in *both* directions, and
it is symmetric — which is the reason to state it before writing either half.**
The loop places the surfaces, so it alone knows each origin. Input needs
`+origin` on the way in (window-local → screen) before `hit_test` sees it;
rendering needs `−origin` on the way out (screen → window-local) before the
tree is submitted, because `render_taskbar` emits its commands at y ≈ 1040
while the taskbar surface's own buffer starts at 0. Getting one direction right
and the other wrong yields a desktop that draws correctly and responds to
clicks in the wrong place, or vice versa, so the two belong to one abstraction
and one test: a point that hit-tests to a given element must be a point that
element was drawn at.

**Do not fix it by re-basing the shell's rects per surface.** That would make
`taskbar_rect` return `y = 0` and put the shell back to two answers for where
the taskbar is — the thing this entry exists to remove — and it would break the
hit-test ordering in `hit_test`, which relies on all the chrome rects living in
one comparable space (start button before taskbar panel before window, :1313-
1341).

**A related consequence worth deciding before the loop, not during it: two of
`Hit`'s variants become unreachable, and one popup behaviour stops working on
its own.**

- `Hit::WindowContent` and `Hit::Desktop` cannot be produced in production once
  the shell is a client. The compositor routes a press to the topmost window
  whose *client* rect contains it (`Compositor::window_at`, :4974), so a click
  on another application's window is delivered to that application and never
  reaches the shell at all. They stay reachable — and useful — from the shell's
  own tests, which drive `hit_test` directly over a whole screen; but the loop
  must not be written as though it will see them.
- *Click-outside-to-dismiss.* The start menu and the calendar close when the
  user clicks away from them. If each popup is its own small window, the click
  that should dismiss it lands on somebody else's window and the shell is never
  told, so the menu stays open under the window the user just clicked. The
  answer is the one every real desktop uses: while a popup is open, the shell's
  `Overlay` surface covers the whole screen and is the popup's dismiss layer,
  and is unmapped when no popup is open so ordinary clicks pass through. That
  makes the overlay's origin `(0, 0)` — screen space, no translation — which is
  why the offset abstraction above must be per-surface rather than one global
  constant.

**Progress 2026-08-21 (5) — the shell can now ask, and the taskbar asks.** Two
of the three things this entry says are missing are done; the loop itself is
not.

- **A shell can act on a window it does not own.** `ShellControl { window,
  action }` (`gui/remote/src/control.rs`) with five actions — `Activate`,
  `Minimize`, `Restore`, `Maximize`, `Close` — reaching
  `Compositor::activate_window` / `minimize_window` / `restore_window` /
  `maximize_window` / `request_close`. This was the hard blocker: *every* other
  window request is resolved against the sending connection's own windows
  (`ClientLink::resolve`), which is the ownership model and refuses every
  legitimate taskbar click. Deliberately excludes move/resize — placing windows
  is the compositor's, and a shell that could move any window would be the
  second window manager this entry exists to remove.
  - `Activate` is one operation, not restore-then-focus: `focus_window` refuses
    a minimised window on purpose, so un-minimising has to come first. It is
    also *not* `restore_window`, which un-maximises too — a window minimised
    while maximised has to come back maximised.
  - `Close` asks rather than destroys, pushing the same
    `EventNotification::WindowClose` the title-bar button does.
  - Privilege routes through the new `ClientLink::require_shell`, which checks
    nothing and says so. See `TD-C-ANY-CLIENT-CAN-READ-EVERY-WINDOW-TITLE`
    below — that gate is still open, and now has two callers instead of one,
    which is exactly why they were routed through a single function.

- **The shell's window map is a projection, and the taskbar emits intents.**
  `DesktopShell::apply_window_list(&[WindowInfo])` (`gui/desktop/src/lib.rs`) is
  now the authority on what exists; `ShellAction::Control { window, action }` is
  what a taskbar click produces. The click no longer mutates anything. Three
  things `apply_window_list` decides, each with a test:
  - It **replaces** rather than merges — the list is the whole truth, and a
    merge leaves a button for a program that has exited.
  - It drops everything outside `Layer::Normal`, so the taskbar does not list
    itself, the wallpaper and its own start menu.
  - A projected window's geometry is **zero, and that is not a placeholder**:
    `WindowInfo` carries none because the shell does not place windows, so
    there is nothing truthful to put there. The only reader would be
    `Hit::WindowContent`, which a live session never reaches (see above), and a
    zero rectangle matches nothing — the right answer to a question never asked.

**Still missing, and now the whole of what is left here:** the event loop. The
three `Layer`-banded surfaces (wallpaper/`Background`, taskbar/`Overlay`,
popups/full-screen `Overlay`), `watch_desktop(true)`, a re-render of the four
trees on each `desktop_revision` change, and the per-surface origin translation
in both directions described above. Also still open: `DesktopShell` retains
`add_window`, `focus_window`, `minimize_window`, `snap_window*` and
`toggle_maximize` — the geometry half of the old private window manager. They
are now used only by the demo and the tests, which have no compositor to be told
by, but they are a second answer to questions the compositor already answers and
should go when the loop lands. `main.rs` demonstrates the honest path in its
`-- taskbar --` section and builds a *fresh* `DesktopShell` to do it, precisely
because ids from the two regimes must not be mixed.

**RESOLVED 2026-08-21 — the loop exists.** `gui/desktop/src/session.rs`,
`ShellSession<T: ConnectionTransport>`: it opens the three banded surfaces,
subscribes to the window list, pumps input, and submits every tree the shell
renders. The four render methods now have a caller that is not the demo, and a
taskbar click reaches the compositor as a `ShellControl` request. 21 tests in
`session/tests.rs` run it end to end against `oswindow::testing::desktop()` —
no compositor, no display; **all 16 reintroducible defects listed below were
reintroduced one at a time and each produced a deterministic failure naming its
own test.** Full crate suite: 2493 pass.

The design questions this entry raised before the loop was written were all
answered as the entry specified, which is the argument for having written them
down first — none was discovered mid-build:

- *Per-surface origin, both directions, one abstraction.* `Surface { window,
  origin }` with exactly two methods: `to_screen` (input, `+origin`) and
  `localize` (output, `−origin`). They are the only two places either
  translation is written. The entry's own acceptance test —
  "a point that hit-tests to a given element must be a point that element was
  drawn at" — is `a_point_that_hits_an_element_is_a_point_that_element_was_
  drawn_at`, and it was verified to catch *both* asymmetric bugs
  (`render-translated-the-wrong-way` and `input-translated-the-wrong-way`),
  which is the failure an ordinary suite would miss.
- *The shell's rects were not re-based.* `taskbar_rect` still answers in screen
  space and `hit_test`'s ordering still compares popup rects against it.
- *The overlay is full-screen and unmapped while nothing is open*, exactly as
  the click-outside-to-dismiss paragraph above requires. `popups_shown` starts
  `true` because the compositor maps a new window, so the first `paint_chrome`
  is what performs the initial unmap.
- *`Hit::WindowContent` / `Hit::Desktop` are not special-cased.* The loop never
  sees them and does not pretend to.

Two implementation choices the entry did not anticipate, both documented at
their definitions:

- **The output translation is `PushTranslate`/`PopTranslate`, not a coordinate
  rewrite.** `RenderCommand` has a dozen variants carrying positions in
  different shapes; a rewriter would need extending for each new one and would
  silently draw in the wrong place until somebody noticed. It is applied
  unconditionally, even for origin `(0,0)`, because a "skip when it is a no-op"
  branch would send the identity surfaces down a path the translated one never
  takes — which is how the two directions get to disagree unnoticed.
  `a_surface_at_the_screens_origin_is_translated_the_same_way_as_any_other`
  holds that.
- **Input is dispatched before the new window list is folded in.**
  `EventLoop::poll` is also what reads the list off the wire, so by dispatch
  time the connection may already hold a newer desktop than the taskbar the
  user clicked was drawn from. Folding it in first renumbers
  `visible_windows()` underneath a click aimed at the old numbering — the click
  would minimise whichever window inherited the slot.
  `a_window_list_arriving_with_a_click_is_folded_in_after_it`.

**Still open, and now tracked only in its own entries, not here:**

- `DesktopShell::add_window`, `focus_window`, `minimize_window`, `snap_window*`,
  `toggle_maximize` and `take_window_id` — the geometry half of the old private
  window manager — survive, now used *only* by the demo and by tests. The loop
  has landed, so the condition this entry attached to their removal is met and
  they should go next.
- `ShellAction::Launch` is queued by the session and handed back from
  `take_launches()`; nothing starts a process. That is
  `TD-SHELL-HAS-NOWHERE-TO-SEND-A-LAUNCH`, which is narrowed rather than closed
  — see its own progress note.
- `require_shell` still checks nothing, so the subscription the loop depends on
  is open to any client: `TD-C-ANY-CLIENT-CAN-READ-EVERY-WINDOW-TITLE`, below.
