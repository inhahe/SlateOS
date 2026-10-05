## D-SPIKES-BASH-CROSS-CONFIGURE-GUESSED-WHAT-IT-COULD-NOT-RUN — bash's configure answers seventeen questions by running a test program; cross-compiled it guesses instead, and some guesses cost bash features on SlateOS (lane D, 2026-10-01)

**Status:** OPEN — four of bash's configure guesses need a fact about SlateOS checked

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
