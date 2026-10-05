## 518. A virtual desktop is a number on a window that the compositor reads, not a filter the taskbar applies

**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** the desktop had four "virtual desktops" — four separate screenfuls
of windows you switch between with a button on the taskbar — and switching one
changed nothing except which buttons the taskbar drew. The windows of the
desktop you left stayed on the glass, kept swallowing clicks, and could no
longer be reached from the taskbar, because the taskbar was now showing a
different desktop's list. That is worse than not having the feature at all: it
takes windows away from the user and gives back no way to get them. This
section decides that the *compositor* — the program that decides which pixels
are on the screen — is the thing that has to know about desktops, and that the
taskbar's job is only to ask it.

**Two ways to do it, and only one of them is safe.** The shell (the taskbar and
its menus) already knew which desktop each window was filed on; it just had no
power to act on it. So the cheap fix is to keep the filing where it is and give
the shell a new request — "hide that window" — that it sends for each window
belonging to a desktop being left. The cost is not the request count. It is
that such a verb necessarily lets *any* shell hide *any* client's window, which
is precisely the ambient authority (power you have merely by being who you are,
rather than by holding a token for it) that the whole `ShellControl` design
exists to refuse. A window's visibility would become something a program other
than its owner can take away, permanently, with no record of who did it. The
alternative — the compositor holds each window's desktop number — needs a wire
field and two new verbs, and in exchange the shell's power stays exactly what
it already was: it can ask which desktop is showing and ask for a different one.
It cannot reach into a window. That is the one chosen.

There is a second, quieter reason. Once the compositor holds the number, a
switch is *one* recomposite computed from a consistent picture. With the shell
driving it, a switch is N requests that arrive one at a time, and the desktop is
briefly a mixture of two of them — every intermediate state visible, and a
failure halfway through leaving a permanent mixture that nothing detects.

**One predicate, not twelve spellings.** Before this, "on screen" was written
out by hand as `visible && !minimized` at twelve places: the panel-reservation
scan, the direct-scanout candidate, focusing, hit-testing against the client
area, hit-testing against the decorated frame, the cursor-shape lookup, the
opaque-occluder test, the full-scene draw, the damage draw, the per-window draw,
the screen-capture stream, and the topmost-window focus search. A *third* reason
a window is not on screen, added to twelve sites independently, is exactly how a
window ends up invisible but still clickable — which is the bug being fixed
here, so repeating its shape would have been perverse. All twelve now call
`Window::is_showing(current_workspace)`. The next reason a window is hidden goes
in one place, and there are six markers below (`wsoccludesanyway`,
`wshittestsanyway`, `wsrenderallanyway`, `wsrenderdamagedanyway`,
`wsrenderwindowanyway`, `wsscanoutanyway`) that exist purely to prove a
*partially* converted compositor is caught — because the half-converted state is
the one that ships by accident.

**The layer is the stickiness rule.** A window outside `Layer::Normal` — the
wallpaper below everything, the taskbar and panels above it — is on every
desktop. This is not a special case bolted on; it is the same distinction the
shell already draws when it leaves non-`Normal` windows out of its taskbar.
Those surfaces are furniture belonging to the *screen*, not documents belonging
to a desktop. The alternative, a `sticky` flag someone has to remember to set,
has an obvious failure mode: a taskbar that vanishes on a switch takes with it
the only control that could switch back. Reusing `Layer` means the property
cannot be forgotten, because it is a consequence of what the surface already
declared itself to be.

**The compositor owns *which* desktop shows; the shell owns *how many* there
are.** The compositor deliberately has no idea that four is the number, and
`switch_workspace` has no upper bound to check against. A count is a user
preference and belongs to whatever is offering the user the choice; a second
copy in the compositor would be a second answer, and the two would drift the
first time the preference changed. What the compositor owns is the part that
decides pixels.

**Activating follows the window; it does not drag it here.** Clicking a taskbar
button for a window on another desktop switches to that desktop. The other
reading — bring the window to me — is defensible and some desktops do it, but it
is not undoable by the same gesture that caused it: switching moves exactly one
thing (which desktop is showing) and switching back restores it, whereas
dragging rearranges the desktops themselves and leaves the user to notice the
damage and repair it by hand. The rule extends to `activate_window` generally:
un-minimizing and switching desktop are the same act — undo whatever is hiding
this window — done in one call, because a shell should not have to know *which*
kind of hiding it is undoing.

