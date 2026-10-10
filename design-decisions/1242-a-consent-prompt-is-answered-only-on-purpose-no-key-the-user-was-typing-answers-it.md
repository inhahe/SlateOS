## 1242. A consent prompt is answered only on purpose: no key the user was typing answers it

**Date:** 2026-10-10 · **Decided by:** Operator (the rule, given in chat to
lane E's session; the operator's words are below and in
`operator-answers/2026-10-10-chat-consent-prompts-take-no-keystrokes.txt`).
The details marked *Claude's* are Claude's (operator-approved scope) and the
operator may change them. · **Lane:** E (recorded); the prompt is lane C's
(`gui/desktop/src/security_dialog.rs`), where the keyboard goes when a window
opens is lane F's (`gui/compositor`).

**In short:** When a program asks for more access than it was started with,
the desktop shows a prompt -- Allow or Deny. It appears whenever the program
asks, which may be in the middle of the user typing a sentence, and as written
every key it receives answers or changes it: Enter and A allow, Escape and D
deny, R ticks "Remember this decision". A user typing the word "great" as it
appears would give that program the access, and keep giving it for eight
hours, without having seen the question. The rule: no key the user was
already typing can answer the prompt. It is answered by clicking -- and, for
anyone who cannot use a pointer, through a deliberate keyboard route that
ordinary typing never produces.

> make sure when a capability prompt pops up, there's no key press that can
> cause it to accept or deny, because the user could be in the middle of
> typing when it pops up. it can only be answered by clicking. or if it needs
> keypress support for accessibility reasons, maybe just be able to navigate
> the answers with arrow keys and then press enter (with none selected by
> default), or have a key combination for each answer that's not likely to be
> used by what they're currently doing.
>
> -- the operator, 2026-10-10

### Why it holds for this design

- **The prompt interrupts by design.** `design.txt` has programs start without
  some capabilities -- a capability is an unforgeable handle to one kernel
  object, and holding it is the permission -- and "ask the user for them when
  they install or when they run" (§918 lets a program ask for the keyboard,
  the microphone or the camera). The kernel's broker queues the request
  (`SYS_CAP_REQUEST`) and the desktop shows it; the user did nothing to open
  it.
- **It would get the keyboard the moment it appeared.** The compositor gives
  every new window the keyboard (`Compositor::create_window_from_spec` calls
  `focus_window`), and lane C's plan for the prompt is "a surface of its own
  above every window" (`TD-C-A-PROGRAM-ASKING-FOR-A-CAPABILITY-REACHES-NO-ONE`).
- **And the prompt answers keys.** `SecurityDialog::handle_key_event`: Enter
  and A allow, Escape and D deny, Ctrl+D denies every request waiting, R ticks
  "Remember this decision" -- a remembered Allow answers the same program's
  next requests without asking, for eight hours -- and every other key is
  swallowed, so the user's text is lost as well.
- **Not only by accident.** The program asking is very often the program the
  user is typing into, so it receives their keystrokes and knows exactly when
  they are mid-word. It can time its request to land on "reat" of "great": R,
  then A. Consent that the asker can collect by timing is not consent; the
  prompt exists to make sure the *user* decided.
- **Nothing shows the prompt yet** -- the chain from the kernel to the desktop
  is not connected -- so this has harmed no one. It is the time to fix it:
  before the prompt is wired up, not after.

### The rule

1. **The prompt never takes the keyboard by itself.** When it appears, what
   the user is typing goes on going to the window they are typing in: not
   answering the prompt, and not swallowed by it either.
2. **No key answers the prompt or changes what it will do** -- no Allow or Deny
   key, no "remember" key, no "deny all" key -- until the user has moved the
   keyboard into it on purpose (5).
3. **A click answers it.**
4. *Claude's:* **but not a click already on its way.** For the first second
   after the prompt appears -- and again when it moves on to the next request
   in its queue, whose buttons are in the same places -- its buttons do
   nothing, and are drawn so. A click counts only if the button was both
   pressed and released after that. Otherwise a double-click, or a run of
   clicks a program has asked for ("click here quickly"), aimed where the
   program knows the Allow button will appear -- the prompt is centred, its
   layout fixed -- answers it just as typing would. Web browsers hold back
   their permission prompts' buttons for the same reason.
5. *Claude's, from the operator's second suggestion:* **the keyboard route.** A
   chord with the logo (Super) key that the desktop does not use for anything
   else moves the keyboard into the prompt. The compositor keeps such chords
   for the desktop (§565): no program receives one, and typing never
   produces one. The prompt's text names the chord, and when the desktop
   has a screen reader (it has none yet), the question and the chord are what
   it reads out as the prompt appears. Inside, Tab and Shift+Tab or the arrow keys
   move between the answers, with **none chosen** until the user moves; Enter
   or Space gives the chosen answer; Escape gives the keyboard back to the
   window it came from, answering nothing. The one-second hold (4) applies to
   keys inside it too.
6. **It covers every prompt of its kind:** any prompt that asks the user to
   grant or refuse something and that opens without the user having asked for
   it. Today that is the capability prompt; it will be the ones F-Q3 (a
   program capturing the screen), F-Q6 (a remote viewer) and §918 (the
   keyboard, microphone or camera) bring, and the credential service's "this
   program asks for a password" (`requests/c-a-a-capability-to-ask-the-credential-service-for-a-password.md`).
   It does not cover a dialog the user's own action opened -- "Save your
   changes?" on closing a document, a file chooser -- which keep their keys.

### The two keyboard alternatives the operator raised

| Alternative | Why not as the route |
|---|---|
| Arrow keys move among the answers, Enter answers, nothing chosen at first | Kept, behind one step (5). From the start it is not safe: Up then Enter is how a terminal runs its last command again, and Down then Enter is how an editor starts a new line -- two keys typing produces constantly, and the first would choose an answer and the second give it. |
| A key combination per answer that typing is unlikely to produce | Workable if the combinations are desktop-only Super chords. Not chosen because an answer bound to one chord is a single stray press from being given, and the user gives it without the question in front of them; the route in (5) passes through the question first. |

| Other alternative | Why not |
|---|---|
| Keep the keys, add only the one-second hold | The program asking chooses when to ask and sees the user typing; it can wait out a hold as easily as aim at a word. |
| Take the keyboard but answer no keys | Safe, but every key the user types while the prompt is up is lost from what they were writing. |
| Answer only with keys typing never produces, no clicking | Shuts out no one but slows everyone, and the operator asked for clicking. |

### Where it bites

`gui/desktop/src/security_dialog.rs` (`handle_key_event`, `handle_mouse_event`,
the drawing of the buttons while held back), the desktop's wiring of the
prompt (`TD-C-A-PROGRAM-ASKING-FOR-A-CAPABILITY-REACHES-NO-ONE`), and
`gui/compositor` (a window that does not take the keyboard when it opens, and
the keyboard going back where it was when the prompt closes). Asked of lanes C
and F in `requests/e-cf-a-consent-prompt-is-answered-only-on-purpose.md`; the
defect is `known-issues/E-the-capability-prompt-is-answered-by-the-keys-the-user-is-typing.md`
until it is fixed.

**Reversal:** the rule is the operator's. Changing the one-second hold, the
chord, or the keys inside the prompt is lane C's, recorded here.
