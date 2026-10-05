## 719. `sh`'s diagnostics follow POSIX's wording and the current redirections, and the differential harness normalises only dash's side

**Date:** 2026-08-30 · **Decided by:** Claude (autonomous)
**Lane:** B

**In short:** A shell's error messages are an interface: scripts grep them, and
the harness that certifies this shell against `dash` compares them byte for
byte. Three things had to be settled — where a message goes when the script has
redirected stderr, what words it uses, and what the harness is allowed to ignore
when the two shells disagree about wording.

### Decision 1: a thread-local "current stderr", not a parameter on every message

`CURRENT_ERR` is a `thread_local!` holding the `Fd` diagnostics go to, and
`ErrScope` sets and restores it as execution enters and leaves each construct.
The local `diag!` macro writes through it.

*What changes:* `nosuchcommand 2> err` puts the not-found message in `err`,
where before it went to the terminal and `err` was empty.

- **For:** the alternative is threading an `&Io` into every function that can
  fail, including expansion and arithmetic, which are recursive and many layers
  deep — and a single missed thread is a message in the wrong file. `sed.rs`
  already sets this precedent in this crate.
- **Against:** it is global mutable state, and an `ErrScope` that is dropped out
  of order leaves the wrong destination installed.
- **Why the "for" wins:** the scope guard is RAII, so out-of-order is not
  reachable without going out of one's way, and the harness compares the *files*
  a case left behind as well as its streams — which is exactly the check that
  catches a message written to the wrong place.

### Decision 2: POSIX's `strerror` text, not the host's and not dash's

`coreutils::errmsg::strerror` supplies the words: `File exists`, not Windows's
`The file exists. (os error 80)`; and `No such file or directory`, not dash's
abbreviated `No such file`.

*What changes:* a failed redirection reads the same on the host build as on
SlateOS, and reads the way every other program on the system reads.

- **For:** an error message that changes with the build host is untestable.
  Against dash specifically: dash carries a *private* abbreviated table, and
  copying it would make our messages disagree with our own `strerror` and with
  every other binary in the tree.
- **Against:** a script that greps for dash's exact text does not match ours.
- **Why the "for" wins:** such a script is already unportable — bash, ksh and
  busybox all say `No such file or directory` too. The same reasoning settles
  `cd`: a failed `cd` names the errno (`cd: /nonexistent-dir: No such file or
  directory`) the way bash does, rather than dash's `cd: can't cd to …`, and
  `echo $(( ))` is `0` as in bash and ksh where dash calls it a syntax error.
  Both are in the harness's "differ on purpose" list, so the difference is
  recorded rather than drifted into.

### Decision 3: the harness normalises two fields, on dash's side only

`scripts/sh-diff.sh` strips dash's `sh: <line>: ` prefix and expands dash's
abbreviated `strerror` before comparing. Everything else — stdout, exit status,
stderr, and every file the case left on disk — is compared byte for byte.

*What changes:* the harness can be green while our messages still differ from
dash's in exactly those two respects, and in no other.

- **For:** neither field is comparable. The line number is dash's position in
  *its* input, which for a `-c` string is meaningless; the abbreviation is
  settled by decision 2 above. Every other byte stays under test, including the
  disk state, which is what catches a diagnostic written to the wrong file.
- **Against:** a normaliser is a hole in a differential test by construction —
  it is the one place a real difference can hide.
- **Why the "for" wins:** the hole is bounded by two things. The rules are
  keyed on *which side is dash*, not on which side is the reference, so
  `OURS=/bin/dash` runs the harness against itself as a self-check — and that
  self-check is the reason the keying is that way: a rule that fired on "the
  reference" only would make the self-check report every diagnostic case as a
  difference and thereby useless. And an early side-blind version of the rule
  was caught eating *our* descriptor number out of `sh: 3: Bad file descriptor`,
  which is precisely the class of bug the objection predicts; it was found in
  one run because the self-check exists.
