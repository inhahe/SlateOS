### TD-OILS-DOLLAR-ZERO-ARGV0. `$0` under `-c` (and the bare REPL) is the literal `osh`, where bash uses the path it was invoked as — 2026-08-02 — ✅ **RESOLVED 2026-08-02**

**Where:** `userspace/oils/src/interp.rs` line ~3719 seeds `name: b"osh".to_vec()`,
and `userspace/oils/src/main.rs` (`InvokeMode::Command` ~line 501,
`InvokeMode::Repl` ~line 523) only calls `sh.set_name()` when there is an
explicit operand — the `-c` *name* argument, or a script path. With neither,
`$0` keeps that seeded default and never sees `argv[0]`.

**What:** bash sets `$0` from `argv[0]` at startup, so `-c` without a name
operand reports the invocation path, and every diagnostic prefixed with `$0`
carries it too.

```
$ bash -c 'echo $0'      $ osh -c 'echo $0'
C:/Program Files/Git/usr/bin/bash        osh
```

The prefix divergence is visible on any `-c` diagnostic, and specifically on
the exported-function import error, which is emitted *before* `-c` has settled
`$0`, so even passing the name operand does not align the two:

```
$ env 'BASH_FUNC_w%%=() { if true; }' bash -c 'declare -F w' myname
C:/Program Files/Git/usr/bin/bash: w: line 0: syntax error near unexpected token `}'
…
$ env 'BASH_FUNC_w%%=() { if true; }' osh -c 'declare -F w' myname
osh: w: line 0: syntax error near unexpected token `}'
…
```

**Proper fix.** Seed `Shell::name` from `argv[0]` in `main.rs` before anything
that can diagnose (the environment import included), and let the existing
`set_name` calls override it when `-c name` or a script path is given. The
seeded `b"osh"` becomes the fallback for the case where `argv[0]` is absent.

**Impact.** Cosmetic but pervasive: every `-c`-mode diagnostic is prefixed
differently from bash, and a script that keys off `$0` to re-exec itself
(`exec "$0" …`) breaks under `osh -c`. Normalised away in
`tests/corpus/export-f-exported-functions.sh`, which is the only corpus case
that currently trips over it. Found while writing that case.

**RESOLVED 2026-08-02** — `main.rs`'s `run` now seeds `$0` from `argv[0]`
before `import_environment()`, and the existing `set_name` calls for the `-c`
*name* operand and for a script path still override it.

Two consumers were asserting the old, *wrong* behaviour and were corrected
rather than accommodated — real bash was checked in each case:

- `tests/cli_options.rs` expected `osh: line 2: g: command not found` from a
  `-c` run with no name operand. bash prints the invocation path there
  (`echo nosuch | bash` → `C:/Program Files/Git/usr/bin/bash: line 1: …`), so
  the expectations now build the prefix from a documented `shell_name()`
  helper (`env!("CARGO_BIN_EXE_osh")`). The diagnostics emitted *before* the
  shell starts — `osh: -z: invalid option` and the rest of the option parser —
  are literals in `main.rs` and are correctly unaffected.
- `tests/corpus/for-in-list-words.sh` folded the shell name with
  `sed 's/^[^:]*: -c: /SH: -c: /'`. `[^:]*` cannot span a Windows path, whose
  drive letter carries a colon of its own, so the fold silently stopped
  working once `$0` became a path. Widened to `^.*: -c: `.

Full suite green (1147 + 4 + 39 + 7 + doctests), clippy clean, corpus sweep
261 matched / 0 failed.