**Hiding is only hiding.** A window on a desktop that is not showing is not
minimized, not unmapped, not moved, and its client is not told. The temptation
is to reuse minimize, since the machinery exists — but minimize is a *user's*
statement about a window, one they can see in the taskbar and undo; borrowing it
for a switch would silently overwrite that statement, and the windows would come
back from the other desktop minimized. From the client's side nothing has
happened that it could act on: the user looked away. The test named
`a_window_comes_back_from_another_desktop_exactly_as_it_was_left` is what holds
this line, and it fails the moment the switch is implemented by minimizing.

**Focus follows the screen.** A window nobody can see must not hold the
keyboard — that is every keystroke disappearing into a window the user cannot
find. Both `switch_workspace` and `set_window_workspace` therefore check whether
the focused window is still showing, and if it is not, drop focus, tell the
client it lost focus, and hand the keyboard to the topmost window that *is*
showing — or to nothing at all, on an empty desktop. `focus_window` itself now
refuses a window that is not showing, for the same reason it already refused a
minimized one.

**A window's desktop is not in its spec.** A new window is put on whichever
desktop is showing, and a client cannot ask for another. Choosing which desktop
a program appears on is the user's business, and a client that could pick would
be able to open a window somewhere the user is not looking — which is either a
window that seems not to have opened, or a place to hide one.

**An assignment aimed at a panel is stored and ignored, not refused.** Setting a
non-`Normal` window's desktop succeeds, records the number, and changes nothing
about what is drawn. Refusing it would make every shell that moves "all of my
windows" first work out which of them are furniture — pushing the stickiness
rule out of the compositor and into every caller, which is the opposite of the
point.

**What this does not do.** The compositor holds no per-desktop state beyond the
number on each window: no per-desktop wallpaper, no per-desktop window
arrangement, no memory of which window was focused on a desktop you return to
(focus goes to the topmost showing window, which is usually but not always the
same thing). Nothing yet moves a window to a desktop by dragging it. And the
number is not persisted across a compositor restart. All of those want a
per-desktop record rather than a per-window number, which is a larger design
than this one.

### The wire: a desktop number per window, *and* one in the header

The window list (`WLST`) went to version 2 and gained two things: a `showing`
field in the header, and a `workspace` on every window. Only the second is
obviously needed, and the first looks redundant — a shell could infer which
desktop is up by finding a `Layer::Normal` window that is `visible`. It cannot,
for two reasons, and both of them are ordinary rather than exotic:

- **An empty desktop has no window to infer it from.** Switch to a desktop
  nobody has opened anything on and the list is either empty or entirely windows
  filed elsewhere. A taskbar that has to guess would show the previous desktop's
  number until the user opened something.
- **The compositor switches desktops on its own account.** Activating a window
  that is filed elsewhere *is* a switch (see "Activating follows the window"
  above), so a shell that remembered the number it last asked for would list one
  desktop's windows over another desktop's screen — which is a smaller version of
  the very bug being fixed. Both `Connection::current_workspace` and
  `EventLoop::current_desktop` are documented "read this, do not remember it",
  and both return `Option` so that "not told yet" cannot be confused with
  "desktop 0".

The field order was chosen to leave the existing decoder tests' byte offsets
alone: `workspace` goes *after* the state byte, so `state_at = HEADER + 8 + 8 + 1`
and `layer_at = HEADER + 8 + 8` still name the fields they were written for.
That is not laziness — those tests exist to catch a field being written in the
wrong place, and rewriting their arithmetic in the same change that moves the
fields would have disarmed them for exactly one commit.

### Two new request verbs, not a new `ShellControlAction`

`ShellControlAction` is a byte in a `(window, action)` pair, and the obvious
cheap move was to add `SwitchDesktop(n)` to it. It does not fit, twice over:

- **A switch names no window.** Sending it against an arbitrary window makes the
  wire lie about what the request is aimed at, and leaves "which window?"
  unanswerable on an empty desktop — the one case that most needs to work.
- **`ShellControlAction` has no room.** Its `ZONE_BYTE_BASE = 7` opens an
  unbounded tail for zone slots, so there is no free byte to take.

So `RequestBody` gained `SwitchWorkspace { workspace }` (tag `0x12`) and
`SetWindowWorkspace { window, workspace }` (tag `0x13`), both behind
`require_shell()` — the same seam as reading the window list, for the same
reason: they act on windows the sender does not own. (`require_shell()` still
refuses nobody; it is a named place for a capability check that needs kernel
attestation. See §495 and `TD-C-ANY-CLIENT-CAN-READ-EVERY-WINDOW-TITLE`.)

### The shell asks; it no longer decides

