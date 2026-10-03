## `TD-C-A-SHORTCUT-ONLY-WORKS-WHEN-THE-DESKTOP-IS-FOCUSED` (lane C, 2026-08-26) -- **CLOSED; entry was stale**

**Closed 2026-09-07 (lane C), on verifying it.** The entry's mechanism claim --
"there is no global-hotkey path, no key-grab table, and no 'system keys' list
anywhere in the tree" -- is no longer true. `Compositor::handle_key` consults a
grab table, and the shell's session claims its chords at startup.

The implementation goes further than the entry asked for, and the comments at
the call site name the two decisions that matter:

- **Grabs are consulted after the chord is known and before the focused window
  is looked up.** After, because a grab is on Alt+Tab rather than on scancode
  `0x0F`, so it cannot be matched until the layout and the AltGr fold have had
  their say. Before, because the whole point is to reach a client that is not
  focused.
- **A match returns rather than also delivering to the focused window** --
  otherwise a text field would take a bare `Tab` every time the user switched
  windows.
- A modifier release owed to a grabber is taken from the table *before* the
  grab check, because a modifier key can itself be a grabbed chord (the shell
  holds bare Super, and Super is also the modifier in Super+D), so a debt
  collected after the early return would never be collected for exactly the
  keys most likely to owe one.

Eleven grab tests pass, including
`a_grabbed_chord_reaches_the_grabber_and_not_the_focused_window` and
`a_shell_grabs_a_chord_over_the_wire_and_receives_it_from_another_window` --
which is this entry's headline case, Alt+Tab pressed from inside somebody
else's window.

**Nothing was done to the code for this closure.** Fifth stale entry closed
today; see the note in `todo.txt`.

Original entry follows.

---


**In short:** every keyboard shortcut the desktop defines — Super+N, Alt+Tab,
Super+D, and now the volume keys — only fires while the taskbar itself has
keyboard focus, which for a taskbar is almost never. The moment the user clicks
into any application window, the entire shortcut table goes dead. Alt+Tab, the
one shortcut whose entire purpose is to be pressed *while you are in another
window*, cannot work at all. This is not a bug in the shortcut table; the
shortcuts are registered correctly and would fire. It is that key events never
reach the shell.

**The mechanism.** `Compositor::handle_key` (`gui/compositor/src/lib.rs`, ~line
6640) opens with:

```rust
let Some(window_id) = self.focused_window else { return; };
```

and then delivers the event to that window and no other. There is no
global-hotkey path, no key-grab table, and no "system keys" list anywhere in the
tree — a client can only ever see keys pressed while it is focused. The desktop
shell is just another client.

**What this costs, concretely.**

| Shortcut | Works when the desktop is focused | Works when an app is focused |
|---|---|---|
| Alt+Tab (window switcher) | yes | no |
| Super+D (show desktop) | yes | no |
| Super+N (notification pane) | yes | no |
| Volume up/down/mute | yes | no |
| Anything a user binds in their config | yes | no |

**What the proper fix is.** A key-grab table in the compositor, checked *before*
the focus lookup above:

- Two new requests on the existing client protocol, `GrabKey { key, modifiers }`
  and `UngrabKey { .. }`, held in a compositor-owned map from chord to client.
- `handle_key` consults that map first. A grabbed chord is delivered to the
  grabbing client and **not** to the focused window; everything else falls
  through to the code that exists today, unchanged.
- First grabber wins, and a second client asking for the same chord gets an
  error rather than silently shadowing the first — a grab that quietly does
  nothing is the failure mode this whole entry is about.
- Grabs die with the client's connection, so a crashed shell does not
  permanently swallow Alt+Tab.

This is the X11 / `RegisterHotKey` shape, and it is the reason those systems'
shortcuts work from inside applications.

**Who is allowed to grab, and why that needed no new decision.** A grab is
plainly privileged — a program that claims Super+L can put up a fake lock screen
and collect the password, and one that claims Alt+Tab breaks window switching for
the whole session. That question was briefly filed for the operator and then
withdrawn, because the compositor had already answered it:
`ClientLink::require_shell` (`gui/compositor/src/wire.rs`) is the single seam
every privileged request goes through, it refuses nobody today, and it says so at
length — the honest gate needs a capability the kernel attests at connection
accept, which kernel channel IPC does not yet carry to the compositor
(`design-decisions.md` §495, tracked as
`TD-C-ANY-CLIENT-CAN-READ-EVERY-WINDOW-TITLE`). Key grabs go behind that same
seam rather than growing a policy of their own, so the day the capability arrives
they are gated along with reading the window list, acting on other people's
windows and reserving a panel edge.

**What happens if nothing is done.** It does not get worse with time, but it
gets more expensive to notice: every shortcut added from here on is written,
tested at the unit level, and appears to work, while being unreachable in
practice. Two have already been added in this state (the media keys, and the
notification-pane toggle). Anything gated behind a keystroke — an on-screen
volume overlay, for instance — cannot be finished until this is.

**Resolved (2026-08-26).** Built as described above, in two commits: the
mechanism, then the shell wiring that uses it. `Compositor::handle_key` now
consults a chord-keyed grab table after resolving the keystroke into a chord and
before the focused-window lookup, `GrabKey`/`UngrabKey` ride the client protocol
behind `require_shell` (`CONTROL_VERSION` 1 to 2), grabs die with the window that
took them — including via `Server::reclaim`, so a *crashed* shell gives Alt+Tab
back — and `ShellSession::start` claims all seventeen of the desktop's chords
against the panel window, with Escape grabbed and ungrabbed as popups open and
close. Two details the plan above did not anticipate, both now in
`design-decisions.md` 565:

- A grabbed chord's **release** does not match the grab (Alt+Tab held while Tab
  is let go sends a bare `Tab` up), so the target is remembered per *scancode*,
  the only thing identical between a press and its release.
- Alt+Tab commits on the **Alt release**, which nobody grabs. The modifier keys
  that formed a grabbed chord now owe their release to the grabber as well —
  *in addition to* the focused window, never instead of it, since that window saw
  the press and must not be left with a stuck Alt.

The shell's grab list is checked against `DesktopAction::for_chord` in both
directions by test, sweeping the entire key vocabulary, so a binding added
without a grab — a shortcut silently dead in every window but one — fails the
build rather than shipping.
