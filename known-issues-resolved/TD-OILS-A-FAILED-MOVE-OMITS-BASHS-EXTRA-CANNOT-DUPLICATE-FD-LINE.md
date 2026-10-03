### TD-OILS-A-FAILED-MOVE-OMITS-BASHS-EXTRA-CANNOT-DUPLICATE-FD-LINE. A move from a bad descriptor prints one diagnostic where bash prints two — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/interp.rs` — the `RedirectOp::DupOut`/`DupIn` arm
of `Shell::resolve_one_redirect` and of `Shell::apply_persistent_redirect`, and
`Shell::report_redirect_failure`, which prints the *numbered* line.

**What.** When a move's source descriptor is not open, bash prints an extra,
**unnumbered** line before the ordinary one:

```text
$ bash -c 'true 1>&9-'
bash: redirection error: cannot duplicate fd: Bad file descriptor
bash: line 1: 9: Bad file descriptor
$ osh -c 'true 1>&9-'
osh: line 1: 9: Bad file descriptor
```

The numbered line, and the status, already match. The extra line is bash's
`add_undo_redirect` failing to save a descriptor for the restore list, and it
appears on a precisely measured condition — **the redirector fd is already
open, and the shell itself performs the redirection**:

| case | redirector open? | performed by | extra line |
|---|---|---|---|
| `true 1>&9-`, `true 0<&9-`, `true 2>&9-` | yes | shell | yes |
| `exec 5>&1; true 5>&9-` | yes | shell | yes |
| `exec 1>&9-`, `1>&9-` (null command) | yes | shell | yes |
| `true 3>&9-`, `true 5>&9-` | no | shell | **no** |
| `/bin/true 1>&9-`, `cat <&9-` | yes | forked child | **no** |

**Why those, and not others.** `redir.c`'s dup case has *two* `fcntl(F_DUPFD)`
calls on the dup's **source**, and each prints the line on its own failure:

* `add_undo_redirect (redir_fd, r_close_this, -1)` (`redir.c:1307`) saves the
  redirector so the close a *move* adds can be undone. It is guarded twice —
  `RX_UNDOABLE`, and `fcntl (redirector, F_GETFD, 0) != -1`, i.e. the
  redirector must already be open — which is exactly the measured table above.
  `exec` is **not** exempt: `execute_cmd.c:797` applies a simple command's
  list with `RX_ACTIVE|RX_UNDOABLE` like any other, and `exec` merely throws
  the undo list away *afterwards*. So `exec 1>&9-` prints it and
  `exec 3>&9-` does not, exactly as `true 1>&9-`/`true 3>&9-` do.
* `redirector = fcntl (redir_fd, F_DUPFD, SHELL_FD_BASE)` (`redir.c:1134`)
  allocates a `{v}>&N` / `{v}<&N` redirect's own descriptor. That save is how
  the redirect is *performed*, so it is not tied to keeping an undo list: a
  forked child prints it too, and a plain `{v}>&9` prints it as readily as a
  move does.

Both saves are of the source, so both fail exactly when the source is not
open — which is when the numbered line is raised. The extra line therefore
never appears alone, never accompanies `ambiguous redirect`, and never
attaches to a plain numbered `N>&M`.

**Fixed by** a `Shell::dup_save_note` field carrying a `DupSaveNote`
(`None` / `Always` / `WhenNotForked`) set at the dup site and consumed by the
failure reporter, which knows whether the command forks. `Shell::
dup_save_note_for` downgrades it to `None` for the two words that never reach
the `fcntl` — a close (no source to save) and a non-descriptor word (rejected
earlier as `ambiguous redirect`).

Corpus case:
`a-failed-dup-prints-an-extra-unnumbered-cannot-duplicate-fd-line.sh`.
