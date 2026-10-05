### TD-OILS-WRITE-ERROR-REPORTING. A builtin whose output write fails says nothing and returns success — ✅ RESOLVED 2026-07-28

**Fix (2026-07-28, commit `8af7af90e`).** The write `io::Result` is now threaded
back rather than discarded. `write_to_write_fd` returns `io::Result<()>`, and
every builtin output path (`write_bytes`, `write_redirected`) funnels its result
through one new `Shell::finish_write`, which turns it into the status the builtin
returns:

- `Ok(())` → 0.
- `ErrorKind::BrokenPipe` → sets `pipe_broken` and returns 141 (128 + SIGPIPE),
  **silently** — `cmd | head` closing the pipe is the normal end of a pipeline,
  not an error, and bash suppresses it for the same reason.
- anything else → `{prefix}{builtin}: write error: {io_error_message(e)}` on
  stderr and status 1.

The builtin's *name* comes from a new `Shell::builtin_names: Vec<String>` stack,
pushed/popped in `run_builtin`. A stack rather than a scalar because
`eval`/`command`/`source` run commands of their own, so the name that should
appear is the innermost builtin actually producing the bytes.

Doing it in `finish_write` — one funnel — rather than per-builtin is what keeps
the change from being a signature churn across every output helper, which is
what the original report worried about.

The `Out::Inherit` path additionally flushes: `io::stdout()` is line buffered, so
a full disk or a closed terminal surfaces at the flush rather than in
`write_all`, and a `write_all`-only check would have missed exactly the case the
issue is about.

**Testing note (resolved).** As predicted, `/dev/full` was not usable in the
differential harness. Coverage instead comes from `EBADF` via `>&0` — see
TD-OILS-FD0-WRITE and `tests/corpus/redirect-fd0-write.sh`, which pins
`echo: write error: Bad file descriptor` / status 1 byte-for-byte against bash.
`map_device_path` was **not** given a synthetic always-failing `/dev/full`: the
error path is shared, so `EBADF` exercises it just as well as `ENOSPC` would,
and a fake device node visible to scripts is a bigger commitment than the
coverage justifies.

**Original report follows.**

**Where:** `userspace/oils/src/interp.rs` — `Shell::write_bytes` and the builtin
output helpers that call it; the `Out`/`WriteFd` write paths generally.

**What.** bash checks the write and reports it, naming the builtin:

```
$ bash -c 'echo hi > /dev/full; echo "st=$?"'
bash: line 1: echo: write error: No space left on device
st=1
$ bash -c 'printf abc > /dev/full; echo "st=$?"'
bash: line 1: printf: write error: No space left on device
st=1
```

osh has no such diagnostic anywhere — grep for `write error` finds nothing. A
failed write is discarded, so a script that fills a disk (or writes to a
descriptor that is not open for writing) sees success and carries on with
truncated output. That is a silent-data-loss class of bug, not a cosmetic one.

**Related.** TD-OILS-FD0-WRITE needs exactly this path: `{ echo X >&0; } < f`
must produce `echo: write error: Bad file descriptor` and status 1.

**Testing note.** The obvious test vehicle, `/dev/full`, is *not* usable in the
differential harness: MSYS bash emulates it, osh's `map_device_path` does not
map it, and Windows has no always-failing sink to map it to. (Mapping it to
`NUL` would be wrong — `NUL` accepts writes.) So the corpus can only cover
`EBADF` (via `>&0`, once TD-OILS-FD0-WRITE lands) and whatever a closed pipe
gives; `ENOSPC` has to be a unit test against a stub sink. Deciding whether
`map_device_path` should grow a synthetic always-failing `/dev/full` is part of
the work.

**Proper fix.** Thread the write `io::Result` back to the builtin that produced
the output instead of discarding it, and have each builtin emit
`{name}: write error: {io_error_message(e)}` and return 1. The awkward part is
that osh's builtins mostly hand a `String`/`Vec<u8>` to a shared writer and
never see the result, so this is a signature change across the builtin output
helpers — do it once, uniformly, rather than per-builtin. `SIGPIPE`/broken-pipe
must stay silent (bash suppresses `EPIPE` for the same reason: it is the normal
end of `cmd | head`), which the existing broken-pipe handling in `run_external`
already reasons about.
