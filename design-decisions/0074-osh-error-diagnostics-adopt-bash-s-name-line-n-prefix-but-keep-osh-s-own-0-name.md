## 74. osh error diagnostics — adopt bash's `<name>: line N:` prefix, but keep osh's own `$0` name (not bash's `environment` pseudo-name) and a uniform syntax-error form

**Date:** 2026-07-19
**Decided by:** Operator authorized the overall feature (the operator directed
"Continue porting a bash-compatible shell from oils" / "port all of it" and
recorded the pro-`line N:` argument in `todo2.txt`, lifting the prior gate on
TD-OILS-ERRLINE); **Claude (autonomous)** made the implementation sub-calls
below. See known-issues.md TD-OILS-ERRLINE for the full shipped writeup.

**Context.** bash prefixes non-interactive runtime diagnostics with
`<$0>: line <N>: `. osh previously emitted only `osh: <msg>` (no line, and it
hard-coded `osh:` even for scripts). Adopting the prefix is a real
debugging-usability win. Byte-matching bash is impossible regardless, because
osh's `$0` is `osh`, not `bash` — so this is about format fidelity for SlateOS's
own shell, not literal equality.

**Sub-decisions (Claude autonomous tradeoffs).**
- **Function-scope source name.** Inside a `-c`-defined function, bash reports the
  magic source name `environment` (`environment: line N:`). osh keeps its own
  `$0`-based name (`osh: line N:`) instead. *Pro:* consistent, meaningful name;
  osh's name differs from bash anyway so mirroring the magic string buys nothing.
  *Con:* one more surface where the literal text diverges from bash. Chosen: the
  meaningful name. (Function-relative *line numbers* DO match bash.)
- **Syntax/parse errors.** bash inserts an extra `-c:` for `-c` parse errors
  (`bash: -c: line N: syntax error…`). osh uses the uniform
  `<name>: line N: syntax error…` form (no `-c:` insert). *Pro:* one code path,
  no special-casing of the invocation channel; the name differs anyway. *Con:*
  the `-c:` token is absent. Minor; chosen for simplicity.
- **`line N:` gated to non-interactive mode.** Matches bash (interactive bash
  omits the line number). osh's REPL therefore stays `osh: <msg>`.
- **`eprintln!` → `errln`.** Converted all error sites off `eprintln!` (which
  bypassed osh's stderr-redirect stack) onto `errln`/`emit_stderr`, so a
  diagnostic under `cmd 2>file` now goes to the file — bash parity, and a latent
  bug fix independent of the prefix itself.
- **Pure `builtin_usage()` lines stay unprefixed.** bash prints
  `<builtin>: usage: …` with no shell-name/line prefix; osh matches that exactly
  for the getopts/trap/unalias usage messages (excluded from the prefix helper).

**How to reverse.** Error-message formatting is trivially reversible: the prefix
is produced in one place (`Shell::err_prefix()`), so the format can be changed or
reverted centrally without touching the ~140 call sites again.

**Where it lives.** Strategy note only (no new code). Related: §72 (OSH),
§69 (giant-port ordering — Mesa/Chromium/WINE supply the C++ toolchain
prerequisite). Roadmap: YSH tracked as blocked-on-C++-toolchain under the Oils
line.
