### TD-OILS21. `BASH_SOURCE`/`caller` do not track *per-function* definition source across `source`/`.` — ✅ RESOLVED 2026-07-27

**Where:** `userspace/oils/src/interp.rs` (`Shell.refresh_funcname`/`frame_source`
build `BASH_SOURCE` from a single mode-derived label for every function frame;
`Shell.builtin_caller`/`bash_source_at` do the same), `Shell.funcs` (a name→body
map with no definition-site metadata).

**Status (2026-07-19, partially fixed):** the *invocation-mode* dimension is now
correct. `frame_source` returns the label bash uses for function frames —
`environment` under `-c`, `main` under stdin/interactive, and the script path
(`$0`) in script-file mode — and `refresh_funcname` now materialises the
script-file **base frame** (`BASH_SOURCE[0]`/`BASH_LINENO[0]` = script path / 0
even at top level, with `FUNCNAME` gaining its bottom `main` entry only once a
function frame sits above it, so the arrays legitimately differ in length at a
script's top level). `caller`/`caller N` were reworked to bash's indexing
(line `BASH_LINENO[n]`, name `FUNCNAME[n+1]`, source `BASH_SOURCE[n+1]` with a
`NULL` fallback for a top-level caller), so a `caller 0` from a lone function
under `-c` is now correctly out-of-range and the source column matches bash in
every mode. Verified byte-for-byte against real bash in `-c`, stdin, and
script-file modes.

**Gap that remained after that pass:** every frame shared one source label,
because the interpreter stored only a function's body (`funcs: name → Program`)
with no record of which file it was defined in. In bash, a function defined in a
file pulled in via `source`/`.` reports *that* file as its `BASH_SOURCE` entry,
while ours reported the current mode's label. This mattered for scripts that
`source` a library of functions and then introspect `BASH_SOURCE`/`caller` to
locate the defining file (e.g. stack-trace/error-reporting frameworks).

**Resolution (2026-07-27):** fixed as part of BUG-OILS-SOURCED-FILE-ERROR-NAME
above, which replaced the `source_depth` counter with a real interleaved frame
stack. `Shell::func_sources` now records each function's definition file
(written at `Command::Function`, cleared by `unset -f`), `fn_source_stack`
carries it per live call frame, and `merged_frames()` is the single source of
truth behind `funcname_at`/`bash_lineno_at`/`bash_source_at`/`refresh_funcname`
and hence `caller`. See that entry for the measured bash model and the
regression tests.
