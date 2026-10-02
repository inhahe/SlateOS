### TD-OILS-STDERR-INTERLEAVE. Same-sink stdout+stderr redirects flush in the wrong order — FIXED 2026-07-19 (all subcases resolved; the capture+subshell+`2>&1` subcase was later fixed by the compound fd-dup routing work)

**Where:** `userspace/oils/src/interp.rs` — `exec_with_redirects` (the
compound/group-command redirect+capture path).

**Symptom (fixed):** a compound command that wrote to **both** stdout and stderr
with both redirected to the **same** file in one shot — `{ …; } >f 2>&1`,
`( … ) >f 2>&1`, `for … done >f 2>&1`, the `&>f` shorthand — buffered stdout to
the end and folded stderr in ahead of it, so `osh` produced `e\no` where bash
produces `o\ne` (content correct, interleave order wrong).

**Fix:** a `> f` stdout redirect now drives fd 1 through the file *live* via a
scoped `exec_stdout` override (instead of capturing to a `Vec` and dumping at the
end). When fd 2 targets the same path (`>f 2>&1`, `&>f`) it shares fd 1's open
handle (a `try_clone`, same file object / same OS offset on both Unix and
Windows), so the two streams interleave at one shared offset exactly as bash's
dup does. A parallel scoped `exec_stderr` override was added so `( … )` subshell
bodies — which clone `exec_stdout`/`exec_stderr` but reset `stderr_stack` — also
reach the file (this incidentally fixed `( echo e >&2 ) 2> f`, which previously
leaked the subshell's stderr to the real terminal). The `2>&1 > f` ordering case
(fd 2 copies fd 1's sink *before* `> f` rebinds it) is handled by snapshotting
fd 1's pre-override sink into a concrete handle. Regression tests:
`group_redirect_stdout_stderr_interleave`, `for_loop_redirect_stdout_stderr_interleave`,
`subshell_redirect_stdout_stderr_interleave`, `subshell_stderr_only_redirect_reaches_file`,
`stderr_then_stdout_redirect_order_keeps_stderr_on_prior_sink`, and the updated
`amp_redirect_both_streams`.

**Formerly-remaining subcase (now RESOLVED 2026-07-19):** command-substitution
*capture* of a `2>&1` **subshell** — `x=$( ( echo o; echo e >&2 ) 2>&1 )` — used
to lose the subshell's stderr (it went to the real terminal, so `x` got only
`o`). This was fixed as a side effect of the compound fd-dup routing work
(`AliasStd`/`stdout_to_fd`/`stderr_to_fd` handling in `exec_with_redirects`):
verified `x=$( ( echo o; echo e >&2 ) 2>&1 )` now yields `o\ne` matching bash,
as does the non-subshell form `x=$( { echo o; echo e >&2; } 2>&1 )`. No open
subcases remain for this item.

**Later subcase — capture *ordering* (RESOLVED 2026-07-28):** routing the
subshell's stderr into the capture was only half the story. Until now fd 2
wrote into a *separate* `StderrTarget::Buffer` that was appended to the capture
after the body finished, so a stderr write that came **first** still landed
last: `x=$( { echo E >&2; echo O; } 2>&1 )` gave `O\nE` where bash gives
`E\nO`. Fixed by making the capture itself shared — `Out::Capture` now holds an
`Arc<Mutex<Vec<u8>>>` instead of a `&mut Vec<u8>`, and a `2>&1` over a captured
fd 1 pushes a `StderrTarget::Buffer` holding *that same* `Arc`, so the two
streams interleave in write order exactly as they do on a shared file or pipe.
This covers the compound path (`exec_with_redirects`), the builtin path
(`run_builtin`) and redirect-failure diagnostics (`push_partial_stderr`, whose
`Out::Capture` arm no longer has to be special-cased in
`report_redirect_failure`). Regression coverage: the "including when that sink
is a command substitution's capture" section of
`tests/corpus/redirect-2to1-dup.sh` and the interleave probe in
`tests/corpus/redirect-dev-fd.sh`.
