### TD-OILS-A-FAILED-EXEC-REDIRECTION-NEITHER-STOPS-THE-LIST-NOR-UNDOES-IT. `exec 4>e1 3>&9 5>e2` binds fd 4 and opens `e2` where bash does neither — 2026-08-10 — ✅ FIXED 2026-08-10

**Where:** `userspace/oils/src/interp.rs`, `Shell::apply_exec_redirects` — the
loop that walks a redirection-only `exec`'s redirects in source order. On a
failure it set `rc = 1` and *continued*, and it never put back what the earlier
redirects in the same list had already bound.

**Reproduce.**

```sh
exec 4>e1 3>&9 5>e2
echo "rc=$?"
echo four >&4        # bash: 4: Bad file descriptor     osh: written to e1
ls e1 e2             # bash: e1 only                    osh: both
```

| | bash 5.2.37 | osh (before) |
|---|---|---|
| `e1` opened | yes | yes |
| `e2` opened | no — the list stopped | yes |
| fd 4 afterwards | unbound (undone) | bound to `e1` |
| `exec 3>/dev/null; exec 3>&- 4>&9` → fd 3 | still open (close undone) | closed |

**What bash does.** `do_redirections` returns at the first error, so nothing
after it is performed (redir.c:260-271); `execute_builtin` then runs the undo
list that `add_undo_redirect` / `add_undo_close_redirect` built as each redirect
was applied (execute_cmd.c:5457-5465). The undo is a *restore* — the descriptor
was dup'd aside before being overwritten — so a previously-bound fd goes back to
what it held, an unbound one is closed, and one the list closed is reopened.

Two things survive it, both measured:

* a `{name}` allocation, because bash saves the redirector it *just assigned*
  and so restores it to itself (redir.c:959-962); `$name` keeps its number
  either way. Only `varredir_close` takes the `add_undo_close_redirect` branch
  and really closes it.
* everything a *fatal expansion* error left behind — that unwinds rather than
  returning an error code, so the undo list is discarded unrun.
  `exec 4>s1 3>"/dev/null${z:-'$(fi)'}" 5>s2` leaves fd 4 bound. osh already
  matched this one (the `break` in the loop).

**The fix — done.** `Shell::save_exec_fds` copies the persistent fd table into
an `ExecFdTable` before the loop and `Shell::restore_exec_fds` puts it back on
the first redirection failure, leaving the `{name}` allocations the list made
as they stand. The table is every field a persistent redirect can bind —
`exec_stdin`/`exec_stdin_write`, `exec_stdout`, `exec_stderr` with its
`exec_stderr_depth`, and the `open_fds`/`open_write_fds` maps — and copying it
whole is cheap, every entry being an `Arc`.

**Tests.** `tests/corpus/a-failed-exec-redirection-stops-the-list-and-undoes-what-it-did.sh`.
