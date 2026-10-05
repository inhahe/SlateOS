## 565. A shortcut belongs to the shell even while another window has the keyboard, and the key that *ends* the gesture is owed to both of them

**Date:** 2026-08-26
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** Until now, every keyboard shortcut the desktop defines — Alt+Tab,
Super, Super+D, Alt+F4 — stopped working the instant you clicked into an
application, because the compositor (the program that owns the screen and the
keyboard) sent each keystroke to whichever window had the focus, and that was no
longer the desktop. Alt+Tab in particular could never work, since its whole
purpose is to be pressed *while you are in another window*. The fix is a **key
grab**: the shell asks the compositor to reserve a specific chord for it, and
that chord is routed to the shell no matter who has the focus. This entry
records the rules that made grabs behave, and the one that is genuinely
two-sided: what to do with the **Alt release** that ends an Alt+Tab.

### The mechanism, and the three easy rules

A *grab* is a claim on a **chord** — a key plus a set of modifiers — not on a
key. Grabbing Alt+Tab must not take Tab away from every text field on the
desktop, so the table is keyed `(Key, Modifiers)`. The claim is checked after
the keystroke has been resolved into a chord (so that the keyboard layout and
the AltGr fold have already been applied — a grab is on Alt+Tab, not on
"scancode 0x0F") but **before** the focused-window lookup, which is the entire
point.

Three consequences follow without argument:

- **A grabbed chord's *release* is not the same chord.** Hold Alt+Tab and let go
  of Tab while Alt is still down and the release event is a bare `Tab`, matching
  no grab. So the compositor remembers, per **scancode**, which window received
  the press (`grabbed_presses`) — scancode, because the physical key is the only
  thing guaranteed identical between a press and its release; the `Key` can
  change under a layout switch mid-keystroke.
- **A grabbed chord carries no text and does not feed the dead-key composer.**
  Alt+Tab is not an attempt to type anything, and letting it prime a composition
  would corrupt the next real keystroke.
- **A grab lives exactly as long as the window that took it.** `destroy_window`
  releases the window's grabs, and because `Server::reclaim` destroys a departed
  client's windows through that same function, a shell that *crashes* gives back
  Alt+Tab by the same route as one that exits tidily. A grab that outlived its
  owner would make the chord permanently dead with nothing left to fix it.

### The decision: who gets the modifier release

The fourth rule is the one with two defensible answers.

A hold-to-preview switcher does not act when Tab goes down; it acts when **Alt
comes up** — that is how it knows which window you landed on. But nobody grabs a
bare modifier, so the Alt release matches no grab, goes to the focused
application, and the switcher is never told the gesture ended. It would sit on
screen having activated nothing until some unrelated keystroke arrived.

So: when a grabbed chord that *has* modifiers fires, the compositor records the
scancodes of the modifier keys currently held that contributed to it
(`grabbed_modifiers`), and delivers their releases to the grabber. The question
is whether that delivery **replaces** the focused window's copy or is **added**
to it.

| Option | Verdict |
|---|---|
| Redirect: the grabber gets the modifier release, the focused window does not | **Rejected.** The focused window already saw the Alt *press* — it was routed there normally, before any chord existed. Withholding the release leaves that window believing Alt is held down forever: its next `f` is Alt+F, its next click is an Alt-click. That is precisely the stuck-modifier bug `release_all_modifiers` exists to prevent, and a grab must not manufacture one. |
| **Duplicate: both the grabber and the focused window get the release** | **Chosen.** Every recipient sees a well-formed press/release pair for the keys it saw pressed. The grabber sees a release for a key it never saw pressed, which is the lesser anomaly: a switcher is *looking* for that release, and a client that is not expecting one has no state to corrupt, because a release of a key it never saw pressed is already a no-op in any correct handler. |
| Grab Alt itself while the switcher is open | **Rejected.** Either the grab is permanent, in which case Alt+F mnemonics stop working in every application on the desktop, or it is taken when the switcher opens — which needs a request/confirm round trip to the compositor *in the middle of a gesture*, and still leaves the focused application with the stuck Alt, because a grab taken after the press does not conjure a release for it. |

