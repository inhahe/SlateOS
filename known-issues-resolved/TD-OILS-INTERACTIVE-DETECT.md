### TD-OILS-INTERACTIVE-DETECT. Non-tty stdin is treated as interactive — FIXED 2026-07-20

**Where:** `userspace/oils/src/interp.rs` — `shopt_default`/`aliases_enabled`
(the `expand_aliases` default gate) and, more broadly, everywhere `osh`
decides "am I an interactive shell?"; `userspace/oils/src/main.rs` — `run_source`
dispatch and the REPL loop that prints prompts.

**Symptom:** bash decides interactivity by testing `isatty(stdin)` (plus the
`-i` flag and whether a script/`-c` was given). `osh` currently approximates
interactivity purely by mode flags: `command_mode` (`-c`) and `script_mode`
(script file) are non-interactive, and *everything else* — including a REPL
reading from a **pipe or redirected file** (`echo 'cmd' | osh`, `osh < file`) —
is treated as interactive. Two observable divergences follow:

```
printf 'alias ll="ls -l"\nll\n' | bash    → ll: command not found   (aliases OFF: non-interactive)
printf 'alias ll="ls -l"\nll\n' | osh     → runs `ls -l`            (aliases ON: osh thinks it's interactive)
echo pwd | bash                            → (no prompt printed)
echo pwd | osh                             → prints the PS1 prompt before running pwd
```

So `osh` (a) expands aliases when piped stdin should have them off by default,
and (b) prints prompts to a non-tty. Both stem from the same missing check.

**Proper fix:** add an `is_interactive()` predicate that mirrors bash:
interactive iff (`-i` given) OR (no `-c`, no script arg, AND `isatty(0) &&
isatty(2)`). Wire `stdin`/`stderr` tty detection (Windows: `GetFileType` /
`_isatty` on the raw handle; POSIX: `libc::isatty`) into a cached bool on the
shell set once at startup. Then: `shopt_default("expand_aliases")` returns
`is_interactive()` instead of `!command_mode && !script_mode`; the REPL only
prints `PS1`/`PS2` when interactive; and this same predicate feeds the
TD-OILS-ERRLINE `line N:` gate (bash omits the line number only for *interactive*
input, and piped-stdin is non-interactive there too). Until then, the
mode-flag approximation is correct for the common `-c`/script/tty-REPL cases and
only wrong for the rarer piped-/redirected-stdin REPL.

**Fix (2026-07-20):** added a `repl_interactive` flag on `Shell` plus a single
`is_interactive()` predicate = `!command_mode && !script_mode && repl_interactive`
(mirrors bash's `interactive` global). The binary (`main.rs`) sets the flag from
`std::io::IsTerminal` — interactive iff `stdin.is_terminal() && stderr.is_terminal()`
— before entering the REPL, with `-i` / `+i` command-line overrides
(`force_interactive`). `is_interactive()` now drives all four divergent
behaviours in one place: (a) `shopt_default("expand_aliases")` returns it, so a
piped REPL no longer expands aliases by default; (b) the REPL prints `PS1`/`PS2`
and the EOF newline only when interactive, so `echo pwd | osh` emits no prompt;
(c) `job_control_enabled()` uses it; and (d) `err_prefix`/`syntax_error_prefix`
now show the `line N:` token for *any* non-interactive input (piped stdin
included), not just `-c`/script. Default is `true` so the unit-test harness
(which builds a `Shell` directly, no mode) stays interactive-like. Regression
tests: `noninteractive_repl_disables_alias_default`,
`command_and_script_modes_are_never_interactive`,
`noninteractive_repl_shows_line_number_in_errors`.

**Residual caveat (host vs. SlateOS target):** tty detection uses
`std::io::IsTerminal`, which is accurate on the host (where osh's bash-fidelity
tests run). On the SlateOS target, `IsTerminal` is only as good as std's port of
`isatty`/`GetFileType` for that platform; if it reports `false` on a real SlateOS
console, an on-target interactive `osh` would lose its prompt/alias default. When
osh is deployed as a SlateOS shell, verify the target's `isatty` wiring (or pass
`-i`). This does not affect the host fidelity this item was about.
