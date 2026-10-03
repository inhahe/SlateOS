## §376 — The WSL differential harnesses share one sourced preamble, not a copy of it apiece

**Date:** 2026-08-24
**Decided by:** Claude (autonomous)

**In short:** Each script under `scripts/` that compares one of our utilities
against the real GNU one has to do the same half-dozen chores before it can
compare anything: get itself inside WSL, find the repository, find the GNU
binary, build ours for Linux, fix the locale, and arrange for both binaries to
be reachable under one name. That was about fifty-five lines, written out
nine times, and roughly twenty-nine more harnesses are due to be moved onto the
same footing. The nine copies are now one file, `scripts/diff-wsl.sh`, which a
harness sources on its first line after setting a few named knobs.

### Why the chores exist at all

Two of them are load-bearing rather than tidiness, and both were learned from a
harness that was green while measuring the wrong thing.

**The reference has to be glibc's.** MSYS2 is a Cygwin derivative and its
`getopt` is not glibc's — `unknown option -- x` where glibc says
`invalid option -- 'x'`. A harness that compares against whatever `getopt` MSYS2
ships certifies wording that no GNU/Linux system prints. `sort-diff.sh` did
exactly that for eight cases and passed the whole time
(`known-issues.md` → TD-COREUTILS-GETOPT-DIAGNOSTICS-USE-THE-WRONG-SHAPE).

**The subject has to be a Linux binary.** Some of what these utilities do
exists only there. `coreutils::stdfd` — the module that makes `prog >&-`
behave (§375) — is `#[cfg(target_os = "linux")]`, because the two runtime
behaviours it undoes are undone with an `.init_array` constructor and raw
`write(2)`. A Windows build cannot execute a line of it, so a Windows-hosted
harness cannot catch a regression in it — which is the sweep currently in
progress.

So the chores are not incidental setup that a harness could reasonably skip or
approximate; each one is the difference between a measurement and a
green-looking non-measurement. That is the argument for having exactly one copy
of them.

### Options

| | *What changes* |
|---|---|
| **A. Leave each harness with its own copy** | Nothing today; a correction to the setup has to be made — correctly — in thirty-eight files, and a harness written next month starts from whichever copy was pasted |
| **B. A wrapper script that runs the harness as its child** | The setup runs once, but the harness is no longer a program you can run: it is a fragment invoked as `diff-wsl.sh cat-body.sh`, and everything the setup computed has to cross a process boundary as environment variables |
| **C. (chosen) A sourced preamble with named knobs** | The harness is still `sh scripts/cat-diff.sh`; its first two lines say `DIFF_PROG=cat` and `. "$(dirname "$0")/diff-wsl.sh"`, and the variables it needs are simply in scope afterwards |
| **D. Rewrite the family as one test binary** | The cases stop being shell and become Rust or Python — which loses the thing that makes them trustworthy |

### Why C

Against B: a harness needs *values* back from the setup — the repository root,
the built binary's path, the scratch directory, a helper function that maps a
binary name to its path. A child process can only return an exit status, so B
would have to serialise all of that into the environment and the harness would
have to trust it was set. Sourcing puts it in scope, which is what the harness
actually wants, and leaves each harness runnable on its own — which matters
because `all-diff.sh` runs them individually and because a failing one is
debugged by running just it.

Against D: these harnesses are trustworthy *because* they are shell. A case is
written the way a user would type it, with the shell's own quoting,
redirection and `$?` — including the cases whose entire subject is a
redirection (`>&-`, `>/dev/full`). Re-expressing `cat missing f >&-` as a
`Command` builder would be re-expressing the thing under test.

Against A, beyond the arithmetic: the copies had already drifted. Some
harnesses compared stderr as text and some only for presence; some built into
the shared Linux target directory of §374 and some did not; one had grown a
bespoke case runner for `getopt` diagnostics that skipped the stdout
comparison every other case performed. Drift is what a copied preamble does,
and none of those differences were decisions — they were the residue of which
harness was written when.

### The cost

The preamble is sourced, so it can set traps, and it sets one: an `EXIT` trap
that removes its scratch directory. A harness that sets its own `EXIT` trap
*replaces* that one rather than adding to it, and leaks the directory on every
run. There is no way to make shell warn about this, so the file's own
documentation states it and offers the alternative (extend `diff_cleanup`),
and every harness's fixtures were moved under the shared `$DIFF_TMP` so that
none of them wants a second trap in the first place.
