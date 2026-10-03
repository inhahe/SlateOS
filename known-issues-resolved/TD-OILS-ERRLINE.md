### TD-OILS-ERRLINE. Error diagnostics lack bash's `<name>: line N:` prefix — ✅ RESOLVED 2026-07-19

**Resolution (2026-07-19):** implemented. Every runtime/expansion diagnostic now
carries bash's `<name>: line <N>: ` prefix in non-interactive mode (`osh -c` /
scripts), matching bash's format (the shell *name* still differs — `osh` vs
`bash` — which is intended and unavoidable). Shipped design:

- New `Shell::err_prefix()` returns `"{name}: line {line}: "` when
  `command_mode`/`script_mode` is set, else `"{name}: "` (interactive bash omits
  the line number too). `name` is `$0` (the `-c` pseudo-name `osh` or the NAME
  arg, or the script path); `line` is `current_line` (already backs `$LINENO`,
  verified to track bash including function-relative numbering).
- All ~140 production error sites were routed through `err_prefix()`. This also
  fixed the latent bug where sites hard-coded a literal `osh:` even when running
  a *script* (errors now correctly name the script path, as bash does).
- The `eprintln!(...)` error sites (which bypassed osh's stderr-redirection
  stack, so an error under `cmd 2>file` leaked to the terminal) were converted to
  `self.errln(&format!(...))`, so all diagnostics now honour an active
  `2>`/`2>&1` redirect — bash parity.
- **Subtlety handled:** bash's pure `builtin_usage()` messages
  (`<builtin>: usage: …`, e.g. bare `getopts`/`unalias`/`trap`) are emitted with
  *no* shell-name/line prefix at all — only the builtin name. Those sites
  (getopts/trap/unalias usage) are excluded from `err_prefix()` and now
  byte-match bash. The `getopts` runtime `illegal option`/`requires argument`
  diagnostics use `$0:` *without* a line number in bash, which osh already
  matched (they use `self.name` directly, not the prefix).
- Syntax/parse errors go through `format_parse_error(e, prefix)` (now takes the
  prefix as a parameter) and get `<name>: line N: syntax error: …`. bash inserts
  an extra `-c:` for `-c` parse errors (`bash: -c: line N:`); osh keeps the
  uniform `<name>: line N:` form (name differs anyway).
- Tests: `error_prefix_includes_line_number_in_command_mode` covers the prefixed
  runtime errors (line 1 & line 2), the redirect-honouring command-not-found, and
  the unprefixed usage line. All 550 oils tests pass; clippy clean; both host and
  slateos targets build.

Remaining related nicety: the arithmetic error *body* was tracked separately as
TD-OILS-ARITH-ERRFMT and is now **RESOLVED** (2026-07-19) — osh emits bash's full
`<name>: line N: [<builtin>: ]<expr>: <body> (error token is "…")` form.

**Original report follows for reference.**
