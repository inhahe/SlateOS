### TD-OILS-AN-EXEC-INSIDE-A-REDIRECTED-GROUP-COULD-NOT-REBIND-FD-0 — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/interp.rs` — `Shell::exec_with_redirects`, where
a compound command's own `< file` is installed for its body.

**What.** A group's `< file` was handed to the body *twice*: as the ambient
`Shell::exec_stdin`, and again as a `StdinSrc::Fd` argument. The argument wins
everywhere it is consulted, so it **froze** fd 0 for the body — an `exec` inside
the group could not rebind it, though `exec >` and `exec 2>` inside the same
group could, because fd 1 and fd 2 are only ever passed as
`exec_stdout`/`exec_stderr` overrides.

```text
$ bash -c '( exec 0<&-; read x; echo rc=$? ) < /dev/null'
bash: line 1: read: read error: 0: Bad file descriptor
rc=1
$ osh  -c '( exec 0<&-; read x; echo rc=$? ) < /dev/null'
rc=1                                    # silent: an EOF, not an error
$ osh  -c '( exec 0< f; read x; echo "x=$x" ) < /dev/null'
x=                                      # the `exec` was ignored outright
```

**Fixed by** passing `StdinSrc::Inherit` down when the group binds fd 0, so the
body resolves fd 0 through `exec_stdin` — the single binding an inner `exec`
replaces. Found while writing the corpus case for the `{v}>&-` value rule.
