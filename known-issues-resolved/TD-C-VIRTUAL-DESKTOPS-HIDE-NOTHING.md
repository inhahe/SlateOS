## TD-C-VIRTUAL-DESKTOPS-HIDE-NOTHING (lane C, 2026-08-21) — RESOLVED 2026-08-21 (see the final note)

**What:** switching virtual desktop changes which windows the *taskbar lists*
and nothing else. On a live session the windows of the desktop you just left
stay on screen, because the compositor has never heard of virtual desktops and
the shell has no verb that would tell it to unmap anything.

**Where:** `gui/desktop/src/lib.rs` — `DesktopShell::switch_desktop` sets
`current_desktop`, and `visible_windows()` filters on it. That filter is the
entire implementation. `gui/remote/src/control.rs` `ShellControlAction` has no
hide/show-without-minimizing action, and `RequestBody` has nothing that would
carry a desktop assignment.

**How it looks:** open a window, press Ctrl+Super+Right. The taskbar empties and
`switch_desktop` asks for the topmost window on the new desktop to be raised —
correct as far as it goes — but the window from desktop 1 is still drawn, still
takes clicks, and is not reachable from the taskbar any more. Worse than not
having the feature.

**Why it was not noticed sooner:** every test drives `DesktopShell` with no
compositor, where "visible" means "in `visible_windows()`" and the two notions
of visible cannot be told apart. It became a real defect at the same moment the
keyboard shortcuts did — when `ShellSession` gave the shell a live connection.

**Proper fix, and the fork in it:** either (a) the compositor learns about
workspaces, holding the assignment itself and unmapping a whole set at once,
which is what wlroots-based compositors do and what makes switching one
atomic recomposite rather than N requests; or (b) the shell keeps the
assignment and gets a `SetVisible`-for-a-window-it-does-not-own verb, which is
cheaper but hands a shell the power to hide any client's window — the same
ambient authority the whole `ShellControl` design exists to avoid. (a) is
almost certainly right, and it is a compositor change, so it belongs with the
rest of the compositor's window-state work rather than here. Not filed as an
open question yet because nothing is blocked on the answer: the feature is
incomplete, not wrong-when-used.

**If never fixed:** the virtual-desktop shortcuts and the desktop indicator are
worse than absent — they promise a switch and deliver a taskbar that disagrees
with the screen.

**RESOLVED 2026-08-21, by option (a) — the compositor learned what a virtual
desktop is.** Two commits, and the fork above was decided the way the entry
itself predicted, for the reason it gave: option (b) needed a
hide-any-window-you-do-not-own verb, and that is the ambient authority the
whole `ShellControl` design exists to refuse. Full reasoning in
`design-decisions.md` section 518.

| Stage | Commit | What landed |
|---|---|---|
| 1 | `a86ff739` | The compositor holds it. `Window::workspace`, `Compositor::current_workspace`, `switch_workspace`, `set_window_workspace`; one `shows_on_current_workspace` predicate consulted by the full-scene pass, the per-window draw, the damage path, hit testing, focus and activation. `Layer` is the stickiness rule — a window outside `Layer::Normal` is on every desktop, so panels and menus never vanish. |
| 2 | `ed8d5eea2` | It reaches a screen. `WLST` went to version 2 with a `showing` field in the header and a `workspace` on every window; `RequestBody::SwitchWorkspace` (`0x12`) and `SetWindowWorkspace` (`0x13`) behind `require_shell()`; `Connection`/`EventLoop` accessors that return `Option` so "not told yet" cannot be read as "desktop 0"; and a `DesktopShell` that asks instead of deciding. |

**The shell's three second copies are gone.** `current_desktop` and the
per-window `desktop` are still fields, but nothing writes them except
`apply_window_list`, which takes the whole `WindowList` and copies the
compositor's answers in. `switch_desktop` and `move_window_to_desktop` became
`&self` and return a `ShellRequest` — a new enum, because a desktop switch
names no window at all and so could not honestly be carried in
`ShellControlAction`'s `(window, action)` pair. The cost is one round trip
before the taskbar relabels; the alternative draws the new desktop's buttons
over the old desktop's windows for a frame, which is this bug in miniature.

**Two stage-1 tests were passing for the wrong reason, and reintroducing the
defects is what found them** — not reading.
`a_window_on_another_virtual_desktop_does_not_occlude_the_one_in_front_of_you`
survived a deliberately broken occlusion cull because `Buffer::is_opaque()` is
a question about the pixel *format*, so an `Argb8888` buffer full of opaque
alpha is not an occluder and the hidden window could never have occluded
anything either way. Fixing that was still not enough: `focus_window` ends in
`raise_within_layer`, so moving a window elsewhere hands the keyboard to the
window below it and *reorders the stack* — the window left showing climbs above
the one just hidden, and a hidden window at the bottom can neither occlude nor
overpaint. `stack_with_a_hidden_window_on_top` gives the focus handoff a third
window to land on, which is the ordinary arrangement rather than a contrivance.

**Still open, and named in section 518 as what this deliberately does not do:**
no per-desktop wallpaper, no per-desktop window arrangement, no memory of which
window was focused on a desktop you return to, no drag-a-window-to-a-desktop,
and the number is not persisted across a compositor restart. All of those want
a per-desktop record rather than a per-window number. The desktop *count* is
still fixed at `DesktopShell` construction, which is what blocks the overview
screen's "add a desktop" — see `TD-C-THE-OVERVIEW-SCREEN-IS-1856-LINES-NOBODY-CALLS`.