This is the half that made the compositor's new knowledge reach a screen.
`DesktopShell` kept three pieces of state that were now second copies:
`current_desktop`, a per-window `desktop`, and the arithmetic that moved them.
All three are gone in the sense that mattered — the fields remain, but nothing
writes them except `apply_window_list`, which takes the whole `WindowList` and
copies the compositor's answers in.

- `switch_desktop` and `move_window_to_desktop` became `&self` and return a
  `ShellRequest` instead of mutating. They used to assign `current_desktop` and
  return an `Activate` for the topmost window on the desktop being arrived at,
  which was as far as a virtual desktop ever got: the taskbar relabelled itself,
  the windows of the desktop being left stayed on screen, and the window it
  raised was raised *over* them.
- `HotkeyOutcome::requests` and `ShellAction::Control` carry `ShellRequest`
  rather than `WindowRequest`. One shape for everything the shell asks for,
  because the alternative is two shapes and two places to forget a verb.
- What the shell kept is the *count* of desktops, which is the only bound either
  side enforces, and the per-window icon, which the compositor has never heard
  of.

The cost of not being optimistic is one round trip before the taskbar changes.
That is the right trade even ignoring correctness: the alternative shows the new
desktop's buttons over the old desktop's windows for a frame, which is the
original bug in miniature and would be indistinguishable from it if the request
were ever refused.

### Verification

The whole point of the exercise is that a test which passes is not yet evidence.
Every test named below was proved by putting its defect back with
`D:/tmp/reintro.py` — a guarded, reversible one-string patcher that refuses to
apply unless the string occurs exactly once, and verifies the file is restored
byte-for-byte by SHA-256 afterwards. `git checkout` was never used, because it
would discard unrelated uncommitted work.

**Two tests were passing for the wrong reason, and both were found this way
rather than by reading.**

- `a_window_on_another_virtual_desktop_does_not_occlude_the_one_in_front_of_you`
  survived a deliberately broken occlusion cull, because `Buffer::is_opaque()`
  is a question about the pixel *format* and not about the bytes: an
  `Argb8888` buffer full of `0xFF` alpha is not an occluder, so the hidden
  window could never have occluded anything whether the cull worked or not.
- Then *that* fix was still not enough, and the reason was subtler:
  `focus_window` ends by calling `raise_within_layer`. Moving a window to
  another desktop hands the keyboard to the window below it, so the focus
  handoff *reorders the stack* — the window left showing climbs above the one
  just hidden. A hidden window at the bottom of the stack can neither occlude
  nor overpaint, so both the occlusion test and the damage-path test were
  exercising nothing. `stack_with_a_hidden_window_on_top` fixes it by giving the
  handoff a third window elsewhere on screen to land on, which is also the
  ordinary arrangement rather than a contrivance.

**Some guards are deliberately redundant, and single markers cannot prove them.**
The full-scene pass culls a hidden window and the per-window draw refuses it
again, so a defect at exactly one of those sites is invisible. Three of the
stage-1 markers therefore only fire in combination:

| Markers | Tests that fail |
|---|---|
| `wsrenderallanyway` + `wsrenderwindowanyway` | `…is_not_drawn`, `…does_not_occlude…` |
| `wsrenderdamagedanyway` + `wsrenderwindowanyway` | `the_damage_path_does_not_repaint_a_window_from_another_desktop` |

That redundancy is kept rather than removed: two passes that each independently
refuse to draw a hidden window is the correct belt-and-braces for the one thing
this whole section exists to guarantee. The price is that proving it needs
combinations, and the markers record that price honestly.

**Sixteen stage-2 markers, and the prover rebuilt all four crates for each one.**
That is slow — the whole GUI stack per marker — and it is the only honest way to
watch a defect in `guiremote` surface as a failure in `desktop`. It did: the two
encoder markers each fail tests in three different crates, which a per-crate
prover would have scored as a `guiremote` problem and stopped there.

