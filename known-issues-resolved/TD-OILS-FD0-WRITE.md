### TD-OILS-FD0-WRITE. fd 0 has no write side, so `>&0` silently writes to stdout instead of the descriptor — ✅ RESOLVED 2026-07-28

**Fix (2026-07-28).** fd 0 now carries its write half, and its *access mode* is
modelled rather than probed. Two new variants say what a descriptor can do:
`WriteFd::ReadOnly` (open, but its description is not open for writing — fd 0
under `< file`, a here-document, or a pipe), whose write returns a synthetic
`EBADF`; and `StderrTarget::Discard`, the `2>&0` case where a diagnostic has
nowhere to go and bash drops it. The write half itself lives in
`Shell::exec_stdin_write: Option<Arc<File>>` (for `exec 0<> f`) and
`RedirPlan::stdin_write` (for the scoped `{ …; } <> f`), saved and restored
alongside `exec_stdin` in `exec_with_redirects` so the two halves always travel
together — re-binding fd 0 read-only drops the write half again. A new
`Shell::write_fd_for(n, plan)` replaces all eight bare `open_write_fds.get(&n)`
lookups, so every `>&N` path routes fd 0 through `stdin_write_fd`. `exec 3>&0`
*succeeds* on a read-only fd 0 (bash's dup succeeds; the later write through
fd 3 is what fails), which is why this is a descriptor variant and not a
redirect-time error.

Modelling the mode rather than attempting the write is what makes the answer
portable: Windows reports `ERROR_ACCESS_DENIED` for a write to a read-only
handle, which would have printed "Permission denied" where bash prints "Bad file
descriptor".

Depends on TD-OILS-WRITE-ERROR-REPORTING (resolved the same day) for the
reporting half: the failure surfaces as `echo: write error: Bad file descriptor`
with status 1, named against the builtin that produced the bytes.

**Tests.** `tests/corpus/redirect-fd0-write.sh` (byte-identical to bash 5.2) plus
six unit tests in `interp.rs`
(`write_to_a_read_only_fd0_fails_and_names_the_builtin`,
`write_to_a_read_only_fd0_fails_for_heredocs_and_pipes_too`,
`read_write_fd0_is_writable_and_shares_the_read_position`,
`a_dup_of_fd0_inherits_its_access_mode`,
`a_diagnostic_sent_to_a_read_only_fd0_is_dropped`,
`reopening_fd0_read_only_drops_the_write_half`). The lines excluded from
`tests/corpus/redirect-rw-shared-offset.sh` for this issue are now covered.

**Residual divergences (deliberately excluded from the corpus).** Three narrow
shapes still differ from bash, all same-status/same-stream wording or
unmodelled-syntax cases:

1. ~~An **external child**'s `>&0` on a read-only fd 0 gets `osh: 0: Bad file
   descriptor` from the shell instead of the child's own `write error` message.
   Same status 1, same stream, different wording — the shell refuses to hand the
   child a descriptor it knows is unwritable rather than letting the child
   discover it.~~ **Closed 2026-08-06** by
   `TD-OILS-A-READ-ONLY-FD-2-IS-HANDED-TO-AN-EXTERNAL-CHILD-AS-AN-EMPTY-STREAM`:
   `WriteFd::ReadOnly` now carries the description, so the child is handed it and
   discovers the failure itself, word for word as bash's does. Covered by the
   external-child sections of `tests/corpus/redirect-fd0-write.sh`.
2. ~~`{ echo Q >&0; } <&-` gives a *write-time* error where bash gives a
   *redirect-time* `0: Bad file descriptor`, because `0<&-` (closing fd 0) is not
   modelled — fd 0 has no "closed" state distinct from "read-only".~~ **Closed
   2026-08-01** by `TD-OILS-TRANSIENT-CLOSE-OF-A-STD-FD-IS-A-NO-OP`: fd 0 now has
   that state (`InputSrc::Closed`), and `Shell::stdin_write_fd` answers it with
   `None` — no such descriptor — where a read-only fd 0 is `WriteFd::ReadOnly`.
   Covered by `tests/corpus/redirect-close-of-stdin.sh`.
3. `exec 0<&N` where fd N was opened `<>` does not carry the write half across:
   the input table (`open_input_fds`) and the write table (`open_write_fds`) are
   separate, and the dup consults only the former. Fixing this properly means
   pairing the two tables into one descriptor table keyed by fd — worth doing if
   a third such divergence appears, not for this one alone.

**Original report follows.**

**Where:** `userspace/oils/src/interp.rs` — `Shell::exec_dup_source` and the
transient `>&N` resolution both look fd N up in `open_write_fds`, which never
has an entry for fd 0; the fallbacks resolve to the real stdout. The `<>` fd-0
arm of `resolve_one_redirect` correspondingly drops the write half it now has
(`drop(wr)`), and `apply_persistent_redirect`'s `exec 0<> file` keeps only the
read half.

**What.** osh models fd 0 as an input descriptor only, so a write *through* fd 0
never reaches it. Two shapes diverge, in opposite directions:

```sh
{ echo X >&0; } <> f    # bash: patches f in place       osh: prints X to stdout
{ echo X >&0; } < f     # bash: echo: write error: Bad file descriptor, st=1
                        # osh:  prints X to stdout, st=0
exec 0<> f; echo X >&0  # bash: patches f               osh: prints X to stdout
```

The second is the more troubling one: a write to a read-only descriptor is
supposed to *fail*, and osh turns it into output on an unrelated stream.

**Proper fix.** Give fd 0 a write side in the same places fd 1 and fd 2 have
one. The machinery now exists: `open_rw_pair` already produces both halves of an
`<>` open as duplicates of one descriptor with a shared offset (see
TD-OILS-RW-OFFSET), so what is missing is only somewhere to *put* the write half
and a resolution path that consults it. Concretely: a `Shell::exec_stdin_write`
slot (symmetric with `exec_stdout`/`exec_stderr`) plus a `plan.stdin_write` for
the scoped case, consulted by `exec_dup_source` and the transient `>&N` path
before their std-fd fallbacks; and when fd 0 has no write side, `>&0` must
resolve to an `EBADF` write error rather than to stdout — which is the half that
matters even for scripts that never use `<>`.

**Impact.** `>&0` is rare, and the read-only case fails *silently* rather than
corrupting anything — but it fails by producing output where bash produces a
diagnostic, so a script that redirects `>&0` deliberately (to write to the
terminal, an old idiom when fd 0 is the tty) behaves differently under a
redirect. Found while fixing TD-OILS-RW-OFFSET; the corresponding lines are
excluded from `tests/corpus/redirect-rw-shared-offset.sh`.
