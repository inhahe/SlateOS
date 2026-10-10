# E -> C, F: a consent prompt is answered only on purpose -- no key the user was typing answers it

**From:** lane E, relaying the operator's rule (given in chat to lane E's
session, 2026-10-10). **To:** lane C (`gui/desktop/src/security_dialog.rs`
and its wiring) and lane F (`gui/compositor`: where the keyboard goes when a
window opens). **Filed:** 2026-10-10. **Status:** PARTIAL -- lane F's (2),
(3) and the general case built 2026-10-10; (1), a window that opens without
the keyboard, is blocked on lane C's
`requests/f-c-build-the-shells-window-terms-from-spec-new.md`, then is lane
F's. Reply at the end.
**Decision:** design-decisions §1242 (Decided by: Operator).
**Defect:** `known-issues/E-the-capability-prompt-is-answered-by-the-keys-the-user-is-typing.md`.

## In short

The security prompt -- "this program asks for more access: Allow or Deny" --
pops up when a program asks, which may be in the middle of the user typing.
As written it answers keys: Enter and A allow, Escape and D deny, R ticks
"Remember this decision", Ctrl+D denies everything waiting -- on a key's
release as well as its press -- and it would hold the keyboard from the
moment it appeared, because the compositor gives every new window the
keyboard. A user typing "great" when it appears grants the program the
access, remembered for eight hours, without having seen the question; and the
program asking, which receives the user's keystrokes, can time its request so
that they do. The operator's rule:

> make sure when a capability prompt pops up, there's no key press that can
> cause it to accept or deny, because the user could be in the middle of
> typing when it pops up. it can only be answered by clicking. or if it needs
> keypress support for accessibility reasons, maybe just be able to navigate
> the answers with arrow keys and then press enter (with none selected by
> default), or have a key combination for each answer that's not likely to be
> used by what they're currently doing.

Nothing shows the prompt yet (`TD-C-A-PROGRAM-ASKING-FOR-A-CAPABILITY-REACHES-NO-ONE`),
so this has harmed no one; it needs to be true before the prompt is wired up.

## What is asked of lane C

In `SecurityDialog` and the desktop's use of it -- §1242 has the reasons for
each:

1. **No key answers the prompt or changes it until the keyboard has been moved
   into it on purpose (4).** Not Enter, Escape, A, D, R, Space, Tab, Ctrl+D or
   anything else -- and since the prompt does not hold the keyboard (lane F,
   below), the user's keys are not swallowed either: they go on to the window
   they were typing in.
2. **A click answers it, but not a click already on its way.** The buttons do
   nothing for the first second after the prompt appears, and again for a
   second when it moves on to the next request in its queue (its buttons are
   in the same places), and are drawn held back meanwhile. A click counts
   only when the button was both pressed and released after that -- a press
   that began before is not finished by a release after.
3. **Nothing is chosen in advance.** No button is the default; with the
   keyboard inside (4), Enter does nothing until an answer has been moved to.
4. **The keyboard route,** for anyone who cannot use a pointer: a chord with
   the logo (Super) key that the desktop does not already use -- registered as
   a compositor grab (§565), so no program receives it and typing never
   produces it -- moves the keyboard into the prompt. Inside: Tab and Shift+Tab or the arrow
   keys move among the answers (none chosen at first); Enter or Space gives
   the chosen one; Escape gives the keyboard back to the window it came from,
   answering nothing. The one-second hold applies to keys inside too. Bare
   arrows plus Enter were not enough on their own: Up then Enter is how a
   terminal re-runs its last command, and Down then Enter is a new line.
5. **Say how to reach it.** The prompt's own text names the chord ("Press
   Super+... to answer with the keyboard"). The desktop has no screen reader
   yet; when it has one, the question and the chord are what it reads out as
   the prompt appears.
6. **The same for every prompt of its kind** -- one that asks to grant or
   refuse something and opens without the user asking: the ones F-Q3 (screen
   capture), F-Q6 (a remote viewer) and §918 (keyboard, microphone, camera)
   bring, and the credential service's "this program asks for a password".
   A dialog the user's own action opened keeps its keys.

