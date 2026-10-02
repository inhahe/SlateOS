### TD-OILS-A-QUOTED-SUBSTITUTIONS-OPERAND-IS-LEXED-AS-IF-IT-WERE-BARE. `"${x:-'a b'}"` drops the quotes bash keeps — 2026-08-04 — ✅ FIXED 2026-08-04

*(Was TD-OILS-SINGLE-QUOTES-IN-A-QUOTED-SUBSTITUTION-ARE-EATEN. Renamed
2026-08-04: measuring it turned up backslash as well as `'`, `:=` as well as
`:-`, and heredoc bodies as well as double quotes. The single quote was one
symptom of a lexer being run in the wrong mode.)*

**Where:** `userspace/oils/src/parser.rs` — the `${…}` operand is lexed the same
way wherever the substitution appears. It goes through `word_from_source`
(`tokenize`), which is the *bare-word* lexer: `'` opens a quoted stretch and a
backslash escapes whatever follows it. Inside a double-quoting context neither
is true, and the lexer that does know it is already there —
`dquote_word_from_source`/`crate::lexer::lex_dquote_body`, written for `PS4` and
`${x@P}`.

**Reproduce** (`v=set`, `x` unset; every line is inside `"…"`):

```sh
show() { printf '  %-14s(%d)' "$1" $(($# - 1)); shift; printf '<%s>' "$@"; printf '\n'; }
show 'sq'        "${nope:-'a b'}"   # bash: <'a b'>   osh: <a b>
show 'bs space'  "${nope:-a\ b}"    # bash: <a\ b>    osh: <a b>
show 'bs quote'  "${nope:-\'a\'}"   # bash: <\'a\'>   osh: <'a'>
show 'bs n'      "${nope:-a\nb}"    # bash: <a\nb>    osh: <anb>
show 'bs t'      "${nope:-\t}"      # bash: <\t>      osh: <t>
show 'assign'    "${x:='a b'}"      # bash: <'a b'> and x is 'a b';  osh: <a b>
cat <<EOF
  heredoc: [${nope:-'a b'}]         # bash: ['a b']   osh: [a b]
EOF
```

**The rule.** Within any double-quoting context — `"…"` and an unquoted heredoc
body alike — the *word* operand of `${x OP w}` is lexed with double-quote rules:

* `'` is an ordinary character, and so is anything between a pair of them.
* `\` escapes only `$`, `` ` ``, `"`, `\`, a newline — and `}`, which the brace
  scan strips so the operand can hold one. Before anything else the backslash
  **stays**: `\ ` is a backslash and a space, `\n` is a backslash and an `n`.
* `$'…'` and `$"…"` are still processed (`"${nope:-$'a\tb'}"` is a real tab),
  which is why this is the operand's own rule rather than plain double-quoting —
  inside real quotes `"$'a\tb'"` is literal.
* A nested `"` re-enters quoting as its own word, so `"${nope:-x"'y'"z}"` is
  `x'y'z`: the inner run quotes, and the `'` inside it is literal for the same
  reason.

The rule is the operand's, not the substitution's: the **pattern** operators
still remove quotes, and osh already agrees there — `"${v#'s'}"` strips a literal
`s` and `"${v/'e'/X}"` replaces a literal `e`, in both shells. `:=` is on the
operand side and so gets it wrong today in the assignment as well as the answer.

The four backslash cases osh already handles (`\$`, `` \` ``, `\"`, `\}`) are the
ones the brace scan happens to protect. That they pass is a coincidence of the
scan, not evidence the operand is lexed right.

**The fix.** A parser change, not an expansion one: the expander already does
the right thing with whatever parts it is handed, and `SplitMode::QuotedOperand`
already gives them the right context. What was missing was that the parse never
knew where the `${…}` was written.

* `parser.rs` gained a `Quoting` (`Bare` / `Dquote`) carried down through
  `seg_to_part` → `parse_braced_param_in` → the three operand sites. `Seg::Dq`
  hands `Dquote` down; a here-document body hands it down when its delimiter was
  *not* quoted; `dquote_word_from_source` (`PS4`, `${x@P}`) hands it down because
  that is what the entry point means. Everything else stays `Bare`, so the
  patterns, replacements, subscripts and slice bounds sitting beside the operand
  are untouched — which is what keeps `"${v#'s'}"` stripping a literal `s`.
* `lexer.rs`'s `read_word_verbatim` took a `Verbatim` mode (`Bare` /
  `Replacement` / `Dquote`) in place of its `repl_escapes: bool` — the third
  context needed a third answer, not a second flag. In `Dquote` the `'` arm is
  gone (a `'` falls through to the literal path) and the backslash consumes
  itself only before `$`, `` ` ``, `"`, `\`, `}` and a newline, emitting the
  character it protected as a one-char quoted segment so it is not read again.

The reason this could **not** just reuse `dquote_word_from_source` — the obvious
route, and the one this entry originally proposed — is `$'…'`/`$"…"`. A real
double-quoted body leaves them alone, but an operand expands them
(`"${nope:-$'a\tb'}"` is a real tab), because the operand is a *word* being read
and the quoting only says how its characters are spelled. Hence a third lexer
mode rather than a second caller of the second one.

Watch the interaction with TD-OILS-QUOTED-EMPTY-IN-AN-OPERAND-LEAVES-NO-FIELD:
that entry is about the *unquoted* `${nope:-'' ''}`, where the `''` really is a
quoted empty stretch. Inside quotes there is no such stretch to lose — the same
source is two literal apostrophes — so the two fixes do not overlap, and neither
one makes the other's case go away.

**Pinned by** `userspace/oils/tests/corpus/a-quoted-operands-quoting-is-the-quotes-it-sits-in.sh`.
