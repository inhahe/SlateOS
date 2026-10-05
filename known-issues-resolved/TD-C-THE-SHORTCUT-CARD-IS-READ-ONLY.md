## `TD-C-THE-SHORTCUT-CARD-IS-READ-ONLY` (lane C, 2026-08-26) -- **CLOSED 2026-09-07**

**Two of the three missing parts are done.** The card's rows can be walked with
the arrow keys, Enter starts recording, and the next chord becomes the binding.

The part the entry called "the real work" is the one that landed: while
recording, the keystroke is **data**. The check sits at the very top of
`handle_hotkey_inner`, before every modal surface, because a user rebinding
"show the desktop" presses Super+D -- and if the shell ran it, the desktop
would be shown while they were trying to say what those keys mean. Escape
means "cancel the rebind" there rather than "close the card", and a bare
modifier does not end the recording, so reaching for Ctrl on the way to Ctrl+F9
does not bind the shortcut to Ctrl.

A chord already in use is refused and the refusal *names the holder*
("PrintScreen is already Screenshot"), because "already in use" leaves the user
hunting. A refused rebind leaves the original binding untouched, and a rebind
that fails to register puts the old chord back rather than leaving the action
with no chord at all.

7 tests. Mutation-checked: removing the capture gate -- which is exactly the
old behaviour -- fails four of them.

**Persistence: done 2026-09-07, and it removed a format rather than adding
one.** `HotkeyConfig`'s bespoke text file had **no caller outside its own
tests**, and `design.txt` says configuration files are YAML. So the shell does
not use it: `HotkeyConfig::write_into`/`read_from` put the bindings in a
`Document` under `shortcuts`, saved to `shortcuts.yaml` beside the desktop's
other settings, and the one line parser is shared with the text form so the two
cannot disagree about what `Ctrl+ +` means.

A YAML *sequence* of `chord=action` strings rather than a mapping, and not by
taste: two actions carry a parameter (`switch_desktop:3`,
`launch:/usr/bin/explorer`), so an action-keyed mapping would put a colon and a
path in a YAML key -- and a chord-keyed one could be written but never read
back, because nothing in `yamldoc` enumerates a mapping's keys.

**Loading applies over the defaults rather than replacing them.** A file
written by an older desktop names the shortcuts that existed then; replacing
the table with it would silently drop every shortcut added since. Each saved
binding *moves* its action rather than adding a second chord for it, so a
rebound shortcut does not answer to both its old and new chords after a
restart. An unparseable file is ignored rather than costing the user the
defaults.

The save happens on the rebind, not on shutdown -- a desktop that lost power
between the two would forget it, and the user has no way to know saving was
pending. If the write fails the card says so, because "Ctrl+F12 is now Show
Desktop" and "...but could not be saved" are different promises.

**Re-grabbing: done 2026-09-07.** `ShellSession::reconcile_global_grabs` runs
once per pump, on the same unconditional footing as the existing
`reconcile_escape_grab` and for the same reason -- a rebind happens several
layers down inside `handle_hotkey`, and threading a "the chords changed" flag
back up would be one more thing to forget at one more call site.

The session now *remembers* which chords it holds rather than recomputing
them, because after a rebind the registry can no longer say which chords were
grabbed before it, and an ungrab needs exactly that.

Both directions are tested. The quieter half is the ungrab: grabbing the new
chord without releasing the old one leaves the shell holding a chord no
shortcut uses, which is a key no application can ever see. Mutation-checked --
deleting the ungrab loop fails the test.

Original entry follows.

---


**In short:** you can now open the card that lists every keyboard shortcut
(`Super+/`), but you cannot change a shortcut from it. The shortcuts *are*
changeable — there is a text configuration file format for them, and the code
that reads and writes it works — so the only way to move a shortcut is to edit
that file by hand, which no desktop user is going to do. Every other desktop lets
you click a shortcut and press the new keys.

**Where it lives.** `gui/desktop/src/hotkeys.rs`. Almost all the parts exist:

| Part | State |
|---|---|
| `render_settings_panel`'s `selected_index` — draws one row highlighted | done, and drawn with `None` by `DesktopShell::render_shortcut_card` |
| `HotkeyRegistry::conflicts_with` — "that chord already does X" | done, tested |
| `HotkeyConfig::from_registry(...).save()` / `load` — the file format | done, tested, round-trips every action |
| Moving the selection with the arrow keys | missing |
| A *capture mode* — "press the chord you want now", where the next keystroke is read as data rather than run as a shortcut | missing, and is the real work |
| Anywhere on disk to save to | missing — nothing in the shell reads or writes a settings file yet |

**What the proper fix looks like.** The capture mode is the part that needs
thought, because it inverts the shell's whole input rule: while it is on, the
shell must *not* run the chord the user presses, and must not let the compositor
run it either — including chords the shell has grabbed globally, and including
Escape, which has to mean "cancel" rather than "close the card". A
`shortcut_capture: Option<usize>` on `DesktopShell` naming the row being rebound,
checked at the top of `handle_hotkey` before `bound_action`, is the shape; the
new binding then goes in via `HotkeyRegistry::register`, whose `Err(Conflict)` is
what the card shows instead of applying it.

The grabs also have to be re-reconciled after any change: the set of chords the
shell holds is derived from the registry (`global_chords`), so a rebind must
ungrab the old chord and grab the new one, exactly as
`SessionRunner::reconcile_escape_grab` already does for the conditional ones.
That is the piece most likely to be forgotten, and forgetting it means the
rebound shortcut works until the session restarts.

**If it is never fixed:** the shortcuts stay at their defaults for every user in
practice. Nothing breaks; the card is still worth having as a reference. This is
a feature gap, not a bug.
