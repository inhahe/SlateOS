### BUG-OILS-SOURCED-FILE-ERROR-NAME. Errors inside a `.`/`source`d file are tagged with the *outer* script's name — 2026-07-27 — ✅ RESOLVED 2026-07-27

**Symptom.** With `sub.sh` containing `echo x` then `v='abc`:

```sh
. ./sub.sh
```
```
bash: x / ./sub.sh: line 2: unexpected EOF while looking for matching `''
osh : x / case.sh:   line 2: unexpected EOF while looking for matching `''
```

bash retargets `$0`-in-diagnostics to the file currently being read; osh kept
the top-level script name. (The line number itself was already correct.)

**Root cause.** osh modelled `source` as a bare `source_depth: u32` counter —
enough to make `return` legal inside a sourced file, but carrying no *identity*.
Everything that needs to know "which file am I in" therefore fell back to the
single `name` field: `err_prefix`/`syntax_error_prefix`, and the
`FUNCNAME`/`BASH_SOURCE`/`BASH_LINENO`/`caller` machinery, which only knew about
function frames.

bash's actual model, measured against 5.2.37 with ~30 probes:

* `source`/`.` pushes a frame onto the **same stack as function calls**, so the
  two interleave. `FUNCNAME` shows a source frame as the literal name `source`.
* `BASH_SOURCE[i]` is a *function's definition file* (a function defined by a
  sourced script names that script forever) or a source frame's path **exactly
  as written** on the `.` command line.
* `BASH_LINENO[i]` is the line at which frame `i` was entered.
* `FUNCNAME` is left **unset entirely** whenever no *function* frame is active —
  even at the top level of a sourced file, where `BASH_SOURCE`/`BASH_LINENO` do
  show the frame. (bash unbinds the variable when `variable_context == 0`; the
  underlying array still holds the `source` entries.) This makes the three
  arrays legitimately differ in length.
* Diagnostics are labelled with `BASH_SOURCE[0]` (bash's `get_name_for_error`).
* Subshells and command substitutions **inherit** the source stack, so
  `( return 3 )` inside a sourced file is legal and yields 3.

**Fix.** `userspace/oils/src/interp.rs` — replaced the counter with a real
interleaved frame stack rather than prefix-patching the error sites:

1. New `struct SourceFrame { path, call_line, fn_depth }`; `Shell::source_stack:
   Vec<SourceFrame>` replaces `source_depth`. `fn_depth` records
   `fn_stack.len()` at push time, which is enough to rebuild the merged order.
2. New `Shell::func_sources: HashMap<String, String>` records each function's
   *definition* file (populated at `Command::Function`, cleared by `unset -f`),
   and `fn_source_stack` carries that value per live call frame.
3. New `merged_frames()` interleaves the two stacks outermost-first; it is now
   the single source of truth for `funcname_at`/`bash_lineno_at`/
   `bash_source_at`/`refresh_funcname`, so `caller` follows for free.
4. New `error_source()` (= `BASH_SOURCE[0]`, degrading to `$0` with no frames)
   backs both `err_prefix` and `syntax_error_prefix`.
5. The subshell clone copies `source_stack`, so `return` works inside `( )` and
   `$( )` within a sourced file.

**Regression tests.** `interp::tests::source_frames_interleave_with_function_frames`,
plus corpus cases `source-error-name.sh` (runtime, syntax, function-body,
readonly-builtin and nested-source diagnostics) and `source-frames.sh`
(the frame arrays, `caller`, subshell/command-substitution inheritance).
This also closes **TD-OILS21** (per-function definition source not tracked).
