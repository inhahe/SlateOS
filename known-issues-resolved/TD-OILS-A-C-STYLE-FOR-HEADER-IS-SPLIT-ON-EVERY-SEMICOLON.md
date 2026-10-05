### TD-OILS-A-C-STYLE-FOR-HEADER-IS-SPLIT-ON-EVERY-SEMICOLON. `for ((${x:-;}; 0;))` was a syntax error — 2026-08-07 — ✅ FIXED 2026-08-07

**Where:** `userspace/oils/src/parser.rs` — `Parser::parse_for_arith` and the new
free function `arith_for_sections`.

**What was wrong.** osh carved the header with `raw.split(|&b| b == b';')`. bash
does not split on a raw semicolon at all: each section runs up to the next `;`
found by `skip_to_delim (start, 0, ";", SD_NOJMP|SD_NOPROCSUB)`
(`make_arith_for_command`, make_cmd.c:288), which steps *over* a `\c` pair,
`'…'`, `"…"`, `` `…` ``, `$( … )`, `${ … }`, `$'…'` and `$"…"` — so a `;` inside
any of those is part of the section. What it does **not** step over, because the
flag set says so (subst.c:2181): a `[ … ]` subscript (that wants `SD_GLOB`), a
`<( … )` / `>( … )` (suppressed by `SD_NOPROCSUB`), a bare `( … )` or a `? :`
(those want `SD_ARITHEXP`) and `$[ … ]` (which `skip_to_delim` does not know).
So `a[1;2]` really is two sections in bash.

Nor is a bad count "not a C-style for loop": bash reports it with two
`parser_error` lines and status 2 (make_cmd.c:311–319) — so no source line is
echoed under them — and blames the line the `((` was read on
(`arith_for_lineno`, parse.y:4469), not the `done` the parser has reached by
then, because the header is counted in the grammar's *reduction* (parse.y:881).

```
syntax error: arithmetic expression required      # fewer than 3 sections
syntax error: `;' unexpected                      # more than 3
syntax error: `((${; 0;))'                        # always, with the raw text
```

**Repro** (bash 5.2.37 left, osh right):

| input | bash | osh |
|---|---|---|
| `for ((${x:-;}; 0;)); do :; done` | `((: ;: syntax error: operand expected`, rc 1 | `C-style for loop requires …`, rc 2 |
| `x=1; for ((n=${x:-;}; n<2; n++)); do echo n=$n; done` | `n=1` | `C-style for loop requires …` |
| `for (("1;2"; 0;)); do :; done` | `((: 1;2: … invalid arithmetic operator` | `C-style for loop requires …` |
| ``for ((`echo 1;2`; 0;)); do :; done`` | `2: command not found`, then rc 0 | `C-style for loop requires …` |
| `for (($(echo 1;2); 0;)); do :; done` | `2: command not found`, then rc 0 | `C-style for loop requires …` |
| `for ((a[1;2]=0; 0;)); do :; done` | ``syntax error: `;' unexpected`` + ``syntax error: `((a[1;2]=0; 0;))'`` | `C-style for loop requires …` |
| `for ((${; 0;)); do :; done` | `syntax error: arithmetic expression required` + the header echo, rc 2 | `${: bad substitution`, rc 1 |

**Fixed by** `arith_for_sections`, which walks the raw header with the existing
`skip_construct` — the scanner that already models exactly this flag set — and
by `parse_for_arith` emitting bash's two-message diagnostic at the `((`'s own
line after the whole `do … done` has been read. Reporting two messages under one
error needed `ParseError::msg: Str` to become `msgs: Vec<Str>`, so that the
`line N:` prefix goes on each *message* rather than each physical line (the
header echo quotes back a header that may itself span lines). Covered by
`tests/corpus/a-c-style-for-header-is-carved-by-its-own-scan.sh`.
