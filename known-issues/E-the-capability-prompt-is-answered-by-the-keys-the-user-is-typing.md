### [E] The capability prompt is answered by the keys the user is typing -- 2026-10-10

**Status:** OPEN -- latent: nothing shows the prompt yet
(`TD-C-A-PROGRAM-ASKING-FOR-A-CAPABILITY-REACHES-NO-ONE`), and it must be
fixed before anything does. The fix is lane C's and lane F's, asked in
`requests/e-cf-a-consent-prompt-is-answered-only-on-purpose.md`; the rule is
the operator's, design-decisions §1242. Filed by lane E, which was given the
rule; the code is not lane E's.

**In short:** The desktop's security prompt -- "this program asks for more
access: Allow or Deny" -- answers the keyboard. Enter and A allow, Escape and D
deny, R ticks "Remember this decision", and it would have the keyboard from the
moment it appeared, because the compositor gives every new window the
keyboard. So a user typing the word "great" when the prompt appears gives the
program what it asked for, and goes on giving it for eight hours, without
seeing the question; and the program asking, which receives the user's
keystrokes, can time its request so that this happens.

**Where:**

- `gui/desktop/src/security_dialog.rs`, `SecurityDialog::handle_key_event`:
  Enter and A (`!ctrl`, so Shift+A too) call `allow_current`; Escape and D
  call `deny_current`; Ctrl+D calls `deny_all`; R toggles `remember`; every
  other key is consumed. `allow_current` with `remember` set records the
  decision, and `push_request` then answers that program's matching requests
  itself for `REMEMBERED_ALLOW_LIFETIME_MS` (eight hours). It never reads
  `event.pressed`, so a key's *release* answers as its press does: Enter
  pressed in the user's program just before the prompt appeared allows when
  it comes up. Lane C's own tests pin the key map (`test_keyboard_allow`,
  `test_keyboard_deny`, `test_keyboard_deny_all`, `test_keyboard_toggle_remember`).
- `SecurityDialog::handle_mouse_event`: a press on Allow allows at once, with
  no time for the user to have seen what they are pressing.
- `gui/compositor/src/lib.rs`, `Compositor::create_window_from_spec`: every
  new window is given the keyboard (`focus_window`), with no way for a window
  to open without it.

**Reproduce** (the dialog's own API, since nothing shows it on screen yet):
`push_request` a request, then `handle_key_event` the keys of "great" one by
one. `drain_events` holds `Approved(id)` and `RememberToggled(id, true)`, and
a second `push_request` of the same program, resource and rights is approved
without being shown.

**The proper fix:** §1242 -- the prompt takes no keyboard on its own and no
key answers it; a click answers it, but not within its first second on screen
(or on the next request in its queue); a desktop-only Super chord moves the
keyboard into it, where Tab or the arrows move among answers with none chosen
and Enter answers. The tests that should then hold are listed in the request.
