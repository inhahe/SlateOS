### TD-OILS-A-CMDSUB-REPRINT-IS-RE-READ-WITH-ITS-TAIL. `"$(echo "${x:-$'a\0b'}")"` names `"` in bash, `}` in osh — 2026-08-08 — ✅ FIXED 2026-08-08

**Where:** `userspace/oils/src/parser.rs` — `comsub_reprint_error`,
`reprint_read_past_paren`, `ComsubReprintError`, `parse_paren_body_tokens`;
`userspace/oils/src/interp.rs` — `Shell::comsub_reparse_error`.

**What.** Found while fixing
`TD-OILS-A-NUL-IN-AN-ANSI-C-STRING-DOES-NOT-TRUNCATE-THE-TRANSLATION`, whose cut
is the only thing that can leave a `$( … )` re-print with a construct still open.
`xparse_dolparen` (parse.y:4248) is handed a pointer **into the stored word** —
`extract_command_subst` passes `string + *sindex` (subst.c:1290) — so the text
being read is the re-print, the `)`, and the rest of the word after it, with
`shell_eof_token = ')'`. osh parsed the re-print **alone**, which is
indistinguishable for every re-print that reads back and wrong for every one that
does not.

Three failures are possible, and osh got all three wrong:

| the re-print… | bash | osh (before) |
|---|---|---|
| reads back, grammar error | names a token, echoes the line | ✅ (this part was right) |
| is unclosed, still unclosed at the word's end | `parse_matched_pair`'s own `parser_error (start_lineno, …)` (parse.y:3711), printed **alone** — it sets `PST_NOERROR` "avoid redundant error message" | named `}` instead of `"`, and echoed an offending line under it |
| is unclosed, the tail closes it | the `)` was spent on the construct, so `comsub` never gets one: `unexpected EOF while looking for matching `)'` (parse.y:6289), also alone | not modelled at all |

Two further errors of the same shape were fixed with it: osh echoed an offending
source line under an end-of-input diagnostic (bash calls `print_offending_line`
only from the branches of `report_syntax_error` that *name a token*,
parse.y:6251-6264), and osh treated a re-parse lex error as fatal (rc 2, shell
aborted) where bash does a `DISCARD` (rc 1, script continues) — the lex path of
`parse_cmdsub_body_unmarked` called `.in_paren_body()` on *every* lex error,
contradicting its own contract.

The line numbering falls out of `line_number` being incremented as a line is
**fetched** (parse.y:2361), including the fetch that discovers EOF, so the third
row is numbered exactly one further on than the second for the same shape.

**Fixed by** `parser::comsub_reprint_error(src, tail, close_line, opts) ->
Option<ComsubReprintError>`, which distinguishes the three by *whether the lex of
`src` alone succeeded* and returns the message, whether the line is echoed, the
line offset, and whether the failure is fatal. `parse_cmdsub_body_unmarked` was
split so its tokenize step is separable (`parse_paren_body_tokens`).
