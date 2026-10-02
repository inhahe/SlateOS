## 718. `sh`'s descriptor table is a map of overrides the shell never installs, and an `exec` redirection is scoped to the construct that ran it

**Date:** 2026-08-30 · **Decided by:** Claude (autonomous)
**Lane:** B

**In short:** When a shell runs `echo hi > f`, something has to make `echo`'s
output land in `f`. A shell with `fork` does it by really pointing descriptor 1
at the file in the child, which is safe because the child is thrown away
afterwards. Our builtins run *inside* the shell process, so doing that would
leave the shell itself writing to `f` forever. Instead redirections are carried
as a *table* — "for this command, 1 means this file" — that the real descriptors
never see. Two consequences had to be decided: what a table means for a child
process, and what `exec 2>log` (a redirection with no command, which is supposed
to stick) does to it.

### Decision 1: a table of overrides, not `dup2` on the shell's own descriptors

`Io` is a `BTreeMap<i32, Fd>` consulted by every builtin and translated into
`Stdio` values for a child. The shell's own descriptors are never redirected.

*What changes:* nothing a script can see; it is what makes `echo hi > f; echo
hi` print once and write once, rather than writing twice.

- **For:** correctness without save-and-restore of real descriptors — and the
  host build *cannot* save and restore, its stdout not being a descriptor at
  all. It also makes an `Io` cheap to clone, which the pipeline and subshell
  paths do constantly.
- **Against:** every builtin has to be written to consult the table rather than
  to just print, and two POSIX behaviours become approximations: `n>&-` hands a
  child the null device rather than a closed descriptor, and descriptors above
  2 reach an external child only on unix, where `extra_fds` can install them
  between fork and exec.
- **Why the "for" wins:** the approximations are each observable only by a
  program that tests for `EBADF` on a descriptor the shell deliberately shut,
  which no case in ~225 could construct; the alternative is wrong for every
  builtin on every run. Both gaps are in the module header.

### Decision 2: `exec`'s table is scoped to the construct that ran it, not global

`Shell::exec_io` is cleared on entry to any construct that has redirections of
its own and restored on the way out (`Shell::in_redir_scope`), so `Some` means
"an `exec` happened *in this scope*" and `Shell::io_now` needs no generation
counter.

*What changes:* `{ exec 2>inner; nosuchcommand; } 2>outer` writes the message to
`inner`, matching dash, where a global table put it in `outer`.

- **For:** it is what a real shell's save/`dup2`/restore *does*, expressed
  directly. The rule it replaces was an attempt to state a static precedence
  between `exec`'s redirections and a construct's own — and no static precedence
  is right, because an `exec` *before* a group must lose to the group while one
  *inside* it must win. That is ordering, and ordering is scope.
- **Against:** it is one more thing every execution path has to remember to do,
  and forgetting it is silent: the wrong file gets the output.
- **Why the "for" wins:** the failed alternative is in the history. An overlay
  merge (`merged_io`) was tried first and got the boundary case backwards, and
  a generation counter was the next idea — more state to keep in step, for a
  question the scope already answers. The harness has six cases pinning the
  boundaries, including a subshell one, so the silence is covered by a test.

### Decision 3: a bad descriptor is fatal when it is *unparseable*, not when it is merely closed

`cat <&9` on a closed descriptor fails the command with status 2 and the shell
continues; `cat <&notanumber` kills a non-interactive shell. `fd_is_open` probes
with `dup` + `close`.

*What changes:* `cat <&9; echo $?` prints `2` and keeps going; `cat <&x; echo
unreached` prints nothing after the error.

- **For:** it is dash's line, measured rather than guessed, and it is a sensible
  one: a non-numeric target is a *syntax* error the parser could in principle
  have caught, while a closed descriptor is a runtime condition a script may
  legitimately provoke and check.
- **Against:** the split looks arbitrary read from POSIX alone, which calls both
  a redirection error.
- **Why the "for" wins:** matching the reference shell is the whole point of
  this file, and the harness is how we know what the reference does.
  `dup`+`close` is the probe rather than `fcntl(F_GETFD)` because it needs no
  `fcntl` binding and no `F_GETFD` constant for a target whose libc headers are
  ours; it costs a descriptor for the length of one call and answers the same
  question.
