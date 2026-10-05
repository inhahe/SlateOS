### TD-OILS-BUILTIN-USAGE. `osh` builtins omit bash's second `NAME: usage: …` synopsis line on a usage error — ✅ LARGELY RESOLVED 2026-07-20

**Update (2026-07-20):** a shared `Shell::builtin_invalid_option(builtin, opt,
usage)` helper now emits bash's two-line pair (the located
`NAME: -OPT: invalid option` diagnostic + the unprefixed `NAME: usage: …`
synopsis) and returns status 2. It (and inline equivalents) are wired into every
builtin that previously silently ignored or misclassified unknown options:
`declare`/`typeset`/`local`, `read`, `readonly`, `unset`, `type`, `hash`, `cd`,
`pwd`, `alias`, `unalias`, `jobs`, `trap`, `mapfile`, `command`, `printf`
(no-format), and `source`/`.` (missing-filename **and** bogus-option, tagged with
the invoking name). All verified byte-for-byte against bash 5.x via the CLI probe
and covered by the `builtin_invalid_option_diagnostics` and
`more_builtins_reject_invalid_options` regression tests. `getopts` already matched.
No remaining known builtin silently swallows an invalid option; this entry stays
listed only as a pointer to the helper and the wording convention.

**Where:** `userspace/oils/src/interp.rs` — `Shell::builtin_invalid_option`
(shared helper) and the per-builtin flag loops listed above. (Historically the
usage-error paths of `builtin_getopts`, `builtin_source`, `builtin_mapfile`,
`exec_command_builtin`, `builtin_printf` (`-v`), etc. emitted only the one-line
diagnostic `osh: NAME: <problem>`.)

**What:** on a usage/argument error bash prints **two** lines to stderr — the
diagnostic *and* a synopsis, e.g.

```
bash: line 1: command: -Z: invalid option
command: usage: command [-pVv] command [arg ...]
```

osh prints only the first line. The exit status and the primary diagnostic
match; only the trailing `NAME: usage: …` synopsis line is missing. Affects
`command`, `getopts`, `source`, `mapfile`, `printf`, and any other builtin with
a bash usage synopsis.

**Impact:** very low — cosmetic stderr text only; scripts key on exit status and
`$?`, not the synopsis wording. Shows up only when diffing raw stderr against
bash on an intentionally-malformed builtin invocation.

**Proper fix:** give each builtin a canonical `usage:` synopsis string (matching
bash's exact wording) and, on a usage error, emit it as a second line through the
same redirect-aware sink (`errln` for in-`run_builtin` builtins, `emit_cmd_stderr`
for the `command`/`builtin` wrappers). Mechanical but must match bash's synopsis
text byte-for-byte per builtin.
