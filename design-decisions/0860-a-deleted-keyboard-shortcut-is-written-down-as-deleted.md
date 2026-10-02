## 860. A deleted keyboard shortcut is written down as deleted

**In short:** the shortcuts file is merged onto the shipped defaults rather
than replacing them, so that a shortcut added in a later version still reaches
somebody who has customised theirs. That merge has a consequence: deleting a
shortcut cannot be expressed by leaving it out of the file, because leaving it
out is exactly what a file that has never heard of it looks like — and the
default would come back at the next login. A deletion is therefore written
down, as a line reading `none=<action>`.

**Date:** 2026-09-17. **Lane:** C. **Decided by:** Claude (autonomous).

**The two ways to do it.**

| | *What changes* |
|---|---|
| **A. The file is authoritative** (replace the defaults) | Deleting works with no new syntax. A shortcut added in SlateOS 2 never appears for anybody who has ever changed a shortcut in SlateOS 1, because their file predates it and now outranks it. |
| **B. Merge, and record deletions** (chosen) | New defaults keep arriving. The format grows one spelling, `none=<action>`, and a reader that has never heard of it must not mistake it for a chord. |

**Why B.** The people most likely to have customised a shortcut are the
people most likely to care about a new one, and A silently freezes exactly
them. The cost of B is one reserved word on the left-hand side and the
discipline of writing tombstones on every save — which
`HotkeyConfig::from_registry` does by comparing the registry against
`defaults()`, so a save is truthful without anyone having to remember.

**The trap, and the test that names it.** A deletion that only dropped the
line passes every assertion made against the running session: the registry
really has forgotten the binding. It comes back at the next login. So the
test is `a_deleted_shortcut_is_still_deleted_in_a_fresh_shell`, which builds
a second `DesktopShell` from the defaults, asserts the default *is* there
(or the test proves nothing), loads the file, and asserts it is gone. With
tombstones ignored on load it fails with "the deleted shortcut came back at
the next login".

**`none` only counts as the whole left-hand side.** `none` is a word a chord
could plausibly start with, and a parser that matched a prefix would unbind
something a user had bound. `only_a_bare_none_is_a_tombstone` holds that line.

**A bug this uncovered, which predates it.** Writing
`deleting_one_shortcut_keeps_the_rest` failed with "deleting one shortcut took
Start Menu with it", against code that had nothing to do with deleting.
`load_shortcuts` unregistered every *other* chord bound to an action as it
applied each binding — so an action with two chords had them delete each
other, and only the last survived. The defaults put the Start Menu on both
Super keys on purpose (one answers a driver that sets the Super bit, one a
driver that does not), so after any save and reload one of the two Super keys
silently stopped working. Fixed here, with
`an_action_on_two_chords_keeps_both` guarding it by name.
