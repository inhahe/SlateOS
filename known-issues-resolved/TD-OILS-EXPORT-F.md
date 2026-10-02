### TD-OILS-EXPORT-F. `export -f` is rejected: a function cannot be passed to a child shell — 2026-08-01 — ✅ **RESOLVED 2026-08-02**

**Where:** `userspace/oils/src/interp.rs` — the `export` builtin's flag loop
accepts only `-n` and `-p`, so `-f` comes back as `invalid option`.

**What:** bash exports a function by putting it in the environment under the
name `BASH_FUNC_<name>%%`, with the value being the function's body in
`() { … }` form; a bash child imports any such variable back as a function.

```
$ bash -c 'f(){ :; }; export -f f; bash -c "declare -F f"'
f
$ osh -c 'f(){ :; }; export -f f'
osh: line 1: export: -f: invalid option
```

An imported function has no definition site the shell watched, so bash reports
it as line 0 of `environment`:

```
$ bash -c 'f(){ :; }; export -f f; bash -c "shopt -s extdebug; declare -F f"'
f 0 environment
```

**Proper fix.** Accept `-f` on `export` (and on `declare -x`, which is the same
path), keep a set of exported *function* names, and serialise each into the
child environment as `BASH_FUNC_<name>%%=() { <body> }` — the body being what
`declare -f` already reconstructs. On startup, recognise that name shape in
`import_environment` and bind the function instead of a variable, recording its
source as `environment` and its line as 0 so `declare -F` under extdebug agrees.
`unset -f` and `export -fn` both have to drop the name from the exported set.

**Impact.** A script that exports a helper to a subshell (`export -f` plus
`find -exec bash -c`, `xargs bash -c`, `parallel`) fails outright rather than
silently misbehaving, so it is visible rather than dangerous. Also blocks
`declare -F`'s `0 environment` form, which is otherwise implemented.

**RESOLVED 2026-08-02** — implemented as specified above, and verified
byte-identical to bash 5.2.37 for every behaviour probed.

The encoding turned out *not* to be what `declare -f` reconstructs. bash has
two function printers, and the environment uses the other one:
`named_function_string(NULL, cmd, 0)` rather than `print_function_def`. It
drops the name and the newline before `{` (`() { ` instead of `NAME () \n{ \n`)
**and indents every nesting level by one space** rather than four-per-level. So
`f() { echo hi; }` encodes as `() {  echo hi\n}` — two spaces, one from the
header, one from the body's indent — and a nested brace group stays at that
same one space rather than deepening.

That second difference forced a change in the printer's shape rather than a
post-hoc reformat: a newline *inside a string literal* is emitted verbatim at
column 0 in both forms, so a line-based re-indent of `declare -f` output would
corrupt the literal. `userspace/oils/src/unparse.rs` therefore replaced the
hard-coded `fn ind(level: usize)` with an `Indent` newtype (`base`, `step`,
`level`) threaded through the printer, with `Indent::DECLARE` = 4-per-level and
`Indent::EXPORTED` = 1-at-every-level; `impl Add<usize> for Indent` keeps the
recursive call sites reading as the plain `level + 1` they were before. The new
`unparse_function_exported` is the only user of the second shape.

The rest, in `interp.rs`: a `Shell::exported_funcs` set (cloned into subshells,
since only the *export attribute* crosses the environment — an unexported
function does not); `apply_child_env` serialising each into
`BASH_FUNC_<name>%%`; `env_function_name` + `import_env_function` recognising
and binding the shape in `import_environment` — placed *ahead* of the
identifier gate, which would otherwise park the entry in `opaque_env` and pass
stale text to grandchildren; `export -f`/`-nf`/`-pf` in `builtin_export`;
`declare`'s sign-aware flag scan so `declare +x -f f` reaches the function path
while `declare +f` still does not; and `unset -f` dropping the attribute.

Measured details worth recording, each confirmed against real bash:

- The attribute letters list in a fixed order `f`, `r`, `t`, `x` — `declare -F`
  prints `declare -frtx f` whatever order they were set in. `declare -F NAME`
  (with an operand) prints only the name, never the attribute line.
- `declare -Fx` filters to the exported functions; `-Fxr` is a *union*, not an
  intersection, like the variable-side attribute filters.
- `unset -f` takes the export attribute with the definition: a redefinition
  afterwards is **not** exported again.
- A value that does not begin `() {` is skipped in silence (it is an ordinary
  variable that happens to be named like the encoding, and stays invisible —
  the name is unspellable). Only a value that *did* look like a definition and
  then failed to parse earns the three-line diagnostic, whose middle line
  echoes the reconstructed source.
- `export -f nosuch` reports `export: nosuch: not a function` and returns 1 but
  keeps processing later names; `declare -fx nosuch` is silent and returns 1.
  `-f` also stops `h=1` being an assignment, so it is reported the same way.

Not modelled: bash's `-p` (privileged) mode skips function import entirely.
osh accepts `privileged` as an inert `set` option and has no real privileged
mode, so there is nothing to gate on.

**Tests.** `unparse::tests::exported_form_matches_bash` /
`exported_form_reparses` / `exported_empty_body_uses_noop` pin the encoding for
seven shapes and prove it is a fixed point (so a chain of exec'd shells cannot
drift); `interp::tests::export_f_puts_the_function_in_the_child_environment`
and six siblings cover the child environment, the import, the attribute
letters, the listings and the errors; and
`userspace/oils/tests/corpus/export-f-exported-functions.sh` runs the whole
thing — including a real parent→child→grandchild round trip via `$BASH`, which
names the running shell in both — against real bash.
