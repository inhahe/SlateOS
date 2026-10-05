## D-SPIKES-BASH-CROSS-CONFIGURE-GUESSED-WHAT-IT-COULD-NOT-RUN — bash's configure answers seventeen questions by running a test program; cross-compiled it guesses instead, and some guesses cost bash features on SlateOS (lane D, 2026-10-01)

**Status:** FIXED 2026-10-05

**In short:** bash's build asks some questions about the system by
compiling and *running* a small program -- does `printf` know `%a`, are
named pipes usable. Built on Linux for SlateOS, it cannot run them, so it
guesses, and a wrong guess quietly turns a feature off or swaps in a
weaker substitute. Two were wrong in ways that mattered once bash's link
kept its own order, and are answered now (`getcwd`, `mktime`); four more
need a fact about SlateOS checked before they can be.

**The answers configure guessed** (`config.log` of
`scripts/bash-spike/cross2.sh`'s build, 2026-10-01), against what Linux
measures:

| variable | guessed | what it costs | to answer it, check |
|---|---|---|---|
| `bash_cv_printf_a_format` | no | bash's `printf` builtin refuses `%a`/`%A` | our `printf` formats `%a` as glibc's does |
| `bash_cv_sys_named_pipes` | missing | process substitution leans on `/dev/fd` alone | SlateOS's `mkfifo` and FIFOs |
| `bash_cv_unusable_rtsigs` | yes | `trap` and `kill` lose `SIGRTMIN`..`SIGRTMAX` | real-time signals are queued and deliverable |
| `bash_cv_wexitstatus_offset` | 0 (Linux: 8) | `wait`'s status decoding where bash does it by hand | our `<sys/wait.h>` encoding, which is Linux's |
| `bash_cv_getcwd_malloc` | no | bash's own `getcwd`, wrong under procfs | answered `yes`, 2026-10-01 |
| `ac_cv_func_working_mktime` | no | bash's own `mktime` compiled in | answered `yes`, 2026-10-01 |
| `bash_cv_dev_fd`, `bash_cv_dev_stdin` | standard, present | nothing, if SlateOS has `/dev/fd` -- process substitution if not | these two are read from the *build* machine's `/dev`, not guessed |

The other guesses (`dup2`, `opendir`, `ulimit`, `strcoll`, `fnmatch`,
`sigsetjmp`, signal reinstalling, `WCONTINUED`, `sys_siglist`, the pipeline
process group) are the answers a Linux build measures, or concern features
SlateOS does not have either way.

**The proper fix:** for each, find SlateOS's true answer -- a ring-3 rung
where one is needed -- and pass it to `configure` in `cross2.sh` beside the
two already there, with the measurement in its comment. Then rebuild bash
(`cross2.sh`, `cross3.sh`, `slatelink.sh`).

**Fixed (2026-10-05):** the six are answered in `scripts/bash-spike/cross2.sh`,
each with what it was measured from in the comment, and bash rebuilt. The
measurement is the cross build's own bash -- the SlateOS objects, linked with
musl -- run under WSL before and after, whose wait status words and `printf`
are Linux's and glibc's as ours are:

| variable | answer | before | after |
|---|---|---|---|
| `bash_cv_wexitstatus_offset` | 8: our wait status is Linux's (`posix/src/process.rs`) | `shopt -s lastpipe; true \| false; echo $?` said **129**, `true \| (exit 3)` 131 | 1 and 3 |
| `bash_cv_printf_a_format` | yes: our `%A` and long-double `%LA` are glibc 2.39's (`printf.rs`, its oracle) | `printf '%a' 1`: "invalid format character" | `0x1p+0` (musl's form; glibc's, which ours is, writes a long double's `%La` as `0x8p-3`) |
| `bash_cv_unusable_rtsigs` | no: the test asks only that `SIGRTMIN` (ours 32) be under `2*NSIG` | `kill -l RTMIN`: "invalid signal specification" | `35` under musl -- `32` on SlateOS -- and `kill -l 64` is `RTMAX` |
| `bash_cv_sys_named_pipes` | present: bash's own test answers from `mkfifo` existing | process substitution had no FIFO route | a FIFO (`/tmp/sh-np.*`) where `/dev/fd` is absent; on SlateOS `mkfifo` answers `ENOSYS` today, so process substitution says so |
| `bash_cv_dev_fd`, `bash_cv_dev_stdin` | absent: a native program has no `/dev/fd`, and its `/dev/stdin` and kin are the console (`kernel/src/fs/devfs.rs`) | `echo x > /dev/stderr` reached the console, not fd 2 | bash opens `/dev/stderr`, `/dev/stdout`, `/dev/stdin` and `/dev/fd/N` itself, as the descriptors |

The first was a live bug on the image: `lastpipe` is how a pipeline's last
command runs in the shell itself, and its status was being read as a death by
signal. What the last pair leaves -- process substitution has neither
`/dev/fd` nor a FIFO to work with on SlateOS today -- is
`known-issues/D-POSIX-NATIVE-PROGRAMS-HAVE-NO-DEV-FD.md`. The other guesses in
the paragraph above stand as they were. bash's link still has nothing
undefined and nothing duplicated.
