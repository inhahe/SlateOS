## 568. The keyboard gets its own way to say "start this program", and the Run box is what needed one

**Date:** 2026-08-26
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** the desktop now has a Run box — press the Windows key and R, type
a program's name, press Enter, and it starts. Four smaller choices inside that
had a real alternative on the other side, and this records them: keyboard
shortcuts gained a way to report "start this program", which they had never had
before; the chord is Windows+R rather than Ctrl+R; the box is checked for
keystrokes *before* every other panel the shell can put on screen; and the
box's **Browse...** button was left doing nothing rather than made to do
something adjacent.

### 1. `HotkeyOutcome` gained a launch channel, because it structurally lacked one

The shell reports what it wants done in two different shapes, depending on
which input caused it. A **click** returns a `ShellAction`, which has a
`Launch(String)` variant: the shell has no connection to the process server, so
it does not start programs — it says which one it wants started and the session
loop above it carries that out. A **keystroke** returns a `HotkeyOutcome`,
which had `consumed` and `requests` and nothing else. `requests` is the
keyboard's counterpart of `ShellAction::Control` — minimise this window, tile
that one — and the parallel simply stopped there.

It stopped there because nothing had ever needed the other half. Every shortcut
in the binding table acts on a window that *already exists*: Super+D minimises
what is on screen, Alt+Tab raises one of them, Super+Left tiles one. Pressing
Enter in the Run box is the first keystroke in the shell's history that can
bring a *new* process into being, and at that moment the missing channel stops
being a symmetry and becomes a defect: the box would take the text, resolve it,
report an `Execute`, and the shell would have nowhere to put the answer.

**The alternative considered and rejected** was to have the shell keep a
`pending_launches: Vec<String>` field that the session drains separately, out
of band. It works, and it is smaller. It is wrong because it puts the same fact
— "a program should be started" — into two different places depending on which
input produced it, so any future caller has to know both, and a caller that
learns only one silently drops half the launches. The field also has no natural
moment to be drained: `requests` is drained at the point the keystroke is
handled, and a separate buffer would be drained whenever someone remembered.

`launches` is a `Vec` rather than an `Option` for the same reason `requests`
is: both are unordered independent asks, and one keystroke starting two
programs is not a case worth forbidding in the type when nothing about the
carrying is different.

### 2. Windows+R, not Ctrl+R

The module's own doc comment offered either. Ctrl+R is not taken by the desktop
and looks free — but it is taken by roughly *every application that has a
reload command*, and a desktop shortcut is a **global grab**: the compositor
routes the chord to the shell before the focused window ever sees it. Grabbing
Ctrl+R would therefore break reload in every browser, editor and file manager
on the system, in exchange for saving one modifier key.

Windows+R costs nothing by comparison. The Super key is reserved for the
desktop by convention on every system a user is likely to have come from, the
shell already grabs seven other Super chords, and Windows uses Super+R for this
exact box — so it is the chord a user is most likely to try first.

### 3. The box is checked ahead of every other popup, because it is painted last

`handle_hotkey` walks the shell's modal surfaces in order and gives the key to
the first one that is open. The Run box was inserted at the *front* of that
walk, ahead of the window overview and the notification pane.

The reason is that `paint_chrome` draws it **last**, which puts it on top of
everything else — and the surface on top has to be the surface that owns the
keyboard, or the user is typing into something they cannot see. The two
orderings are one decision expressed twice, and they have to agree.

Worth stating plainly: in practice nothing else is ever open underneath it.
Opening the box dismisses the other popups on the way in, and a press outside
the box closes the box. So this ordering is stating an invariant rather than
resolving a case that arises — which is exactly why it is worth writing down,
because a future surface drawn after the box would break it silently.

The toggle chord is the one exception, as it is for the overview and the pane:
Super+R still reaches the binding table while the box is open, so the chord
that opened it closes it. Without that exception the box would be a one-way
door from the keyboard.

### 4. Browse does nothing, and that is better than doing something adjacent

The box has an OK, a Cancel and a **Browse...** button. Browse asks for a file
*picker* whose answer goes back into the command box. The shell cannot provide
one: `guitk::dialog::FileDialog` exists and would serve, but it must be *fed* a
directory listing by its embedder, and `gui/desktop` performs no filesystem
reads at all.

**The tempting alternative was to start the file manager** — `/usr/bin/explorer`
is in the tree and it is one line. It was rejected because it answers a
different question. The user would get a window they did not ask for, in front
of the box, and a command box exactly as empty as before. That is worse than
the button doing nothing, because it *looks* like it worked, and a user who
believes a button works will press it again rather than typing the path.

So the event is dropped, with the reason at the match arm, an entry in
`known-issues.md` (`TD-C-THE-RUN-BOX-BROWSE-BUTTON-HAS-NOWHERE-TO-GO`) carrying
the four-step proper fix, and a test pinning the current behaviour so it cannot
change by accident.

**Where it lives.** `gui/desktop/src/lib.rs` — `HotkeyOutcome::launches`,
`HotkeyOutcome::start`, `DesktopAction::ToggleRunDialog`, `toggle_run_dialog`,
`centre_run_dialog`, `key_on_run_dialog`, `drain_run_dialog`,
`render_run_dialog`, and the modal arms in `handle_hotkey_inner` /
`handle_mouse_inner`. `gui/desktop/src/run_dialog.rs` — `centre_on`,
`position`. `gui/desktop/src/session.rs` — `paint_chrome`, `popups_open`, and
the `launches` drain in `dispatch`.
