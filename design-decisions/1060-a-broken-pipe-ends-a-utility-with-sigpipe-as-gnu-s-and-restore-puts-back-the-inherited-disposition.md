## 1060. A broken pipe ends a utility with `SIGPIPE`, as GNU's -- `stdfd::restore` puts back the disposition it inherited (supersedes §377)

**Date:** 2026-10-07
**Lane:** B
**Decided by:** Claude (autonomous) -- revisiting §377, also Claude's, after
lane D's §1176 and `requests/d-b-sigpipe-is-raised-now.md`

**In short:** in `seq 1 1000000 | head -1`, `head` leaves after one line, and
`seq`'s next write goes into a pipe that nobody reads. On Linux, GNU's `seq` is
then ended by a signal, `SIGPIPE`: it prints nothing, and the shell records
status 141. Ours got an error back instead, said nothing, and exited 0, because
SlateOS sent no such signal (§377). SlateOS sends it now (§1176), so our
utilities let it end them as GNU's are ended. And when someone has told the
shell to ignore the signal (`trap '' PIPE`), our utilities now report the
failed write as GNU's do, where before they stayed quiet.

### What a user sees

| Command | Before | Now, and GNU |
|---|---|---|
| `yes \| head -1` | `yes` exits 0 | `yes` is ended by `SIGPIPE`: nothing printed, status 141 |
| `trap '' PIPE; yes \| head -1` | `yes` exits 0, prints nothing | `yes: standard output: Broken pipe`, status 1 |
| `trap '' PIPE; seq 1 1000000 \| head -1` | `seq` exits 0, prints nothing | `seq: write error: Broken pipe`, status 1 |
| `set -o pipefail; seq 1 1000000 \| head -1` | pipeline status 0 | 141 |
| `yes \| tee --output-error=warn f \| head -1` | unchanged | unchanged: `tee` ignores the signal itself in that mode, as upstream does |

### Options

| | Approach | *What changes* |
|---|---|---|
| **A** *(chosen)* | `stdfd::restore` puts `SIGPIPE` back to the disposition the program inherited, undoing the Rust runtime's "ignored" | the table above: GNU's behaviour under both dispositions |
| **B** *(§377, until now)* | Keep the runtime's "ignored", and answer `EPIPE` quietly with the status earned so far | `yes \| head -1` exits 0; under `trap '' PIPE` nothing is reported where GNU reports the error |

### Why A

**§377 rejected A for one reason, and that reason is gone.** It said A was
"correct only on the host we test on": the target raised no `SIGPIPE`, so the
Linux harnesses would have measured a behaviour the shipped programs never
had. Lane D's C library now raises `SIGPIPE` where Linux's kernel would
(§1176), and with the default disposition it ends the program with status 141
(`posix/src/signal.rs`, `apply_default_action`). SlateOS's userspace target is
`os = "linux"`, `env = "musl"` over that C library
(`toolchain/x86_64-slateos.json`), so `stdfdguard`'s Linux code is the code the
target runs. The harnesses measure the shipped configuration again.

**B cannot be faithful in one of the two cases.** Under `trap '' PIPE`, GNU
reports the failed write and exits 1; B said nothing and exited 0. A matches
GNU under both dispositions, because it does what GNU does: nothing, leaving
the disposition as the program found it.

**§377's other objection is answered by upstream.** It noted that `tee`, `cmp`,
`sed` and `tar` need the `EPIPE` in hand, so under A "each would have to
re-mask the signal around its own writes". Upstream masks it in exactly two
places, and those are the only two that do so here, through
`stdfd::ignore_sigpipe`:
- `tee` in its non-default `--output-error` modes;
- `split --filter` (`default_SIGPIPE`).

`cmp`, `sed` and `tar` do not mask it upstream. They die of it there, and they
die of it here now.

### How

- `stdfdguard`'s constructor already recorded `SIGPIPE`'s inherited disposition
  before the runtime replaced it, for `split --filter`'s `default_SIGPIPE`.
  `restore()` now puts it back too, and `sigpipe_restored()` reports that it
  did.
- `stdfd::reader_gone(e)`, §377's predicate, is now true only where the signal
  was *not* put back:
  - a utility that does not yet expand `guard_std_fds!` and call `restore`
    (the list in `known-issues/TD-COREUTILS-AN-UNWRITABLE-STDERR-ABORTS-THE-PROCESS.md`);
  - a build where `stdfdguard` does nothing (the Windows development host).

  There §377's convention still holds, so such a utility stays quiet rather
  than starting to print `write error: Broken pipe`. It gains GNU's behaviour
  when it gains the guard.
- The hand-rolled `ErrorKind::BrokenPipe` checks in `yes`, `uniq`, `sed`,
  `tail` and `stat` now ask `reader_gone`, so each follows the rule above.
- **procps `ps` is not GNU.** It installs a `SIGPIPE` handler that exits 0,
  whatever disposition it inherited, so `ps | head` is status 0 even under
  `trap '' PIPE`. Ours keeps that: after `restore` it ignores the signal
  itself, and answers `EPIPE` with status 0 directly.

### What it costs

- **A program that needed to clean up is now killed first.** Where upstream
  traps the signal to clean up, ours must too. Audited:
  - GNU `sort` traps it to delete its temporary files. Ours keeps everything in
    memory and makes none.
  - `tar` refuses `--to-command` and `--use-compress-program`, so it runs no
    child whose disposition would matter.
- **Children.** Rust's `Command` resets `SIGPIPE` to its default in every
  child. `split --filter` and `timeout` already hand their commands the
  disposition they themselves inherited (`sigpipe_ignored_at_startup`), as
  upstream does.

### Verified

`scripts/write-error-diff.sh` gained two shapes, `pipe` (`SIGPIPE` at its
default) and `pipeign` (ignored), each applied to fifteen utilities whose
output far exceeds a pipe's buffer. `yes-diff.sh` now compares the status of
every endless case, and `tee-diff.sh` runs its default-mode pipe cases under
both dispositions. Each had recorded the old behaviour as an expected
difference, and none does now.
