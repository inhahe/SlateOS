### TD-OILS-ERRLINE (original report — SUPERSEDED by the ✅ RESOLVED entry above, kept for reference). Error diagnostics lack bash's `<name>: line N:` prefix — 2026-07-19

**Where:** `userspace/oils/src/interp.rs` — all ~40 error-emission sites, which
hardcode a bare `osh: ` prefix in three different ways: `eprintln!("osh: …")`,
`self.errln(&format!("osh: …"))`, and `self.emit_stderr(format!("osh: …"))`.

**Symptom:** bash prefixes every non-interactive runtime/expansion diagnostic
with `<$0>: line <N>: ` — the source name (`$0`: `bash`/the NAME arg for `-c`,
or the script path for a file) plus the 1-based source line of the failing
command. `osh` emits only `osh: <msg>` with no line number and no script name:

```
bash -c 'foo'            → bash: line 1: foo: command not found
bash -c 'foo' myname     → myname: line 1: foo: command not found   ($0 == prefix)
bash script.sh (line 47) → script.sh: line 47: foo: command not found
osh  -c 'foo'            → osh: foo: command not found
```

The `line N:` component is a real debugging-usability feature (it points the
user at the failing line in a long script), so this is a genuine bash-superset
gap, not mere cosmetics. Interactive (tty) bash omits the line number, matching
`osh`'s current behavior — the divergence is only in the non-interactive
`-c`/script/piped-stdin modes.

**Infrastructure already present:** `self.current_line` (backs `$LINENO`, verified
to track bash) supplies N; `self.name` (settable via `set_name`, == `$0`) supplies
the prefix name; `self.command_mode`/`self.script_mode` distinguish the
non-interactive modes that get the line number.

**Proper fix:** add a single `fn diag(&mut self, msg: &str)` helper that emits
`{name}: line {line}: {msg}\n` in `command_mode`/`script_mode` (else `{name}: {msg}\n`)
**through `emit_stderr`** so the diagnostic also respects active `2>` redirections,
then convert every error site to call it. That conversion also fixes a *second*
latent bug: the `eprintln!("osh: …")` sites write to the real process stderr and
so **bypass osh's stderr-redirection machinery** (an error under `cmd 2>file`
leaks to the terminal instead of the file) — routing all diagnostics through
`emit_stderr` corrects that too. Subtleties to preserve: builtin usage lines
(`read: usage: …`) are emitted *without* the line prefix (bash keeps only the
primary diagnostic prefixed); the piped-stdin REPL is non-interactive in bash
but currently indistinguishable from a tty REPL in `osh` (a `--`/isatty check is
a follow-up). ~40 emission sites + ~15 test assertions to update — see
`open-questions.md` (flagged for the operator because it reformats *every* error
message the shell prints, a pervasive user-visible output change).
