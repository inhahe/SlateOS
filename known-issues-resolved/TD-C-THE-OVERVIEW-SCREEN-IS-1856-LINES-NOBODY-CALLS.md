## TD-C-THE-OVERVIEW-SCREEN-IS-1856-LINES-NOBODY-CALLS (lane C, 2026-08-21) — RESOLVED 2026-08-22

**In short:** The desktop has a finished Expose / Mission-Control screen —
thumbnails of every window, one lane per virtual desktop, arrow-key navigation,
type-to-search, click to switch. It is 1856 lines, it is tested, and *nothing in
the operating system ever shows it*. There is no key that opens it, no code that
fills it with real windows, and no code that acts on what the user clicks.

**Where:** `gui/desktop/src/overview.rs`, declared `pub mod overview;` at
`gui/desktop/src/lib.rs:109`. A search of the whole tree for `OverviewState`,
`DesktopLane` and `overview::` matches exactly one file — `overview.rs` itself.
Its only constructors of `DesktopLane` are its own `sample_lanes()` fixture and
three tests.

**How it looks:** it does not look like anything. That is the defect. A user
cannot reach it; a developer reading `roadmap.md` would tick the feature off.

**Why it was not noticed sooner:** the module is *good* — 15 public items with
doc comments, a grid layout, a lane layout, a full render pass and a keyboard
model, all covered by tests that pass. Every test drives the module directly, so
"is this wired to anything?" is a question none of them can ask. This is the
same failure mode as `TD-C-VIRTUAL-DESKTOPS-HIDE-NOTHING` — a self-consistent
component whose tests prove it correct with respect to inputs no one supplies.

**Three things are missing, and they are not the same size.**

1. *A data source. The wire change it needed is now done (`WLST` v3,
   design-decisions.md §519); the projection is not.* `WindowThumbnail` wants
   `window_id`, `desktop_id`, `title`, focus, minimized and a rectangle. All of
   those are now on `guiremote::window_list::WindowInfo`, with
   `DesktopLane::is_current` being the header's `current_workspace`. The
   rectangle was the blocker and is why this entry originally said the wire had
   to change first: `compute_grid_layout` calls
   `fit_aspect(thumb.width, thumb.height, …)`, and `fit_aspect` returns
   `(0.0, 0.0)` for a zero input, so a projection written against the v2 list
   would have given every thumbnail a zero-by-zero rectangle — nothing drawn,
   and `on_mouse_click` hit-testing against nothing. That is the exact failure
   `Hit::WindowContent` had before §506 deleted it, for the exact same reason: a
   rectangle nobody supplies. Uniform boxes were rejected as the substitute — an
   Exposé whose thumbnails do not have their windows' proportions is a list,
   which is the taskbar. What remains is the projection itself: a
   `DesktopShell` → `OverviewState` function driven by the same `WindowList`
   that `apply_window_list` already takes, so the overview cannot disagree with
   the taskbar about which desktop is showing.
   The *thumbnail image* is separately absent and is the part that should stay
   absent for now: the module lays out rectangles and draws titles, and never
   asks the compositor for window contents. Live thumbnails need a verb that
   does not exist (a scaled read of another client's buffer) and is a capability
   question of the same shape as `TD-C-ANY-CLIENT-CAN-READ-EVERY-WINDOW-TITLE` —
   only worse, because a title is text and a thumbnail is the screen. Titled
   proportional rectangles are a usable Exposé without it.
2. *An output path.* `OverviewAction` is a fourth spelling of verbs the shell
   already has: `SwitchToWindow(u64)` is `ShellControlAction::Activate`,
   `SwitchToDesktop(u32)` is `ShellRequest::SwitchDesktop`, `CloseWindow(u64)`
   is `ShellControlAction::Close`. Wiring should *delete* those three variants
   and return `Option<ShellRequest>`, exactly as the zone-snapping work deleted
   the shell's second copy of the edge rules (see
   `TD-C-EDGE-DRAG-TILING-HAS-NO-DRAG-TO-FIRE-ON`, resolved). A second spelling
   is a second thing to drift.
   `OverviewAction::AddDesktop` has no counterpart and cannot get one today:
   `DesktopShell::num_desktops` is fixed at construction and is the only bound
   either side enforces (design-decisions.md §518). Making the desktop count
   mutable is a real decision, not a wiring detail.
3. *A field nothing in the system can fill.* `WindowThumbnail::app_name` is
   rendered as a second label under each title (`overview.rs:757`) and is one of
   the two things type-to-search matches (`overview.rs:183`). Nothing in the
   shell knows it. `guiremote::window_list::WindowInfo` carries `id`, `pid`,
   `layer`, `title`, four state bits, a desktop number and (as of `WLST` v3) a
   rectangle — no application identity; a tree-wide search finds `app_name` only
   in `context_ext.rs`, `focus_assist.rs` and `notif_pane.rs`, all of which are
   given the string by whoever registers with them, and none of which is
   reachable from a window id. Only the three test fixtures inside `overview.rs`
   ever set it, always to the literal `"app"`, which is exactly why the tests do
   not notice. A projection written today would have to pass the empty string,
   and then the overview would render a blank line under every title and
   type-to-search would silently match half of what its doc comment promises.
   The honest fix is to *delete* the field rather than invent a value for it:
   an application's name is a property of the program, which means it comes from
   the package/desktop-entry metadata keyed by executable, and the compositor
   would have to learn an app identity per window before the wire could carry
   one. Until that exists, a titled rectangle is the whole of what the shell
   knows. (Deleting it also removes the second search key, so `update_search`'s
   doc comment has to stop promising it.)

