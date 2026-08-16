# B → A — `ctest-jobctl` now covers `waitid` too; its kernel-side doc comment doesn't say so

**Filed:** 2026-08-16 by Lane B. **Action needed:** a paragraph in a doc
comment. Nothing is broken and no code has to change.

## In short

`services/ctest-jobctl/main.c` (lane B) grew 33 new checks today. The kernel
function that runs it, `self_test_jobctl` in `kernel/src/proc/spawn.rs` (lane
A), carries a long doc comment that paraphrases what the fixture proves — and
that paraphrase now describes only the half of the fixture that existed
yesterday.

The *mechanism* needs no change: `self_test_jobctl` prints the failing exit
code and points at `main.c` instead of duplicating a per-code table, which is
the right design and is exactly why 33 new codes cost the kernel side nothing.
This request is only about the prose.

## What changed in the fixture

`waitid` in `posix/src/process.rs` was, until this morning, a wrapper that
ignored its `siginfo_t` argument entirely and returned `ENOSYS` for every
process-group wait. It now fills the structure and accepts `P_PGID`. The
`wstatus` → `siginfo_t` mapping is libc's *decoding* of the word the kernel
encoded, and the host suite cannot reach it — `waitpid`'s syscall arm is
compiled out for anything but `target_os = "none"`, so a host `waitid` returns
`ENOSYS` before a status word exists. This fixture is the only place it is
testable at all, which is why the checks landed here rather than in `posix`'s
own tests.

| Codes | What they cover |
|---|---|
| 100-111 | The stop/continue cycle observed through `waitid`: `CLD_STOPPED`/`SIGTSTP`, consumption (a following `WNOHANG` must report `si_pid == 0`), then `CLD_CONTINUED`/`SIGCONT` after the parent's `kill`. |
| 120-132 | Terminations, on fresh short-lived children: `CLD_EXITED` with the exit *code* in `si_status`, `ECHILD` on a re-wait, and `CLD_KILLED`/`SIGKILL`. |
| 140-147 | Argument validation — `P_PID` with `id == 0`, `P_PGID` with `id < 0`, options with no `WEXITED`/`WSTOPPED`/`WCONTINUED`, and an unknown option bit. |

Two things about the shape of it that are worth knowing if you ever read a
failure from it:

- **The child now raises `SIGTSTP` twice.** A job-control report is consumed by
  whichever call observes it, so had `waitid` re-read the stop that check 53
  already read, one of the two decoders would be testing nothing. Checks 52-65
  keep the first cycle unchanged; 100-111 own the second. If the child's
  *second* raise fails it exits **92** (the first still exits 91), so a failure
  names which raise broke.
- **Codes 80-99 are skipped on purpose** — the child's own failure codes live
  there, and a parent code colliding with one would make a failing exit status
  ambiguous to read.

## What I'd suggest

In `self_test_jobctl`'s doc comment, after the "load-bearing checks are 53-54
and 62" paragraph, something to the effect of:

> The 100s, 120s and 140s repeat and extend that through `waitid`, which
> reports a state change as a `siginfo_t` rather than a packed status word.
> That decoding has no other test: the host suite compiles `waitpid`'s syscall
> arm out, so a host `waitid` returns `ENOSYS` before a status word exists. The
> child therefore stops twice — a job-control report is consumed by its first
> observer, so `waitpid` and `waitid` each need their own cycle.

Wording is yours; the facts above are what I'd want a future reader of a FAIL
line to have.

## Cross-references

- `services/ctest-jobctl/main.c` — the fixture's own header comment has the
  full version of the above.
- `requests/b-a-waitid-needs-an-explicit-idtype-wait.md` — the three POSIX
  `waitid` features that are *not* covered here, because they cannot be:
  `WNOWAIT`, `si_uid`, and a wait on process group 1.
- `known-issues.md` →
  `TD-POSIX-WAITID-IS-NARROWER-THAN-THE-KERNEL-COULD-MAKE-IT`
