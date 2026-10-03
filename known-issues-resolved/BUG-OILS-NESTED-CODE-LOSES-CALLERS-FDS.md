### BUG-OILS-NESTED-CODE-LOSES-CALLERS-FDS. Trap handlers, `eval` and `source` ran with the real fd 0/1 instead of the caller's sink and stdin — 2026-07-27 — ✅ RESOLVED 2026-07-27

**Symptom.** Internally-generated shell code (a trap handler, an `eval` string,
a sourced script, a `mapfile -C` callback) ran through `run_source_flow`, which
manufactured a fresh `Out::Inherit` **and** `StdinSrc::Inherit`. Both halves
diverged from bash:

| Case | bash | osh (before) |
|---|---|---|
| `x=$(trap 'echo T' DEBUG; :)` | `x=T` | `x=`, `T` on the terminal |
| `x=$(trap 'echo E' EXIT; echo body)` | `x=$'body\nE'` | `x=body`, `E` on the terminal |
| `set -T; trap 'echo RET' RETURN; f(){ echo body; }; x=$(f)` | `x=$'body\nRET'` | `x=body`, nothing fired |
| `echo hi \| { eval 'read v; echo "[$v]"'; }` | `[hi]` | `[]` — read the real fd 0 |
| `echo ho \| { . reads-stdin.sh; }` | `[ho]` | `[]` |

**Root cause.** `run_source_flow_out` took an `out` but hardcoded
`StdinSrc::Inherit`, and `fire_trap_flow` took neither. On top of that, the
command-substitution path *deliberately deleted* the DEBUG/RETURN/ERR traps from
the subshell clone, on the stated grounds that "bash's behaviour for these
pseudo-signal traps inside `$( … )` is quirky and inconsistent (it captures the
trap's own output into the result…)". That reading was an artefact of the
missing sink threading: bash's actual rule is uniform — the handler runs in the
substitution's subshell, whose fd 1 *is* the pipe, so of course its output is
captured. The band-aid was papering over the routing bug.

**Fix** (`userspace/oils/src/interp.rs`). `run_source_flow_out` now takes
`stdin: &StdinSrc`; `fire_trap_flow` takes `out` and `stdin`; both are threaded
from every call site (`exec_and_or`, `exec_pipeline`, `exec_simple_inner`,
`call_function` ×3, `deliver_self_signal` — which meant giving `builtin_kill` /
`kill_one` an `out`+`stdin` — `builtin_source`, the `eval` arm, and the
`mapfile -C` callback). `run_source_flow` (the `Out::Inherit` wrapper) is
deleted, so the compiler enforces that every caller supplies real fds. The
cmdsub trace-trap deletion is removed: `clone_for_subshell`'s ordinary
`functrace`/`errtrace` inheritance rules now apply there like anywhere else.

The whole probe script (RETURN/DEBUG/ERR × captured/uncaptured) now produces
**byte-identical output** to bash 5.x.

**Tests.** `trap_output_goes_to_the_callers_sink`,
`cmdsub_inherits_trace_traps_only_under_functrace` (all four expectations
re-checked against bash after they passed, so the test pins bash's behaviour and
not merely osh's), `eval_and_source_read_the_callers_stdin`. 725 + 15 pass,
clippy clean.