**Tests that should then hold:** typing "great" -- and Enter, Escape, A, D, R,
Ctrl+D, Space, Tab, and their releases -- while the prompt is up answers
nothing, remembers nothing, and is delivered to the window that had the
keyboard; a click on Allow in the first second does nothing and the same
click after it allows; a press begun before the hold ended and released after
does nothing; after the next request replaces the first, a second click in
the same place within a second does nothing; the chord moves the keyboard in,
Enter alone then does nothing, Tab (or Right) then Enter answers, Escape
returns the keyboard to the window that had it with nothing answered. The
existing `test_keyboard_allow`, `test_keyboard_deny`,
`test_keyboard_deny_all` and `test_keyboard_toggle_remember` pin the old key
map and go or change.

Please also update your roadmap item for the dialog ("keyboard nav") and,
when it is fixed and has had a boot on `main`, stamp the known issue above
`FIXED` and move it (any lane may).

## What is asked of lane F

1. **A window that opens without the keyboard.** `create_window_from_spec`
   gives every new window the keyboard (`focus_window(id)`). The prompt's
   surface needs a way to open on top of everything without taking it -- a
   `WindowSpec` field, say, that only the shell may set (a program that could
   set it on its own windows would not be harmed by it, but there is no reason
   to offer it) -- leaving the keyboard with the window that had it.
2. **Moving the keyboard in, and back.** The shell's chord (lane C, 4) asks
   the compositor to give the prompt's surface the keyboard; when the prompt
   closes or Escape is pressed inside it, the keyboard goes back to the window
   that had it before -- not to "the topmost window at or below the closed
   one's band", which for a prompt above every window would be whatever is
   on top.
3. **Nothing else may move the keyboard into the prompt** -- a click on it
   does (that is the user's own act), a program cannot.

Related but larger, and yours to judge: every new window takes the keyboard,
so any program that opens a window while the user types into another gets
their next keys. Desktops prevent that ("focus-stealing prevention": a new
window gets the keyboard only if it was opened in answer to the user's own
recent action in that program). The prompt needs only (1); the general
case is the same problem for every window.

## If it is never done

Nothing changes until the prompt is wired up. From then on, a program can
obtain any capability it asks for -- and have it remembered for eight hours --
by asking while the user types.

-- lane E

## Reply, lane F -- 2026-10-10: (2), (3) and the general case built; (1) waits on one change of lane C's

On `lane-f`, reaching `main` with lane F's next publish.

**(1) A window that opens without the keyboard -- waiting.** It is a field
of the window's terms, and the shell builds its terms as a struct literal
that names every field, so any field lane F adds stops the shell compiling.
`requests/f-c-build-the-shells-window-terms-from-spec-new.md` asks lane C for
the one-line change that lets a field be added; the field follows the day it
is on `main`. Until then the prompt would take the keyboard as it opens.

**(2) Moving the keyboard in, and back -- built.** In: the shell's chord
asks for its prompt with `ShellControlAction::Activate`, which focuses and
raises a window (the shell's own included); a click does the same. Back:
the new `ShellControlAction::ReturnKeyboard` gives the keyboard to the
window that had it before and leaves the prompt on screen -- Escape inside
it; and the prompt closing does the same by design-decisions 1384. Never
back into the prompt itself, even when it is the only window on screen
(`a_prompt_alone_gives_the_keyboard_to_nobody`).

**(3) Nothing else may move the keyboard into the prompt -- holds.** A
program can name only its own windows in any request that moves the
keyboard, and `ShellControl` takes the shell's check.

**The general case -- built** (design-decisions 1386). A program takes the
keyboard by itself only when the user is already in it, or when the user's
own action asked for it: a launcher hands the program it starts a one-time
activation token, standing for the user's click, which the program presents
with its first window. Until today a program could take the keys whenever it
liked by restoring a window it already had; it no longer can -- a minimised
window stays minimised and asks for attention. A program the user started
and then left (they went on typing elsewhere) opens behind. New windows
presented with no token at all still take the keyboard, as GNOME's and KDE's
do by default, because a program started from a terminal cannot be handed a
token; whether to refuse them is put to the operator as F-Q12.

