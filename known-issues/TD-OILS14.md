### TD-OILS14. `osh` `exec`: input+output redirection-only forms + named input/write fds + scoped per-command extra fds + builtin stderr redirects + function-invocation redirects + fd save/restore/swap (`exec 3>&1`/`1>&3`/swap) implemented; only a true in-place `execve` remains — PARTIALLY RESOLVED 2026-07-19

**Where:** `userspace/oils/src/interp.rs` (`run_builtin` `"exec"` arm sets the
persistent targets; `Shell.exec_stdout`/`exec_stderr`/`exec_stdin` fields;
`write_bytes` `Out::Inherit`, `emit_stderr`, `run_external`, `read_line`,
`read_record_input`, and `read_all_bytes` consult them).

**What:** the `exec` builtin is implemented for the command-replacement case
(`exec cmd args` runs `cmd` then exits the shell with its status; a missing
command exits 127). Status of the remaining aspects:
1. **Output redirection-only `exec` — RESOLVED 2026-07-19.** `exec > file`,
   `exec >> file`, `exec 2> file`, `exec 2>> file`, `exec > file 2>&1`, and
   `exec 1>&2` now persistently rebind the shell's ambient fd 1 / fd 2. The
   shell stores the target as `exec_stdout`/`exec_stderr: Option<Arc<File>>` —
   the file opened once (truncated for `>`, append for `>>`) with the handle
   kept so all subsequent writes share one OS offset (bash dups the fd; it does
   not reopen) — and every ambient fd-1/fd-2 write consults it: builtins via
   `write_bytes`'s `Out::Inherit`
   arm, `>&2` diagnostics via `emit_stderr`, and external children via
   `run_external` (the child's stdout/stderr is opened on the file). Subshell
   clones inherit the redirect (bash: a subshell inherits the fd table). Note
   the same left-to-right ordering simplification the rest of the shell has for
   `2>&1 > f` vs `> f 2>&1` applies here (the dup follows fd 1's *final* sink).
2. **Input redirection-only `exec < file` — RESOLVED 2026-07-19.** `exec < file`
   (and `exec << EOF`) reads the source fully into a per-shell
   `exec_stdin: Option<RefCell<Cursor<Vec<u8>>>>` at exec time; every base fd-0
   read consults it: the `read` builtin (`read_line`/`read_record_input`),
   `read_all_bytes` (used by `mapfile`/`$(<file)`-style reads), and an external
   command inheriting fd 0 (`run_external`'s `StdinSrc::Inherit`+no-redir arm,
   which feeds the child the cursor's remaining bytes). Successive `read`s
   therefore consume successive lines. A subshell clone inherits a *snapshot* of
   the remaining bytes with an independent cursor (reads in the subshell don't
   advance the parent's offset — a minor deviation from bash's shared-fd
   semantics, consistent with how our subshells already copy their stdin). One
   approximation: an external that inherits fd 0 is handed the cursor's whole
   remaining buffer via a pipe, so a subsequent `read` in the parent sees EOF
   rather than the bytes the child left unconsumed (a shared-fd offset would
   differ) — acceptable for the common `exec < f; read …` idiom.
3. **Named input descriptors (`exec 3< file`, `read -u 3`) — RESOLVED
   2026-07-19.** `exec 3< file` (and `exec 3<< EOF` / `3<<< str`) opens a
   user-space input descriptor fd ≥ 3 in a per-shell
   `open_fds: HashMap<i32, RefCell<Cursor<Vec<u8>>>>` (the file is slurped once
   into a position-tracking cursor at redirection time, so a missing/unreadable
   path is an error there); `exec 3<&-` removes the entry. `read -u N` (N ≥ 3)
   then reads successive records from that cursor instead of the ambient input,
   independently of fd 0; an unopened fd is a status-1
   `read: N: bad file descriptor`. `resolve_redirects` captures fd ≥ 3 input
   redirects into `RedirPlan.extra_fds` (`ExtraFdOp::InputBytes`/`Close`), which
   only the `exec` builtin consumes. A subshell clone inherits an
   independent-offset snapshot of each open fd (same approximation as
   `exec_stdin`).
4. **Named write descriptors (`exec 3> file`, `echo >&3`) — RESOLVED
   2026-07-19.** `exec 3> file`/`3>> file` opens a user-space write descriptor
   fd ≥ 3 in a per-shell `open_write_fds: HashMap<i32, Arc<File>>` (the file
   opened once — truncated for `>`, append for `>>` — and the handle kept so
   successive writes accumulate on one OS offset); `exec 3>&-` removes it.
   `resolve_redirects` captures `N> file` (N ≥ 3) into
   `ExtraFdOp::OutputFile(path, append)` (consumed by `exec`) and `M>&N` (N ≥ 3)
   into `RedirPlan.stdout_to_fd`/`stderr_to_fd`. Output is routed there: a
   builtin's `echo … >&3` via a `write_to_fd` helper in `write_bytes`, and an
   external `cmd >&3`/`cmd 2>&3` by building the child's `Stdio` from a
   `try_clone` of the shared handle in `run_external`. An unopened write fd is a
   status-1 `N: Bad file descriptor`. A subshell shares the same `Arc<File>`
   (bash fd inheritance). This also fixed a latent bug: a per-command `N> file`
   (N ≥ 3) previously fell into the Write arm's `_ => plan.stdout` case and
   wrongly redirected fd 1; fd ≥ 3 output redirects now route to `extra_fds` (a
   documented no-op on any command other than `exec`).
5. **Scoped per-command extra-fd redirects (`{ …; } 3< file`) — RESOLVED
   2026-07-19.** A *compound* command carrying a fd ≥ 3 redirect
   (`while read -u 3 line; do …; done 3< file`, `{ …; } 4> log`, `… 3<&-`) now
   installs the descriptor into `open_fds`/`open_write_fds` for the body's
   duration only: `exec_redirected` consumes `plan.extra_fds`, saving each
   touched fd's prior binding (taken by ownership out of the map) and restoring
   it — removing the scoped fd — after the body. So `read -u 3` inside the loop
   reads the file while fd 0 stays free, and fd 3 is gone once the loop exits.
   (A repeated fd in the same plan drops the earlier install before applying the
   next; the *first* occurrence's prior binding is the one restored.)
6. **Builtin stderr redirects (`echo … 2>&3`, `read … 2> file`, `… 2>&1` on a
   *builtin*) — RESOLVED 2026-07-19.** A simple-command builtin now honors its
   own fd-2 redirect: `run_builtin` pushes a scoped `StderrTarget` for the
   builtin's duration based on the `RedirPlan` — `2> file`/`2>> file` →
   `File`, `2>&N` (N ≥ 3) → the new `StderrTarget::WriteFd(Arc<File>)` (routed
   to `open_write_fds[N]`), and `2>&1` (fd 1 not a file) → mirror fd 1's live
   sink (`Buffer` for a captured stdout, `Pipe` for a pipeline stage, `Stdout`
   for the real terminal). Both `emit_stderr` and `child_stdio_for_stderr` gained
   a `WriteFd` arm. The `2>&1`-into-captured-stdout case folds the buffered
   stderr into fd 1's sink after the builtin's stdout (line-level interleaving
   not preserved, as elsewhere). The `exec` builtin is exempt from the scoped
   push (it sets the *persistent* `exec_stderr` itself). The former order-free
   caveat (the `>&2 2>file` combination routing `>&2` output to the file rather
   than the pre-redirect stderr) is **RESOLVED 2026-07-19**: the resolver only
   sets `stdout_to_stderr` for the dup-first ordering (`2>file >&2` still copies
   the file target into `stdout`), so when a per-command stderr redirect is
   present it is the freshly-pushed top of `stderr_stack`. Both the builtin
   `write_bytes` path and the compound/function `exec_with_redirects` finaliser
   now route the `>&2` output via `emit_stderr_depth`, skipping that top entry so
   fd 1 lands in the pre-redirect (enclosing/inherited) sink — matching bash's
   left-to-right redirection semantics. Regression: `dup_stdout_before_stderr_redirect`.
7. **Function-invocation redirects — RESOLVED 2026-07-19.** A function call
   carrying its own redirects (`myfunc > file`, `myfunc 2> err`, `myfunc < in`,
   and compound-scoped fd ≥ 3 forms like `myfunc 3< file`) now applies the
   redirect to the *whole function body*. The former `exec_redirected` body was
   extracted into a reusable `exec_with_redirects(plan, out, stdin, run)` helper
   (it establishes the stdin cursor, pushes the `StderrTarget`, installs scoped
   fd ≥ 3 descriptors, captures file/stderr-bound stdout, runs the body via the
   `run` closure, then tears everything down). `exec_redirected` delegates to it
   with `run = |sh,o,s| sh.exec_command(inner, o, s)`; `exec_simple`'s function
   branch delegates with `run = |sh,o,s| sh.call_function(name, args, assigns,
   o, s, default)` — gated on a new `RedirPlan::needs_scope()` so a redirect-free
   call still dispatches directly. Tests: `function_invocation_stdout_redirect`,
   `function_invocation_stderr_redirect`, `function_invocation_stdin_redirect`.
   Note `stdout_to_fd`/`stderr_to_fd` (dup onto an `exec`-opened write descriptor)
   are *not* covered by the scope — those are applied per-builtin/-external — so
   `myfunc >&3` is not yet routed for the whole body.
8. **fd save/restore/swap (`exec 3>&1`, `exec 1>&3`, `exec 3>&1 1>&2 2>&3`) —
   RESOLVED 2026-07-19.** A redirection-only `exec` now applies its redirects in
   strict left-to-right *source order* against the shell's persistent fd table,
   so all the standard fd-juggling idioms work: `exec 3>&1` saves the current
   fd 1 sink into a user-space write fd (a `try_clone` of `exec_stdout`'s handle,
   or a dup of the real terminal when fd 1 is unredirected); `exec 1>&3` restores
   fd 1 from a saved fd; and the classic swap `exec 3>&1 1>&2 2>&3 3>&-` exchanges
   stdout and stderr. The collapsed `RedirPlan` cannot express ordered fd
   mutation (it buckets each effect into a fixed field and loses order), so the
   `exec` builtin bypasses it: `exec_simple_inner` intercepts an args-less `exec`
   with redirects and calls `apply_exec_redirects(&sc.redirects)`, which walks the
   raw redirects in order. Each `M>&N` dup reads fd N's *current* sink via
   `exec_dup_source` (which returns a concrete dup of the real std fd when fd N is
   still on the terminal, so `1>&2` points fd 1 at fd 2's actual sink even when
   the shell's real fd 1 ≠ fd 2 — e.g. launched under `1>file`). This also
   required widening `exec_stdout`/`exec_stderr` from `(path, append)` to
   `Option<Arc<File>>` so a restored fd can point at an arbitrary handle, not just
   a re-openable path. (The rare `command exec`/`builtin exec` re-dispatch still
   goes through the collapsed-plan path, which handles save/restore but not
   arbitrary-order swaps — an essentially nonexistent usage.) Tests:
   `exec_save_and_restore_stdout`, `exec_swap_stdout_stderr`.
9. **Not a true `execve` — still OPEN (gated on kernel `execve`).** `exec cmd`
   spawns `cmd` as a child, waits, and exits with its status — observationally
   the shell does not continue, but the pid is not preserved and signals are not
   transparently forwarded the way a real in-place `execve` would provide.
10. **Per-command dup-then-close (`cmd 2>&3 3>&-`) — RESOLVED 2026-07-20.** The
    canonical idiom that duplicates a saved descriptor onto fd 1/2 and then
    closes it *on the same command* (`echo hi 2>&3 3>&-`, `{ …; } 1>&3 3>&-`,
    `ls … 2>&3 3>&-`) previously failed with a spurious `3: Bad file descriptor`:
    the order-free `RedirPlan` records the `N>&-` close in `extra_fds` and the
    `M>&N` dup in `stdout_to_fd`/`stderr_to_fd`, and `install_extra_fds` applied
    the close *first*, removing fd N before the dup resolved. Fixed with a
    post-pass in `resolve_redirects`: when the plan still dups from a descriptor,
    the transient (command-scoped) close of that descriptor is dropped, so the
    dup resolves against the live fd and fd N is left in its correct
    post-command (still-open) state — a per-command close is undone afterward
    anyway. The only residual is the reverse ordering `3>&- 2>&3` (which bash
    *rejects*), indistinguishable in the collapsed plan and treated as the useful
    ordering. Regression: `dup_then_close_same_command_resolves_before_close`.

**Proper fix:** (2)–(8) done (see above). Remaining: (9) once the kernel exposes
`execve`, replace the spawn+wait+exit with an actual in-place image replacement
for `exec cmd`.
