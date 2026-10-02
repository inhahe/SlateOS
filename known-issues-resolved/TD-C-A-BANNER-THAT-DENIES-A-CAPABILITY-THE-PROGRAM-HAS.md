## TD-C-A-BANNER-THAT-DENIES-A-CAPABILITY-THE-PROGRAM-HAS -- FIXED 2026-09-15

**In short:** three apps told the user "this app has no filesystem access, so
anything you write here is gone when the window closes" on the same day they
each gained a working Save. The sentences were true when written and were made
false by the fix. They are corrected, and there is now a checker for the class,
because adding a capability is exactly what creates it.

### The class

Two of the checkers in `scripts/` look for a program saying **too much** (data
it invented) and saying **too little** (an empty screen with no explanation).
Neither can see a third thing: a program that says something that *used to be
true*.

**A banner that denies a capability the program has is the same defect as one
that claims a capability it lacks, pointed the other way.** Both leave the user
believing something about the program that is not so.

It is the more expensive direction of the two. A false promise is discovered by
trying it -- the user presses the button, nothing happens, and now they know. A
false denial is not discovered at all: the user reads "this app cannot save",
believes it, and never presses anything. **The feature might as well not have
been built**, and no bug report is ever filed, because from where the user sits
nothing is broken.

### What was found

`apps/dbviewer` prompted it. Its sidebar carried a constant written carefully
and honestly for the purpose:

    No database open -- this program cannot open one
    It has no filesystem access and no database driver, so nothing was read

True the day it was written. False from the moment Import could reach a file.
Nothing would have caught it: the fixture scanner sees no invented data here,
and the other two scanners skip any crate that *has* a door.

`scripts/find-stale-admissions.py` was written for the class and immediately
found three more, all of them mine, all from the same day's work:
`apps/calendar`, `apps/contacts` and `apps/spreadsheet` had each just gained a
file dialog and each still said "this app has no filesystem access".

### The fix, and what it preserved

The warning underneath those banners was *real and still is*: none of these
apps autosaves, so closing the window loses the work. That half stays. What
changed is that the sentence is now true and names the remedy:

    Nothing is saved automatically -- press Ctrl+S to write a vCard file,
    or anyone you add is gone when the window closes

`dbviewer`'s constants were renamed `NO_TABLES_*`, because the old name was
also wrong about when the line shows: it appears for a freshly created empty
database too, which *is* open.

### The test that held the wrong wording in place

`apps/contacts` asserted that some banner line contained the words `"Nothing is
saved between runs"`. That assertion stayed green through the entire
regression. The words were still there; they were simply no longer true.

**A test that pins wording keeps passing for exactly as long as the wording is
wrong.** It now asserts the property -- that the warning names the remedy
(`Ctrl+S`) -- which is what has to hold for the sentence to do its job.

### Why this needs a tool and not a habit

Every door added to this tree manufactures a fresh opportunity for it. Six apps
gained file access in one sweep and half of them acquired a false banner in the
same commit that made them useful. Remembering to re-read the empty-state text
after adding a capability is exactly the kind of discipline that works until
the day it matters.

`python scripts/find-stale-admissions.py --roots=apps,gui`

It pairs capabilities with denials rather than matching both loosely -- a crate
that reads files and truthfully says it cannot reach the network is not a
finding. Denials carrying a format placeholder are counted but not printed:
`"cannot read {path}: {err}"` is the program reporting what just happened,
which is the opposite of a claim about what it can ever do.

**Report-only, like the others, and for the same reason:** entries are
legitimate often enough that a gate would train the next reader to silence it
rather than read it. One report stands today, `gui/compositor`'s "the mode-set
the kernel would have refused was never sent", which is a display mode-set and
not a packet. It is named in the script's own docstring so nobody investigates
it twice.
