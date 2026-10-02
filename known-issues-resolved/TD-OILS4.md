### TD-OILS4. `osh` pipelines: per-stage redirects composed with inter-stage pipes — RESOLVED 2026-07-18 (threaded streaming pipeline landed 2026-07-18; redirect composition verified 2026-07-18)

**Where:** `userspace/oils/src/interp.rs` (`exec_pipeline`,
`exec_concurrent_pipeline`, `exec_threaded_pipeline`, `finish_pipeline`,
`stage_is_plain_external`).

**What:** Pipelines now run **concurrently and stream** on every path.
An all-external pipeline wires real OS pipes between child processes; any
pipeline containing a builtin/function/compound stage uses the *threaded*
path (`exec_threaded_pipeline`): each stage runs in its own subshell on
its own thread, connected by real OS pipes (`io::pipe`), via the new
`Out::Pipe`/`StdinSrc::Pipe` endpoints and the `pipe_broken` flag (the
in-process SIGPIPE analogue — a builtin write to a closed downstream pipe
returns exit 141 and unwinds the stage). Downstream early-exit propagates
upstream: an in-process producer stops on `pipe_broken`; an external
producer stops on the OS broken-pipe signal on targets that deliver it.
`pipefail` + `${PIPESTATUS[@]}` record per-stage codes on both paths.
This resolves the former "buffered fallback isn't concurrent" gap.

**RESOLVED:** per-stage redirects *are* composed with the inter-stage
pipe. A stage with its own redirect (`a | b > f`, `a | b 2>err`) is routed
to the threaded path; `run_external` (and `run_builtin`) resolve the
stage's `RedirPlan` against the pipe endpoints when building the child —
`redir.stdout`/`redir.stderr`/`redir.stdin` (a file, or here-doc/cursor
`stdin_data`) override the corresponding `Out::Pipe`/`StdinSrc::Pipe`
endpoint, and where there is no redirect the pipe endpoint is used. Two
Windows tests (`pipeline_stage_stdout_redirect_composes_with_pipe`,
`pipeline_stage_stderr_redirect_composes_with_pipe`) verify a redirected
external stage's stdin still comes from the upstream pipe while its
stdout/stderr are diverted to files (on both the last-stage main-thread
path and a worker-thread stage).

**Note on external-producer early-termination testing:** relying on the
OS to kill an unbounded *external* producer when its consumer exits is a
target-OS property (bash uses SIGPIPE; slateos delivers EPIPE), not shell
logic. The Windows test host can't exercise it — `cmd`'s `echo` ignores
broken-pipe writes and loops forever, so `Child::wait` never returns.
The shell-side cascade is covered by
`threaded_pipeline_inprocess_producer_terminates_early`; there is
intentionally no external-producer variant (see the comment in
`interp.rs` tests).

Documented in the `interp.rs` module header. Tests cover the threaded
streaming path (subshell isolation, in-process early-termination,
classifier routing) but not the deferred per-stage-redirect gap.
