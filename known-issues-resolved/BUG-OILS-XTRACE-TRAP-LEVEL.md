### BUG-OILS-XTRACE-TRAP-LEVEL. A trap handler's body traces at the caller's `PS4` depth instead of one level deeper — 2026-08-01 — ✅ **RESOLVED 2026-08-01**

**Where:** `userspace/oils/src/interp.rs` — `fire_trap_status`, which runs the
handler body via `run_source_flow_out` without touching `Shell::xtrace_level`.

**Reproduce** (found while measuring the DEBUG trap's view of `[[ … ]]`,
`target/dvscratch/t3/xtl.sh`):

```sh
b() { echo B; }
set -x; trap b DEBUG; echo one
```

bash traces the handler one level deeper than the command that fired it:

```
+ trap b DEBUG
++ b
++ echo B
B
+ echo one
```

osh emits `+ b` / `+ echo B`. The same one-level bump applies to an `ERR`
handler, a `RETURN` handler and a signal handler (`trap h USR1; kill -USR1 $$`),
and to an inline handler as much as to a function one — so it is the *trap*
that counts, not the function call (a plain call to `b` traces at `+`, and so
does a `( … )` subshell; only a command substitution otherwise adds a level).

**Not** the `EXIT` trap: bash runs that at shutdown, outside the pending-trap
path that bumps the level, and traces its body at `+`. osh already matches
there, so the fix must go in `fire_trap_status` alone and leave
`run_exit_trap_out` as it is.

**Fixed** by bumping `self.xtrace_level` around the `run_source_flow_out` call
in `fire_trap_status`, the way `expand_command_sub` and the `eval` builtin
already do. One site covers all four traps, since they all come through there;
`run_exit_trap_out` is deliberately left alone. Covered by
`tests/corpus/xtrace-traces-a-trap-handler-one-level-deeper.sh`, which pins the
DEBUG, ERR and RETURN handlers, the inline-versus-function equivalence, the
plain call and the `( … )` subshell that add nothing, the command substitution
that does, and the `EXIT` handler that does not. The signal case is not in the
corpus — `kill -USR1 $$` puts a pid in the trace — but is in the probe
`target/dvscratch/t3/xtl.sh`, and shares the fixed code path.
