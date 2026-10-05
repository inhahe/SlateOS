### TD-OILS-CMDSUB-HEREDOC-INSIDE-A-QUOTED-NESTED-SUBSTITUTION. a `$( … )` inside double quotes inside a `$( … )` never gets its here-document — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/lexer.rs` — `read_balanced_inner`'s `'"'` arm
(around line 3265), which copies from the opening double quote to the closing one
verbatim, honouring only `\` escapes.

**Symptom.** The here-document's body is never fetched, so the substitution reads
nothing and the body lines are left to run as commands:

```
$ cat t.sh
x=$( echo "$(cat <<B)" ); echo "x=[$x]"
bbb
B
$ bash t.sh
t.sh: line 1: warning: command substitution: 1 unterminated here-document
x=[bbb]
$ osh t.sh
t.sh: line 1: warning: command substitution: 1 unterminated here-document
t.sh: line 1: warning: here-document at line 1 delimited by end-of-file (wanted `B')
x=[]
t.sh: line 2: bbb: command not found
t.sh: line 3: B: command not found
```

**Cause.** The extent scan treats a double-quoted span as opaque, so it never
sees the `<<B` and never adds it to `pending`; nothing is gathered from past the
`)`. The re-lex of the outer body *does* parse the `$( … )` inside the quotes —
the lexer proper handles substitutions in double quotes — and its own
`gather_ahead` then looks for the body in the substitution's own text, which ends
right there, so it warns a second time and runs out.

bash has no such blind spot: `parse_matched_pair` recurses into a `$( … )` found
inside a double-quoted span with the same reader, so the `<<B` is recorded like
any other and gathered at the enclosing `)`.

A **backtick** span is not affected, because bash does not handle that case
either — `` x=$( echo `cat <<B` ) `` fails identically in both shells. Only
`"…"` (and, presumably, an unquoted here-document body inside the span) diverges.

**Proper fix.** Make the `'"'` arm scan rather than copy blindly: within the
quoted span, recognise `\`-escapes (already done), `` ` `` (copy whole, as the
outer scan already does), `${ … }` and `$( … )` / `$(( … ))`. The last is the
one that matters — it has to recurse the same way the unquoted case does, pushing
onto `nested` with a `pending` mark so that the fix above gathers it at its `)`.
That means factoring the `$(`-recognition out of the main loop so the quoted arm
can share it, rather than duplicating the depth bookkeeping.

**Impact.** Narrow but *observable*, unlike the ordering bug above: the value is
wrong and the body lines are executed. It needs a here-document whose `<<` and
whose `)` share a line, inside a substitution that is itself inside double quotes
inside another substitution — `"$(cat <<X)"` at top level is fine, since the
top-level lexer parses the quotes properly and only the *extent scan* is blind.

**Fixed 2026-08-06** by making the `'"'` arm scan instead of copy. It now
recognises, inside the quoted span, the same four things the main loop does
outside it: a `\` escape (already there), a backtick body (`read_backtick`), a
`${ … }` body (`read_dollar_brace`), and a `$( … )` — the last by *recursing*
into `read_balanced_inner` rather than by counting parens in the span, which is
what keeps a bare `)` in the text (`"a)b"`, `"${v:-)}"`, `"cost $5(x)"`) from
closing anything. `$(( … ))` is discriminated ahead of `$( … )` and read with
`heredocs = false`, since a `<<` in there is a left shift; the cursor is left on
the inner `(`, which the balanced read counts as a level of its own, so it stops
on the second `)`.

The recursion is what makes the nested gather work: the recursive call is an
ordinary `read_balanced_inner`, so it gathers its own here-documents at its own
`)` and warns there, exactly as the unquoted nesting does after the fix above.
No `nested`-stack bookkeeping was needed in the quoted span at all — that was the
plan's guess, and recursion is simpler and strictly more correct, because the
quoted span's parens are otherwise text.

The backtick asymmetry is deliberate and pinned: bash does *not* fetch a
here-document declared inside a backtick body, so `` x=$( echo "`cat <<B`" ) ``
leaves the body lines to run as commands in both shells.

**Verified:** `tests/corpus/a-nested-substitution-in-double-quotes-is-found-by-the-same-scan.sh`
(byte-for-byte, 14 shapes).
