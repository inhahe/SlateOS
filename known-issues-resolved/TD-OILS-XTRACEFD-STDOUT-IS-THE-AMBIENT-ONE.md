### TD-OILS-XTRACEFD-STDOUT-IS-THE-AMBIENT-ONE. `BASH_XTRACEFD=1` traces to the shell's fd 1, not to an enclosing capture or pipeline stage — 2026-08-04 — ✅ FIXED 2026-08-06

**Where:** `Shell::emit_xtrace` in `userspace/oils/src/interp.rs`, the `1 => …`
arm.

**What.** `$BASH_XTRACEFD` is otherwise complete: the trace follows the number it
names through a rebind, a close ends the diversion, an unset closes the
descriptor, and every descriptor ≥ 3 resolves through `open_write_fds`, which is
the *live* binding. fd 2 is likewise the live fd 2, because `emit_stderr` already
resolves it through the `stderr_stack`. fd 1 is the exception: its live sink is
the `Out` value threaded through command execution, and `emit_xtrace` has no
`Out` — so it falls back to `exec_stdout` (an `exec > file`) or the shell's real
stdout.

The divergence is therefore confined to `BASH_XTRACEFD=1` *inside* a context that
rebinds fd 1 without an `exec`:

```sh
x=$(BASH_XTRACEFD=1; set -x; echo hi; set +x)   # bash: x holds the trace
echo one | { BASH_XTRACEFD=1; set -x; read v; } > p.txt   # bash: trace in p.txt
```

bash puts the trace in the capture / in `p.txt`; osh puts it on the terminal. A
group's `> file` and a pipeline stage's pipe are the same case. Anything ≥ 3 —
which is what the feature exists for, diverting a trace into a log file — is
exact.

**Why it is not simply fixed.** Threading `Out` into the trace emitters means
threading it into `cond_trace_unary` / `cond_trace_binary` and so into the whole
`[[ … ]]` evaluator, which takes no `Out` today because a conditional writes
nothing. Roughly a dozen signatures, for a case (`BASH_XTRACEFD=1` under a
capture) that is a curiosity rather than a use.

**What the proper fix looks like.** Give the shell an fd-1 counterpart to
`stderr_stack` — a stack of live fd-1 sinks pushed and popped where `Out` is
currently switched (command substitution, pipeline stage, a compound command's
`> file`). `emit_xtrace` would then resolve fd 1 the way it already resolves
fd 2, and `exec_stdout_shadowing`'s "fd 1's ambient sink is a value, not a stack"
comment would stop being true — which is the same reason it is worth doing: the
asymmetry between the two standard write descriptors is itself the tech debt.

**Fixed 2026-08-06,** by that route but with the save/restore shape `exec_stdout`
already uses rather than an explicit `Vec` — the call stack is the stack.
`Shell::live_stdout: Option<WriteFd>` holds the fd-1 sink that lives in the
`Out` value, and `emit_xtrace`'s fd-1 arm reads
`self.exec_stdout.as_ref().or(self.live_stdout.as_ref())`.

Why bash needs no such field: `$BASH_XTRACEFD` names a *descriptor*, and bash
opens it once —

```c
      fd = (int)strtol (t, &e, 10);
      if (e != t && *e == '\0' && sh_validfd (fd))
	{
	  fp = fdopen (fd, "w");                 /* variables.c, sv_xtracefd */
	  ...
	    xtrace_set (fd, fp);
```

— after which every trace line is an `fprintf (xtrace_fp, …)` in `print_cmd.c`.
A substitution and a pipeline stage are *forks*, so descriptor 1 in the writing
process simply **is** the pipe. osh runs those bodies in-process and carries
fd 1 as a threaded value, so the number has to be resolved against something the
shell knows; `live_stdout` is that something.

Maintained at exactly the six sites that clear `exec_stdout` to install a
capture/pipe `Out` for a body — `command_sub`, the input half of process
substitution, `exec_redirected`'s two `1>&N`/`1>&2` capture arms, the pipeline
stage thread, and the `coproc` body thread — so the two fields never both
describe fd 1 and the *later word wins* rule falls out of asking `exec_stdout`
first: a runtime `exec > file` inside a capture sets it again, and a compound
command's `> file` shadows via the scoped override `exec_redirected` already
installs. Subshell clones inherit it, because a subshell inherits fd 1. The pipe
cases store a dup (`pipe_writer_to_file`, the same conversion `alias_write_fd`
uses for `3>&1` in a stage); a failed dup only costs the trace its diversion.

**And it was hiding a second bug.** The compound-declaration trace line —
`declare -a arr=(1 2)`'s `+ declare -a arr`, emitted from
`Shell::builtin_declare_scoped`'s phase-3 preamble — went out through
`emit_stderr` rather than `emit_xtrace`, so it was the one trace line
`$BASH_XTRACEFD` did not move *at any number*: `exec 3>t; BASH_XTRACEFD=3;
declare -a arr=(1 2)` left it on fd 2 while every other line went to fd 3. Now
`emit_xtrace`, which routes to fd 2 anyway when the variable is unset, so
nothing else changes.

**Corpus:** `a-trace-diverted-to-fd-one-goes-wherever-fd-one-goes.sh` — 34
probes. Two orderings are pinned there as part of the answer: a simple command
is traced *before* its own redirect is applied (so `echo p | cat > f` puts
`++ echo p` into `f`, by way of `cat`, but not `++ cat`), and an inner
substitution's trace lands in the inner capture and so becomes part of the
*value* the outer assignment is traced with.