The rule is deliberately **not** shell-specific: any hold-to-preview interaction
needs it, and putting it in the compositor means each such client does not
reinvent a worse version. Two grabbers can be waiting on the same physical key —
a switcher on Alt+Tab and an accessibility tool on Alt+F7 — so the table maps one
scancode to a *list* of windows.

**The debt is collected before the grab is matched, not after.** A modifier key
can *itself* be a grabbed chord — the shell holds the bare Super key, because
that is the start menu, and Super is also the modifier in Super+D. Taking the
owed release out of the table only on the fall-through path would therefore have
stranded it for exactly the keys most likely to owe one, since the Super release
matches a grab and returns early. It would then have been paid out to some
*later* Super press instead, ending a gesture minutes after the one it belonged
to. So the entry is removed first, and the window the grab has just delivered to
is skipped when the copies go out, because one physical release must not reach
one client twice.

**Losing the keyboard cancels a gesture rather than finishing it.** On a VT
switch, `release_all_modifiers` synthesises the releases the compositor stopped
being able to see, and it clears `grabbed_modifiers` rather than draining it to
the grabbers. Switching away from the desktop mid-Alt+Tab should not commit a
window switch you never chose; the user let go of nothing.

### Grabs go behind `require_shell`, and they are checked against the bindings

The requests are `GrabKey`/`UngrabKey`, gated by `ClientLink::require_shell` —
the compositor's single seam for privileged requests, which today returns
`Ok(())` for everyone and documents why (an honest gate needs a kernel-attested
capability that channel IPC does not yet carry; see 495 and
`TD-C-ANY-CLIENT-CAN-READ-EVERY-WINDOW-TITLE`). Grabs deliberately do **not**
grow a second, parallel policy: when that seam becomes real, they become real
with it. This is what withdrew open question C-Q10, which had asked the operator
to choose a grab-permission model.

Adding a vocabulary to a wire protocol is a version change, so `CONTROL_VERSION`
went 1 to 2; an older compositor answers the unknown tag with
`DecodeError::BadTag` rather than misreading it.

Finally, the shell's grab list is **machine-checked against its binding table in
both directions** (`gui/desktop/src/lib.rs`, `DesktopShell::global_chords`):
every grabbed chord must be one `DesktopAction::for_chord` acts on, and — the
direction that matters — every chord `for_chord` acts on must be grabbed. The
second test sweeps the whole key vocabulary (`guiremote::input::ALL_KEYS` times
all 16 modifier sets) rather than only the chords already listed, because the
failure that actually happens is a *new binding* added to the table and never
grabbed, which is a shortcut that is silently dead in every window but one.

**Escape is the one exception, and is reconciled per event.** A permanent Escape
grab would take the key from every dialog on the desktop; no grab at all means a
menu opened with the mouse cannot be closed with the keyboard. So the shell holds
Escape only while one of its own surfaces is open, deciding from
`DesktopShell::any_popup_open()` — the same single expression `dismiss_popups`
uses, so the two cannot drift — reconciled once per event-loop pump rather than
at the six places that open and close menus, because a scheme that must be
extended at each new one is a scheme that will be forgotten at the seventh. Per
*pump* and not per *event*, because a menu also closes without one: the session
dismisses them on shutdown, and a caller holding the session can dismiss them
outright. Both would otherwise leave Escape claimed with nothing left for it to
close — the same harm the conditional grab exists to avoid, inverted.

**Where it lives.** `gui/compositor/src/lib.rs` — `Compositor::handle_key`,
`grab_target`, `grab_key`, `ungrab_key`, `release_grabs_of`,
`release_all_modifiers`, and the `grabbed` / `grabbed_presses` /
`grabbed_modifiers` tables. `gui/compositor/src/keymap.rs` —
`ModifierState::held_keys_for`. `gui/compositor/src/wire.rs`,
`gui/remote/src/control.rs` — the two requests. `gui/remote/src/client.rs`,
`gui/window/src/lib.rs` — `grab_key` / `ungrab_key`. `gui/desktop/src/lib.rs` —
`GLOBAL_CHORDS`, `CONDITIONAL_CHORD`, `any_popup_open`.
`gui/desktop/src/session.rs` — `ShellSession::start` takes the grabs,
`reconcile_escape_grab` maintains the conditional one.
