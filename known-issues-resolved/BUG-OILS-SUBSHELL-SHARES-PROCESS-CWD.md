### BUG-OILS-SUBSHELL-SHARES-PROCESS-CWD. `cd` inside a subshell escapes it, because osh emulates subshells in-process and the working directory is process-global — 2026-07-27 — ✅ RESOLVED 2026-07-27

**Symptom.** bash runs every subshell in a forked process, so a directory
change inside one is invisible to the parent. osh emulates subshells by
cloning `Shell` (`clone_for_subshell`, `interp.rs`), and `cd` calls
`std::env::set_current_dir` — which is *process*-global. Every subshell
form therefore leaks its cwd:

```sh
mkdir -p a
pushd a >/dev/null          # cwd = ./a
popd | cat                  # popd runs in a pipeline stage → a subshell
echo "${PWD##*/}"           # bash: a     osh: a   (the *variable* is fine)
cd a | cat                  # bash: "cd: a: No such file or directory"
                            # osh : silently succeeds — the process cwd was
                            #       already moved to the parent dir by `popd`
```

The shell *variable* `$PWD` is correctly scoped (it lives on the `Shell`
clone), so the divergence shows up as a mismatch between `$PWD` and the
real `getcwd()`: every relative path the parent then opens resolves
against the subshell's directory.

**Worse: it is a data race.** Pipeline stages run *concurrently* on
scoped threads (`exec_pipeline`, `interp.rs` ~line 2295), and background
jobs / coprocs run on detached threads (~7136, ~7194). Two stages that
both `cd` fight over one process-global directory, and any relative path
opened by a third stage resolves unpredictably. `a | b` where either side
does `cd` is nondeterministic today.

**Reproduce.**

```sh
osh -c 'R=$PWD; mkdir -p a; pushd a >/dev/null; popd | cat; cd a | cat; echo "cwd=$(pwd)"'
```

bash reports `cd: a: No such file or directory` from the last stage;
osh does not, because its real cwd had already been moved.

**Fix (2026-07-27).** The working directory is now *per-`Shell`* rather
than per-process — the same way `$PWD`, the fd table and the variable
namespace already were. `std::env::set_current_dir` is gone from the
interpreter entirely.

1. `Shell.cwd: String` (absolute, forward-slash, produced by the existing
   `shell_path`), seeded once from `std::env::current_dir()` in
   `Shell::new`; `clone_for_subshell` copies it like any other field.
2. Free function `resolve_against(cwd, path)` plus the `Shell` wrappers
   `resolve` (→ `PathBuf`), `resolve_str`, `host_path` (adds the
   `map_device_path` device mapping) and `resolve_program` (for
   `Command::new`). Every filesystem access in `interp.rs` routes through
   one of them. The free functions that touch the filesystem —
   `eval_unary`, `file_cmp`, `eval_binary`, `glob_or_literal`,
   `glob_expand_field`, `globstar_walk`/`globstar_descend`, `open_out`,
   `open_rw`, `open_output_target` — take the cwd as their first
   parameter.
3. `change_dir` resolves the target against `self.cwd`, folds `.`/`..`
   out textually via the new `normalize_logical` (bash's *logical* `cd`,
   so `cd sym/..` returns where the user came from), confirms the result
   is a directory with `fs::metadata`, and assigns `self.cwd`. The two
   diagnostics `set_current_dir` used to supply for free — "No such file
   or directory" and "Not a directory" — are produced explicitly and were
   re-verified against bash 5.2.
4. Every `PCommand::new` site resolves the program with `resolve_program`
   and sets `.current_dir(&self.cwd)`. (The program path must be resolved
   *as well as* setting `current_dir`: on the Windows host `CreateProcess`
   looks a relative application name up against the calling process's
   directory, not the child's.)

**Regression test.** `tests/corpus/subshell-cwd.sh` — covers `( )`,
`$( )`, single- and multi-stage pipelines, relative redirects, globs,
`test`, `$(< f)`, `source`, `cd -`/`$OLDPWD`, and two concurrent pipeline
stages sitting in different directories at the same time.
