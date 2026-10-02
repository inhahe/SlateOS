## 739. The numbered-backup retry loop stops when it stops making progress, which gnulib's does not — a directory you can write but not read makes upstream spin forever

**Date:** 2026-08-31
**Decided by:** Claude (autonomous)
**Lane:** B

**In short:** `cp -b` keeps the previous contents of a file it is about to
overwrite by moving them aside under a new name — `f.~1~`, `f.~2~` and so on.
It picks the number by listing the directory, and then has to cope with another
process taking that name in the instant between the listing and the move. GNU
copes by trying again, on the reasoning that a fresh listing will show the new
name and pick a higher number. That reasoning quietly assumes the directory can
be listed. In a directory you are allowed to write but not to read — a drop box,
mode `733`, which is a real and deliberate configuration — the listing shows
nothing, so every attempt picks `f.~1~`, and GNU's `cp -b` hangs until it is
killed. Ours makes one extra check: if the retry produced a name it has already
tried, it stops and reports the collision. Everywhere GNU terminates, both do
the same thing.

### The situation

`backupfile_internal` (`gnulib/lib/backupfile.c:390`) is a `while (true)` whose
only exits are a successful rename and a failure that is not "the destination
exists". The retry condition is `e == EEXIST && extended`, where `extended`
means the chosen name was not shortened to fit the filesystem's length limit —
a shortened name cannot be varied, so that case falls out. An *unshortened*
name is assumed to be variable, because `numbered_backup` will next time see
whatever took it and count past it.

The assumption holds only if `opendir` succeeds. When it does not,
`numbered_backup` returns `BACKUP_IS_NEW` and the name is `f.~1~` — the same
`f.~1~` it returned last time, and will return next time. `rename` keeps
answering `EEXIST`, `extended` stays true, and the loop never advances.

Three things have to line up: `--backup=numbered` or `--backup=existing` (the
default `--backup` control, once one numbered backup exists), a destination
directory the caller can write but not read, and something already occupying
`f.~1~`. That is not a contrived stack — mode `733` upload directories exist
precisely so that writers cannot enumerate each other's files, and the whole
point of a backup suffix is that its name is predictable.

### The decision

`Backup::build` remembers the name the previous attempt tried and treats a
repeat as failure, returning `AlreadyExists` — which reaches the user as
`cp: cannot backup 'f': File exists` and exit 1.

The check is on *progress*, not on the readability of the directory. Stating it
that way is what makes it safe: it cannot fire on any run where upstream would
have terminated, because upstream terminating means some attempt produced a
name that was free, and a name that was free is a name not equal to the last
one tried. It also covers cases the readability test would miss — a directory
that becomes unreadable midway, or any future scan that returns a constant for
a reason not yet imagined.

### Alternatives considered

- **Reproduce the loop exactly.** The default position everywhere else in this
  utility, and the reason it was not taken here is that the divergence is
  invisible: a program that hangs produces no output to differ from GNU's, so
  the differential harness could never see it either way. Fidelity is worth
  having where it is observable; matching an infinite loop is matching nothing.
- **Test the directory for readability up front and refuse.** Diagnoses the
  cause more precisely, and is wrong in two directions: it refuses runs that
  would have worked (nothing occupies `f.~1~`, so the first attempt succeeds),
  and it still misses a directory whose permissions change mid-run.
- **Cap the retries at some number.** Terminates, and picks an arbitrary
  constant that is simultaneously too small for a genuinely contended directory
  and too large to be reached quickly. The progress check needs no constant: one
  wasted attempt is the whole cost.

### Why this is a decision and not just a bug fix

Deviating from GNU is normally the wrong answer in this utility, and the
project's standing rule is to reproduce the reference rather than improve on
it — an "improvement" is how a difference nobody asked for gets in. The
counterweight is that this codebase already treats a reachable hang as a defect
in its own right (`lib.rs` records the same judgement about `find`'s `fnmatch`),
and a hang is not a behaviour a user can be bug-compatible with. The deviation
is written down at the code, at the loop, so that a later reader comparing
against `backupfile.c` finds the reason before concluding it is a porting
mistake.
