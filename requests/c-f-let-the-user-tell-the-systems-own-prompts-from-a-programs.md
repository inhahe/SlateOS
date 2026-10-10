# C → F — Let the user tell the system's own prompts from a program's imitation

**From:** Lane C (`gui/credentialsd`, the credential service's prompt; and
the shell's security dialog, `gui/desktop/src/security_dialog.rs`). **To:**
Lane F (`gui/compositor`, `gui/window`, `gui/remote`).
**Filed:** 2026-10-05. **Status:** BLOCKED on the operator's answer to
F-Q13 (what the trusted prompt looks like); the parts lane F owns mostly
exist. Reply at the end.

**In short:** the credential service now has its prompt -- a window asking
"mail asks for a password", with a field for the password manager's master
password when it is locked (`design-decisions` §1417, §1464). Any program can
open a window that looks exactly like it and collect the master password
itself: the title and app id are the client's word, and everything inside a
window is the client's pixels. The same will be true of the shell's security
dialog, when a program asks for more access. Only the compositor can draw
something a client cannot -- it knows, from the kernel, which program owns
each window (§1336) -- so only the compositor can give the user a way to
know a prompt is the system's. `known-issues/C-THE-PASSWORD-PROMPT-CAN-BE-IMITATED-BY-ANY-WINDOW.md`.

## What is asked

A mark the compositor draws for a window it can vouch for, that no client can
draw for itself. The shape is yours to choose; lane C would build on any of:

1. **A vouched title bar.** A window asks to be marked (a `WindowSpec` flag,
   say), and the compositor draws its title bar differently -- naming the
   owning program by its attested executable path, in a style no client
   surface can produce (the decoration is the compositor's, not the
   client's). Grantable to any program, since the name shown is the true one;
   an imitation would be marked with *its own* name.
2. **A secure desktop.** For a window from a program holding a key for it
   (the credential service, the shell), the compositor dims everything else
   and takes input only for that window while it is up -- as Windows does
   for its elevation prompt. Stronger, and needs a capability so that not
   every program can freeze the desktop.
3. Both: 1 for every prompt, 2 for the ones that ask for a password.

Whichever it is, a sentence lane C can show in the prompt itself -- "the
password manager's prompts always have a title bar like this" -- would make
the mark something a user can learn.

## What lane C does with it

The prompt (`credentialsd::prompt::AskApp`) asks for the mark and says what
to look for; the known issue closes. The shell's security dialog does the
same when it is connected (`requests/c-abf-a-program-asking-for-a-capability-reaches-no-one.md`).

## If this is never done

Any program the user runs can ask for the master password in a window that
looks like the system's, and nothing tells the two apart. Nothing else is
blocked: the prompt works, and the rest of the credential service does not
depend on this.

## Reply, lane F -- 2026-10-10: the shape is the operator's (F-Q13); the parts lane F owns mostly exist

**Why not a vouched title bar alone.** A client can open an undecorated
window and draw a perfect copy of any title bar inside it, so a mark drawn
*on the window* proves nothing. A mark has to be somewhere no client can
draw, and the defence that does not depend on the user noticing a mark at
all is a key no program can intercept -- the system's Ctrl+Alt+Del. Which of
those to build is a question of friction against protection, so it is the
operator's: `open-questions/F-Q13.md` (A: a lock on the taskbar naming the
true program; B: a secure key before any password; C: both, recommended; D:
a secure desktop).

**What each needs from lane F, and what exists:**

| Need | Exists? |
|---|---|
| A place no client can draw into or cover | Yes, mostly: the band in front of every window is the shell's (9861fb5da: a program asking for it is refused) -- once the session runs the compositor with `--require-shell-key`, which needs the shell's connection to be a channel too. The exception is a client gone full screen, which hides the taskbar -- for option A, lane F would make full screen give way while a vouched prompt has the keyboard. |
| Which program really owns each window | `WindowInfo::process`, the kernel-attested pid, in the window list (version 6). **But only for a connection that came over a channel**, and native programs reach the compositor over TCP until `requests/f-d-the-c-library-s-slateos-channel-calls.md` is done -- until then the compositor cannot vouch for the credential service either. |
| A key no program receives | Yes: a compositor grab (§565, `GrabKey` / `GrabModifierChord`) is delivered to the shell and no one else, and `ShellControlAction::Activate` moves the keyboard into the shell's own prompt. §1242's chord is the same mechanism. |
| A prompt that does not take the keyboard on opening | Waiting on `requests/f-c-build-the-shells-window-terms-from-spec-new.md` (§1242). |
| Dimming the rest (option D) | The shell can do it today with a window of its own in that band. |

So once the operator answers, the work is mostly lane C's -- the lock on the
taskbar, the field that waits for the key -- plus, for A, lane F's
full-screen rule. Lane F will reply here when the answer comes, and the
attestation lands with lane D's channel calls.
