## ~~TD-B-FOUR-MORE-PROGRAMS-RUN-A-SHELL-AS-THE-WRONG-USER~~ (lane B, 2026-09-10) -- CLOSED the same day, and it was three, not four

**In short:** four programs still check who you are and then run the new user's
shell under the *old* user's identity. `su` was fixed on 2026-09-10; these were
found by the same look and are not yet done.

| Program | What it does today |
|---|---|
| `userspace/doas` | `exec_command` sets `UID`/`GID` *environment variables* as "hints" and calls no credential syscall. |
| `userspace/sudo` | No `setuid`/`setgid`/`CommandExt::uid` anywhere in `src/`. |
| ~~`userspace/sshd`~~ | **This row was wrong.** sshd has done it correctly all along -- see the correction below. |
| `userspace/login` | Its success path is still `eprintln!("login: would exec shell ...")`; it execs nothing at all yet. |

**The premise that expired.** `doas` carries the note that "the real privilege
change will use the kernel's capability system once the POSIX exec layer
supports `setuid`/`setgid` syscalls". That has fired: `posix::setuid` and
`posix::setgid` apply real credentials through `set_real_credentials` and
`getuid()` reflects them. They were stubs returning 0 once -- which is the
interesting part, because **a stub that reports success is what keeps a
deferral looking current**. Nothing about the note went stale in a visible way;
the capability arrived under it.

**What "wrong user" costs today.** Less than it sounds, and more than nothing.
Every process is uid 0 in the current model, so the shell was already root and
the switch was cosmetic either way. What changes with the fix is that the
switch becomes real *first*, so the code is correct before the model tightens
rather than after -- and `su`'s fix proves the mechanism works, which is what
makes the other four a conversion rather than a design.

**Done 2026-09-10.** `doas` and `sudo` now call
`authlib::identity::become_user`, `sudo` refusing outright when the target
account has no uid -- there is no safe default for "which user to run as".
`doas`'s `UID`/`GID` environment "hints" are deleted rather than kept alongside:
they were read as an *identity* by six other programs, so `doas` was
manufacturing the spoofed environment they trusted (see the entry above).
~~`login` remains, and it is a different job -- its success path still prints
"would exec shell" and execs nothing at all; see `todo.txt`.~~

**`login` is done too, and this line was stale when it was written or shortly
after (corrected 2026-09-12).** `build_login_command` builds a real
`process::Command` for the user's shell, clears the inherited environment,
sets the leading-hyphen `argv[0]`, and calls
`authlib::identity::become_user(&mut cmd, user.uid, user.gid)` -- the same
mechanism `doas` and `sudo` use. `spawn_login_shell` then tries the home
directory and falls back to `/`, which is the one setting that can fail for a
reason that is not the caller's fault. 62 tests pass.

There is no `todo.txt` entry for it either, so the pointer at the end of that
sentence led nowhere. Third stale status line found today in a document whose
own subject is stale status lines.

---

**CORRECTION: `sshd` never had this bug, and the row above was my error.**

`session_command` and `login_shell_command` have both been calling
`cmd.gid(user.gid)` then `cmd.uid(user.uid)` since they were written, under doc
comments that state the reasoning exactly: "sshd binds port 22 and therefore
runs as root; if it spawned a session without dropping to the authenticated
account, every user who could log in would get root". It even orders gid before
uid deliberately, with a comment explaining that `std` would order them
correctly anyway and that writing them this way saves the reader having to know.

**How the survey got it wrong**, because the mechanism matters more than the
mistake. The command was a `grep` for `\.uid(|\.gid(|setuid|...` piped through
a `grep -v` meant to drop *reads* of a record's fields -- `record.`, `user.`,
`target.` -- so that `user.uid()` lookups would not drown the signal. The lines
that prove sshd correct are:

    cmd.gid(user.gid);
    cmd.uid(user.uid);

They contain `user.`. The filter built to remove the noise removed exactly the
evidence, the program printed "NO uid/gid drop found", and that was written
into a tracking entry as a fact about the program.

This is the same defect this tree keeps finding in its own tooling: **a command
that answered a narrower question than the one being asked, whose answer was
then reported at the width of the question.** The grep answered "which lines
mention uid/gid and do not mention a record field", and it was reported as
"which programs drop privileges".

sshd is converted to `become_user` anyway -- not as a fix, but because it is the
fourth caller and the argument for the shared function is that the missing
`setgroups` must land in one place.
