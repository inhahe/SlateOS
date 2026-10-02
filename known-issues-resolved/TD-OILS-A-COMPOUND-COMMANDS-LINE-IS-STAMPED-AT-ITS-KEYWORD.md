### TD-OILS-A-COMPOUND-COMMANDS-LINE-IS-STAMPED-AT-ITS-KEYWORD. `case`, `for`, `select` and `[[ … ]]` report the keyword's line where bash reports a later one — 2026-08-08 — RESOLVED 2026-08-08

**Where:** `userspace/oils/src/ast.rs` — `CaseClause`, `ForClause`,
`SelectClause` and the `Cond` command carry no `line` of their own, so
`Shell::current_line` stays at the enclosing `Item::line`
(`ast.rs:65`), which the parser stamps at the item's first token.

**What.** bash stamps these three families *after* reading something, not at
the keyword:

- `case` / `for` / `select` take `word_lineno[word_top]`, assigned in
  `read_token_word` **after the controlling word has been read**
  (parse.y:5356, in the `case CASE: case SELECT: case FOR:` arm), and handed to
  `make_case_command` / `make_for_command` / `make_select_command`
  (parse.y:949, 839, 907). `execute_case_command` then does
  `SET_LINE_NUMBER (case_command->line)` (execute_cmd.c:3545) before expanding
  either the subject or any pattern.
- `[[ … ]]` takes its *root* `COND_COM`'s line — `make_cond_node` stamps
  `line_number` as each node is built (make_cmd.c:463) and
  `make_cond_command` copies the root's (make_cmd.c:486) — and the root is
  built **last**, so the line is where the whole `[[ … ]]` finished parsing.
  `execute_cond_command` does `SET_LINE_NUMBER (cond_command->line)`
  (execute_cmd.c:4030).

Both are observable through any diagnostic raised while the words are expanded:

```sh
case "a
b" in
"y${nope?bad}") ;;
esac
# bash: line 2 (the line the case word ends on)   osh: line 1
```

```sh
[[ ${nope?bad} == x &&
y == y ]]
# bash: line 2 (the line the `]]' is on)          osh: line 1
```

The same shift shows up in the command-substitution re-parse diagnostic, whose
line is the *executing command's* — `case "y$(⏎!⏎)z" in x) ;; esac` reports
line 5 in bash and line 3 in osh.

**The proper fix** is a real one, not a patch at the reporting site: give
`CaseClause`, `ForClause`, `SelectClause` and the cond command a `line` field
of their own, stamp them in the parser where bash does (after the controlling
word for the first three; at the closing `]]` for the last), and have
`exec_case` / `exec_for` / `exec_select` / `exec_cond` set `current_line` from
it the way the simple-command driver already sets it from
`SimpleCommand::line`. `for (( … ))` wants checking in the same pass —
`arith_for_lineno` is assigned alongside `word_lineno` at parse.y:4469.

**Impact.** Cosmetic but broad: every diagnostic raised while expanding a
`case` subject or pattern, a `for`/`select` word list, or a `[[ … ]]` operand
carries the wrong line whenever the construct spans more than one line.
`$LINENO` inside the *body* is unaffected (each command there carries its own
line).

**Found by** the probe matrix for
TD-OILS-A-CMDSUB-BODY-IS-RE-READ-AS-WRITTEN-NOT-AS-REPRINTED (probes `c3`,
`d2`, `e3`, `f1`–`f4`).

**Resolved.** `CaseClause`, `ForClause`, `SelectClause`, `ForArithClause` and a
new `CondClause` each carry a `line` stamped in the parser where bash stamps
it, and `Shell::on_control_line` brackets each executor's header the way bash
brackets `line_number` with `save_line_number` — so the body keeps its own
lines and the surrounding line is put back afterwards. Covered by
`tests/corpus/a-compound-commands-line-is-stamped-after-what-controls-it-not-at-its-keyword.sh`.

Two things this entry got wrong, corrected by measurement:

- The `[[ … ]]` root is **not** always built last. `make_cond_node` stamps each
  node as it is built (make_cmd.c:463), and a *term* — binary, unary, bare word
  or `( … )` — is built the moment its last token has been read, **before**
  `cond_skip_newlines` fetches anything further. So `[[ ${nope?bad} == x⏎]]` is
  line 1, not the `]]`'s line 2. Only `&&`/`||` are built after their
  right-hand term's newline-skip has already read past the newline, which is
  why *those* land on whatever closes the expression. `!` builds no node at
  all — it sets a flag on the node it was handed — so a negated term keeps the
  term's line.
- `case`/`for`/`select` assign `line_number` **directly**, not through
  `SET_LINE_NUMBER`; only Simple, Subshell, Arith and Cond use the macro. The
  difference is that the macro also moves `line_number_for_err_trap`, so those
  three do not move the line an ERR trap reports.

`for (( … ))` turned out to be a fifth family with its own rule:
`arith_for_lineno` is stamped in `parse_dparen` where the `((` is read
(parse.y:4469) — *before* the header is scanned — and
`execute_arith_for_command` brackets **each** of the three expression
evaluations with it separately (execute_cmd.c:3120, 3139-3141, 3171-3174), so
all three sections are blamed on the `((`'s line while the body keeps its own.
