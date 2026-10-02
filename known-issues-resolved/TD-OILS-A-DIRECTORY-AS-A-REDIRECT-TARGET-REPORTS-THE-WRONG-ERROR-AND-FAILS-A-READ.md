### TD-OILS-A-DIRECTORY-AS-A-REDIRECT-TARGET-REPORTS-THE-WRONG-ERROR-AND-FAILS-A-READ — 2026-08-03 — ✅ **FIXED 2026-08-05**

**Where:** `userspace/oils/src/interp.rs`, `open_error` and the input-open path
(`Shell::open_input_source`, and the `RedirectOp::Read` planner).

**What.** Two divergences, one cosmetic and one real, both from opening a
directory:

```
$ mkdir ad
$ bash --norc -c 'echo x > ad'      $ osh -c 'echo x > ad'
bash: ad: Is a directory            osh: ad: Permission denied

$ bash --norc -c 'cat < ad'         $ osh -c 'cat < ad'
cat: -: Is a directory              osh: ad: Permission denied
```

Win32 answers `CreateFile` on a directory with `ERROR_ACCESS_DENIED`, which Rust
maps to `PermissionDenied`, so the message is wrong. The read case is worse than
a message: bash's `< ad` *succeeds* — a directory opens for reading, and it is
the reading command that then fails — where osh fails the redirection itself,
which is a different exit status and a different stream for the diagnostic.

**Fixed 2026-08-03 — the message.** `open_error` sits on the failure path of
every open (`open_out`, `open_rw`, `Shell::open_in_target`) and turns the one
ambiguous kind — `PermissionDenied` on a path that `is_dir()` — into
`IsADirectory`, so `>`, `>>`, `>|`, `&>`, `>&`, `2>`, `<>` and all of their
`exec` forms now report `Is a directory` exactly as bash does. A real permission
error is untouched, and on a POSIX host the correction never fires because the
kernel already said `EISDIR`.

`.`/`source` needed its own check rather than the corrected message: bash calls
`file_isdir` before reading and prints a *different* string for it — labelled
with the builtin and with a lower-case "is" (`.: ad: is a directory`) — and that
refusal is an ordinary failure, not the failed open that ends a non-interactive
posix shell. Covered by `a-directory-is-not-a-file-a-redirect-can-open.sh`.

**Fixed 2026-08-05 — the read.** `< ad` now *opens*, and fails at the first
read. `Shell::open_in_target` became `open_input_source`, returning an `InputFd`
rather than a `File`, and answering a path that `is_dir()` with a new
`InputSrc::Directory` whose `Read`/`BufRead` impls yield `ErrorKind::IsADirectory`.
Because it is a new enum arm, every exhaustive match on `InputSrc` — `read`,
`fill_buf`, `consume`, `Debug`, `child_input` — was a compile error until it had
been considered, which is exactly why a variant was preferred to a flag.

That one shape is enough for the whole family, because everything downstream
already routes through the descriptor rather than the redirect: `exec < ad` and
`true < ad` are status 0, `read < ad` says `read: read error: 0: Is a directory`
at status 1, `read -u 3` names fd 3 where a `<&3` copy still names 0, a dup
(`exec 4<&3`) carries the fact, `1< ad` puts it on the write descriptor,
`while read; done < ad` runs its body zero times, `mapfile` treats it as end of
input (status 0, empty array), and `$(< ad)` stays empty at status 0.

**Not fixed, and cannot be:** an **external** command reading the descriptor.
bash's `cat < ad` fails inside `cat`, with whatever the host's `cat` makes of a
directory handle, and `child_input` has no real handle to hand over — Win32
needs `FILE_FLAG_BACKUP_SEMANTICS` even to open one, and reads through the
result still answer `ERROR_ACCESS_DENIED` (`Permission denied`), which is not
what bash says. So a directory bound to a child's fd 0 is passed as closed, the
same approximation a write-only fd 0 already gets, and the corpus case keeps to
the shell's *own* readers where the answer is exact. On the SlateOS target the
question does not arise: a directory opens for reading and reads answer `EISDIR`
from the kernel, so the variant merely names what would have happened anyway.

Pinned by the extended corpus case
`a-directory-is-not-a-file-a-redirect-can-open.sh` (which now covers both
halves) and the unit test `input_redirect_of_a_directory_opens_and_fails_the_read`.

**Standing lesson:** a host limitation is a reason to *model* a behaviour, not a
reason to skip it. The entry deferred this because Windows could not supply a
handle that behaves — but the behaviour that mattered was never the handle, it
was what the shell's readers say, and that is entirely the shell's own code. Ask
what the host actually blocks: usually it is one narrow edge (here, a child
process's view), not the feature.