| Marker — what it puts back | Tests that fail |
|---|---|
| `wl2noheader` — the header's desktop is never written | `an_empty_desktop_still_says_which_desktop_it_is`, `a_window_keeps_its_own_desktop_number_when_it_is_not_the_one_showing`, `the_desktop_being_shown_comes_from_the_compositor_and_not_from_memory`, `the_list_says_which_desktop_is_showing_after_a_switch_the_shell_did_not_ask_for` |
| `wl2nowindowdesktop` — a window's own desktop is dropped | `a_shell_moves_a_window_to_another_desktop_and_the_list_says_so`, `a_window_keeps_its_own_desktop_number…`, `the_desktop_being_shown…` |
| `wl2shortheader` — the header is measured four bytes short | *no stage-2 test; see below* |
| `ctlswapworkspaceverbs` — the two verbs decode to each other | `both_workspace_requests_survive_the_wire`, `switching_to_the_last_desktop_a_u32_can_name_is_not_clamped`, `switching_desktop_off_the_wire_reaches_the_compositor`, `a_shell_moves_a_window…`, `the_list_says_which_desktop_is_showing…` |
| `compnoshowing` — the compositor always reports desktop 0 showing | `the_list_says_which_desktop_is_showing_after_a_switch_the_shell_did_not_ask_for` |
| `compnowindowdesktop` — every window is reported on desktop 0 | `a_shell_moves_a_window_to_another_desktop_and_the_list_says_so` |
| `wireswitchdropped` — a switch is answered yes and discarded | `switching_desktop_off_the_wire_reaches_the_compositor` |
| `wiremovedropped` — a move is answered yes and discarded | `a_shell_moves_a_window…`, `the_list_says_which_desktop_is_showing…` |
| `clientforgetsshowing` — the client remembers 0 rather than reading | `the_desktop_being_shown_comes_from_the_compositor_and_not_from_memory` |
| `shellkeepsitsown` — the shell ignores the header | `desktop_navigation_stops_at_both_ends`, `the_desktop_indicator_counts_from_one`, `a_window_is_only_visible_on_its_own_desktop`, `switching_desktop_names_no_window` |
| `shelllocalwindowdesktop` — a window's desktop is shell-local again | `a_window_list_is_what_says_which_desktop_a_window_is_on`, `a_window_is_only_visible_on_its_own_desktop`, `switching_desktop_names_no_window`, `super_d_minimizes_everything_on_the_current_desktop`, `a_retitle_reaches_the_button_without_disturbing_shell_local_state` |
| `shellnodesktopbound` — switching walks past the last desktop | `a_desktop_that_does_not_exist_is_not_asked_for` |
| `shellnomovebound` — a window is filed on a desktop that does not exist | `a_window_cannot_be_moved_to_a_desktop_that_does_not_exist` |
| `winguessesdesktopzero` — the client answers 0 before it has been told | `the_desktop_being_shown_comes_from_the_compositor_and_not_from_memory` |
| `shellnextunderflows` — `num_desktops - 1` on a shell with no desktops | `a_shell_with_no_desktops_does_not_underflow` |
| `shellprevunderflows` — `current_desktop - 1` at desktop 0 | `a_shell_with_no_desktops_does_not_underflow`, `desktop_navigation_stops_at_both_ends` |

**`wl2shortheader` fails no stage-2 test, and that is the field-order decision
working.** It fails `a_layer_this_build_does_not_know_is_refused` and
`a_state_bit_with_no_meaning_is_refused_rather_than_ignored` instead — tests written for
something else entirely. `workspace` was put *after* the state byte precisely so
that `state_at = HEADER + 8 + 8 + 1` and `layer_at = HEADER + 8 + 8` kept naming
the fields they were written for; the reward for not rewriting their arithmetic
is that they still catch a mismeasured header. A marker proved only by older
tests is a weaker claim than one proved by its own, and it is recorded as the
weaker claim rather than folded into the count.

**The last two markers exist because the coverage table found a hole.** Running
the first fourteen left `a_shell_with_no_desktops_does_not_underflow` with no
marker at all — every bound-check defect it might have caught was already caught
by a *named* test elsewhere, so the underflow test was passing without ever being
the reason anything failed. The defect it actually guards is written down in
`next_desktop`'s own doc comment — the obvious right-edge test,
`current_desktop < num_desktops - 1`, underflows because `num_desktops` is a
public field that nothing clamps and may be `0`. Putting that back, and the
matching `current_desktop - 1` on the left edge, is what makes the doc comment
checkable rather than merely asserted.

**One marker was refused by the tooling, correctly.** `shellkeepsitsown` was
first written as a *deletion* — replace the assignment with nothing — and
`reintro.py` declines an empty side on purpose: with nothing to search for, the
restore step cannot tell a file that had the anchor from one that never did, so
the byte-for-byte guarantee would be vacuous. Rewritten to replace the
assignment with an inert `let _ = list.current_workspace;`, which drops the
effect and keeps the shape. All six guarded files were verified byte-identical
to baseline after every run.

3193 tests across `guiremote`, `oswindow`, `compositor` and `desktop`; clippy
clean on `x86_64-pc-windows-gnu` (`--all-targets`) and on
`x86_64-unknown-linux-gnu` (`--bins`, the only way the Linux-only arm of the
compositor's `main.rs` is compiled at all); fmt clean.
