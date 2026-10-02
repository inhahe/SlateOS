## `TD-C-A-FROZEN-FIELD-LICENSES-CONSTANTS-THAT-ASSUME-IT` -- **FIXED 2026-09-19** (lane C)

**In short:** `apps/connect4` decided which colour you play in one place and
wrote the answer out again, as words, somewhere else. While the first place
could never change, the second was correct. The moment a key could change it,
the program started announcing the machine's moves under the wrong colour.

**What happened.** `human_player` was `Cell::Red` and `ai_player`
`Cell::Yellow` at construction, with no writer in the crate -- a frozen field
of the kind `scripts/frozen-flag-survey.py` exists to find. Unfreezing it with
an `S` key was three lines. Then the game began saying "Yellow is
thinking..." while the machine played Red, because `status_line` held the
string

    "Yellow is thinking...".to_string()

rather than `self.ai_player.name()`. Two of them, in fact; "Yellow wins!" as
well.

**Why it is worth a name.** Every fix in this sweep unfreezes a field, and
this is the failure mode that fix *creates*. A constant that duplicates a
frozen field is not wrong while the field is frozen -- it is a correct
statement about a value that cannot change -- so it reads as fine in review,
type-checks, and has no lint. It becomes a lie at the instant the field gains
a writer, and the commit that introduces the lie is the commit that fixes the
defect. **The repair is never complete at the field; it ends wherever the old
value was written down.**

**The test shape that catches it.** Not "the status line is unchanged" and
not "the status line contains Yellow" -- both pass against the defect. It has
to assert the string names *the colour the machine is actually playing*:

    assert!(app.status_line().contains(app.ai_player.name()));
    assert!(!app.status_line().contains(app.human_player.name()));

Read from the field, in the test, so the test cannot encode the assumption it
is checking for.

**Checked, not assumed.** The other six apps whose fields this sweep
unfroze -- `videoplayer`, `qrcode`, `hexeditor`, `filesearch`,
`archivemanager`, `systemrestore` -- were searched for production literals
naming a variant of the thawed type. Every hit was in a test asserting a
`label()`, which is what a test should do. `connect4` was the only one, and
it is fixed in `3a96087a1`.

**What would find the next one mechanically.** For a field the survey reports
as frozen, take the string its current value renders to and look for that
string in live code outside the type's own `label`/`Display`. That is a much
narrower search than it sounds -- the survey has 25 open rows -- and it is
worth running as part of each fix rather than as a gate, since the window in
which it matters is the fix itself.
