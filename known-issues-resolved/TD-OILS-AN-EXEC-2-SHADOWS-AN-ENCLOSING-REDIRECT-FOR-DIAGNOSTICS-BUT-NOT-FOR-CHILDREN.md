### TD-OILS-AN-EXEC-2-SHADOWS-AN-ENCLOSING-REDIRECT-FOR-DIAGNOSTICS-BUT-NOT-FOR-CHILDREN. `{ exec 2>f; sh -c 'echo E >&2'; } 2>g` writes to `g` — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/interp.rs` — three sites that read
`self.stderr_stack.last()` directly instead of going through
`Shell::stderr_target_at`: the external-command fd 2 wiring,
`Shell::child_stdio_for_stderr`, and `Shell::stderr_aliases_capture`.

**What.** A body's `2>…` is a *stack entry*; a persistent `exec 2>…` is a
*field*. When the `exec` runs with the entry already pushed, the `exec` is the
later word on fd 2 and shadows it — which is what `exec_stderr_depth` records
and `stderr_target_at` applies. The shell's own diagnostics went through that
helper and were right (`TD-OILS...`, and
`tests/corpus/exec-stderr-shadows-an-enclosing-redirect.sh` already pinned it).
Everything fd 2 reaches *other* than a diagnostic read the raw stack instead and
saw the shadowed entry:

```text
$ { ( exec 2>e; sh -c 'echo E >&2' ); } 2>/dev/null
bash: e=[E]          # the exec won
osh : e=[]           # E went to the enclosing /dev/null

$ { ( exec 2>e; echo B >&2 ); } 2>/dev/null
both: e=[B]          # a builtin was already right
```

Three shapes, all the same root: a child's fd 2 (`sh -c 'echo E >&2'`), a
child's `1>&2` dup of it, and — via `stderr_aliases_capture` — whether a `2>&1`
inside a `$( … )` is still *held* by fd 2. The last is visible as a hang rather
than a misdirected byte: `x=$( { exec 2>/dev/null; sleep 2 & } 2>&1 )` returned
at once under bash and waited two seconds here, because osh still thought fd 2
named the capture the `exec` had just replaced. `exec 2>&-` was wrong in the
same way, and there the *status* differs too — the child's write to a missing
fd 2 fails (rc 1) where a write to the enclosing sink succeeds (rc 0).

**Fix (2026-08-06).** All three sites now call
`self.stderr_target_at(self.stderr_stack.len())`, which is the same resolution
the diagnostic path has used since `exec_stderr_depth` was introduced. No new
mechanism — the helper existed and these callers simply predated it or did not
reach for it.

**Found by** the read-only-fd child work
(`TD-OILS-A-READ-ONLY-FD-2-IS-HANDED-TO-AN-EXTERNAL-CHILD-AS-AN-EMPTY-STREAM`):
a corpus probe wrote its rows through `{ … } 2>&1 | sed`, and three
`exec 2>&3` rows came back wrong for a reason that had nothing to do with
read-only descriptors. The probe was restructured to the sibling cases'
`exec 4>&2 2>err` idiom so the two changes stayed separable.

**Tests.** `tests/corpus/exec-stderr-shadows-an-enclosing-redirect.sh` gained
the external-child half (fd 2, the `1>&2` dup, `exec 2>&-`, and the reversed
order) and a capture-holding probe that reads which of the two happened off a
background job's file rather than off the clock.
