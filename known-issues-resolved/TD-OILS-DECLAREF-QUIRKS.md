### TD-OILS-DECLAREF-QUIRKS. `osh` `declare -f`/`type` deparse differs from bash for four idiosyncratic constructs — 2026-07-19 — ✅ FULLY RESOLVED 2026-07-20 (all four items matched byte-for-byte)

**Where:** `userspace/oils/src/unparse.rs` `command_block` (`If` elif branch,
`Subshell`, `Function` nested case) and `item_stmt`/`program_block`
(background items).

**What:** After the 2026-07-19 byte-fidelity pass, osh's `declare -f`/`type`
output is byte-identical to bash for the common constructs (simple lists,
`if/then/else`, `while`/`until`, `for … in`, `for ((;;))`, `case`, `select`,
nested brace groups). Four bash deparser idiosyncrasies remain unmatched;
all four still emit **valid, re-parsable bash** with equivalent semantics —
only the exact whitespace/keyword layout differs:

1. ~~**`elif` → nested `else if`.** bash rewrites `if a; then …; elif c; then
   …; fi` into `else\n if c; then …; fi` (deeper indentation, extra `fi`).
   osh prints a literal `elif …; then` clause.~~ **RESOLVED 2026-07-20.**
   `unparse.rs` now has a recursive `render_if` that rewrites every `elif` into
   a nested `else { if … fi; }`, indenting one level deeper per `elif` and
   terminating each inner `fi` with `;` (outermost `fi` left for the caller),
   exactly matching bash's `declare -f`/`type`. Verified byte-identical against
   MSYS bash across elif+else, elif-without-else, nested-if-in-elif-body, and
   loop-embedded cases; plain `if`/`if-else` (no elif) unchanged. Regression
   test `interp::declare_f_rewrites_elif_as_nested_else_if`.
2. ~~**Subshell layout.** bash prints `( echo a;\n echo b );` (first statement
   glued to the `(`, continuation dedented). osh uses a clean indented block
   (`(\n    echo a;\n    echo b\n)`).~~ **RESOLVED 2026-07-20.** The `Subshell`
   arm now renders the body as a group at the `(`'s own indent level, strips
   the first line's indent to glue it after `( `, and appends ` )` to the last
   statement — matching bash's `( echo a;\n<ind>echo b )` layout (compound
   commands and backgrounded/pipe'd subshells included). Verified byte-identical
   against MSYS bash. Regression test `interp::declare_f_subshell_glues_parens_to_body`.
3. ~~**Backgrounded statement in a list.** bash keeps `sleep 1 & echo b` on one
   line (`&` as an inline connector). osh puts each `Item` on its own line, so
   `sleep 1 &` and `echo b` split across two lines.~~ **RESOLVED 2026-07-20.**
   `program_block` now treats ` & ` as an inline connector: an item whose
   predecessor was backgrounded is not re-indented and continues on the same
   line, so `a & b & c` stays on one line while a `;`-separated tail still
   breaks. A trailing backgrounded statement ends the block with ` &` (no `;`).
   Verified byte-identical against MSYS bash across inline/mixed/clause/trailing
   cases. Regression test `interp::declare_f_keeps_backgrounded_statement_inline`.
4. ~~**`function` keyword on nested definitions.** bash prints a function
   defined *inside* another function as `function nested () ` (with the
   `function` keyword); top-level defs use `nested () `. osh always omits
   `function`.~~ **RESOLVED 2026-07-20.** The `command_block` `Function` arm
   (only ever reached for nested definitions — top-level defs go through
   `unparse_function`) now prefixes `function `, matching bash regardless of
   the source syntax (`g()` or `function g`). Top-level output unchanged.
   Verified byte-identical against MSYS bash. Regression test
   `interp::declare_f_prefixes_nested_function_with_keyword`.

**Why deferred:** these are rare constructs (subshell/background/nested-fn in
a function body) or a purely cosmetic restructuring (elif), and osh's output
round-trips correctly. Matching bash exactly means replicating quirks with
little practical benefit.

**Proper fix (if pursued):** (1) render elif chains as nested `else { if … }`
with incremented indent and a matching `fi` per level; (2) special-case the
subshell/background inline layouts; (3) thread a "nested function" flag so
inner `Function` defs prepend `function `.

**Follow-on (here-documents) — 2026-07-28, OPEN, deliberate:** here-docs are
now printed as here-docs rather than here-strings (see TD-OILS16/TD-OILS18),
which matches bash for a here-doc attached to an ordinary statement. Three
placement quirks of bash's own printer are deliberately *not* reproduced,
because osh's output is valid bash in every case and bash's is not always:

1. **Body placement.** bash defers a here-doc body to the end of the enclosing
   *statement*; osh flushes it at the end of the *line* the operator was
   rendered on. They agree for a simple command. They differ when the here-doc
   sits in an `if` condition, where bash emits
   `if cat <<EOF; then` / `echo yes` / `cond` / `EOF` — output that no longer
   re-parses, because the body swallows the `then` clause. osh emits the body
   directly after the `if …; then` line, which re-parses correctly.
2. **Pipelines.** bash breaks the line after the `|` (`cat <<EOF |` / body /
   `  tr a-z A-Z`, two-space indent); osh keeps the pipeline on one line and
   puts the body after it. Both re-parse.
3. **Separator suppression.** bash drops the `;` from the statement *after* a
   here-doc statement, but only when the here-doc is the first statement of the
   block (`{ cat <<G; …; echo one; echo two; }` prints `echo one` with no `;`,
   while the same body with any statement before the here-doc prints
   `echo one;`). The rule is an artifact of bash's `was_heredoc` printer state
   and its list nesting; osh always prints the separator. Both re-parse, and
   `userspace/oils/tests/corpus/declare-f-heredoc.sh` sidesteps the shape by
   never opening a block with a here-doc.

**Follow-on (conditional grouping) — 2026-07-28, RESOLVED same day.** The
`[[ … ]]` parser used a parenthesised group only to shape its tree and then
dropped it, so `declare -f` printed `[[ ( a || b ) && c ]]` back as
`[[ a || b && c ]]` — a *different* test, since `&&` binds tighter. Fixed by
keeping a `CondExpr::Group` node that evaluation walks through and printing
renders; redundant groups are echoed too, as bash does. bash's one
normalisation here (a bare-word test prints as the `-n WORD` it means) is now
matched as well. Corpus `declare-f-cond.sh`; test
`interp::declare_f_keeps_conditional_grouping`.

**Follow-on (backtick substitutions) — 2026-07-28, RESOLVED same day.** bash
prints a `$( … )` body back from the parse (normalising spacing) but a
`` ` … ` `` body from the *source*, verbatim. osh re-printed both, which lost
the spacing and — worse — the `` \` `` escapes a nested substitution needs, so
`` echo `echo \`echo n\`` `` printed back as source that no longer parsed. The
lexer now keeps the verbatim span alongside the unescaped body
(`Seg::CmdSub`'s third field, `WordPart::CommandSub::backtick_src`) and the
unparser echoes it. Corpus `declare-f-backtick.sh`; test
`interp::declare_f_prints_backticks_from_the_source`.