**Proper fix:** (a) a `DesktopShell` → `OverviewState` projection driven by the
same `WindowList` `apply_window_list` already takes, so the overview cannot
disagree with the taskbar about which desktop is showing; (b) `OverviewAction`
collapsed into `Option<ShellRequest>` with the three duplicate variants deleted;
(c) a hotkey in `handle_hotkey` to open it and `Esc` to close; (d) thumbnails
left as titled rectangles until the compositor can serve window contents under a
capability, with the module doc saying so rather than implying pictures;
(e) `app_name` deleted from `WindowThumbnail` and `ThumbnailLayout`, and
`update_search` reduced to matching titles, until an application identity exists
to put there.

**If never fixed:** 1856 lines of dead weight that reads as a shipped feature.
The concrete cost is not the disk space — it is that the next person to want an
Expose screen finds this one, believes it works, and wires a data source to an
action enum that has since drifted from the shell's.

**RESOLVED 2026-08-22, in two stages.** Stage 1 (commit `7438f13ff`) put each
window's rectangle on the `WLST` wire at v3, which was the blocker named in
point 1; see `design-decisions.md` §519. Stage 2 connected the overview to it;
see §520. Every lettered step of the proper fix above is done:

- **(a) the projection** — `OverviewState::apply_window_list(list, num_desktops)`
  is called from the same `DesktopShell::apply_window_list` that refreshes the
  taskbar, from the one statement, so the two cannot disagree about which
  desktop is showing. Lanes come from `num_desktops`, not from the windows that
  happen to exist, so an empty desktop still has a lane — the screen whose job
  is to show you where everything is must show the empty places too.
- **(b) the output path** — `SwitchToWindow`, `SwitchToDesktop` and
  `CloseWindow` are deleted; `OverviewAction::Request(ShellRequest)` carries the
  shell's own vocabulary, and `DesktopShell::act_on_overview` is the only thing
  that reads it.
- **(c) the hotkey** — `Super+Tab` opens it (`DesktopAction::ToggleOverview`),
  `Esc` closes it, and while it is up it is *modal*: `handle_press` routes to
  `press_on_overview` before `hit_test`, and `handle_hotkey` routes to
  `key_on_overview` before the shortcut table. Both orderings are load-bearing
  and both are guarded by a reintroduction marker — see §520's Verification
  table. Without the first, a click on the strip the taskbar occupies reaches
  the taskbar from behind an opaque fullscreen overlay; without the second,
  typing `d` into the search box runs Show Desktop.
- **(d) thumbnails stay titled rectangles** — the module doc says so, and says
  why (a scaled read of another client's buffer is a capability question of the
  same shape as `TD-C-ANY-CLIENT-CAN-READ-EVERY-WINDOW-TITLE`, only worse).
- **(e) `app_name` deleted** from `WindowThumbnail` and `ThumbnailLayout`;
  `update_search` matches titles and its doc comment no longer promises more.
  This was not cosmetic: the empty string a projection would have had to pass
  is a substring of *every* query, so search would have silently returned the
  whole desktop. The test that used to prove the field worked
  (`test_search_filters_by_app_name`) proved it against a fixture that set
  `"app"` by hand, which is exactly why nothing noticed.

**`AddDesktop` is resolved by deletion, not by making the desktop count
mutable.** The entry above called that "a real decision, not a wiring detail",
and it still is — but the "+" button did not have to wait for it. A control
that asks for something no verb in the system can do is the *same defect this
whole entry is about*, one button large: it draws, it hit-tests, and it reaches
nothing. It is gone, and with it `on_mouse_click`'s `screen_w`/`screen_h`
parameters, since it was the only thing in the overview placed independently of
the layout pass and therefore the only thing that could be drawn in one place
and clicked in another. Two tests were inverted to keep it deleted
(`nothing_draws_an_add_desktop_button`, `the_bottom_right_corner_is_not_a_button`).
If the desktop count ever becomes mutable, the button comes back with a verb
behind it.

**Wiring it up exposed a second dead feature, now filed separately.** The
overview's open animation had never run and could not: see
`TD-C-THE-SHELL-HAS-NO-FRAME-CLOCK`. Because every draw path began
`if progress <= 0.0 { return }` and `show()` set it to `0.0`, the first working
build of this feature drew a blank, un-clickable overlay. That is worth
recording as the general lesson of this entry: a component tested only against
its own inputs can be wrong in a way no amount of its own tests can see, and
the failure surfaces the moment — and only the moment — something real is
plugged into it.
