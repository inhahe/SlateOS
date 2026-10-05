### [B] TD-OILS-A-LESS-THAN-IN-A-BRACE-ARITHMETIC-FRAGMENT-LOSES-ITS-LEFT-OPERAND

**Status:** ✅ FIXED 2026-08-14. Found 2026-08-14, measured against bash 5.2.37.
The cause turned out to be wider than the title: the two bounds were
**tokenized as a command** rather than read as arithmetic, so `<` was only the
most visible of the operators being lost. See "The fix" at the end.

A `<` in the offset or length of `${z:o:l}` swallows everything to its left.
The same expression inside a plain `$(( ... ))` is fine, so this is the brace
fragment's own reading of the text, not the arithmetic evaluator's:

| written | bash | osh |
|---|---|---|
| `z=abcdef; echo "${z:1<(2)}"` | `bcdef` | `z: <(2): syntax error: operand expected` |
| `z=abcdef; echo "${z:0:1<(2)}"` | `a` | same error |
| `echo $(( 1<(2) ))` | `1` | `1` |

bash reads `1<(2)` as `1 < (2)`, which is `1`, so the offset is 1. osh
evaluates `<(2)` alone -- the `1` is gone by the time the evaluator sees the
expression, which is what the quoted error token shows.

**Where:** `userspace/oils/src/lexer.rs`, the `Verbatim::Arith` path of
[`Lexer::read_word_verbatim`], and whatever splits a `${z:o:l}` body into its
two fragments in `userspace/oils/src/parser.rs`. The `<` is being taken for
something other than a comparison operator -- most likely a fragment boundary.

**Proper fix:** treat `<` in an arithmetic fragment as the comparison operator
it is, so the whole fragment reaches the evaluator. A `<(` there is *not* a
process substitution to be performed either -- measured, `${z:0:<(echo 1)}` is
an `operand expected` in bash with the characters `<(echo 1)` standing as the
error token, which osh already matches.

**Blocked, and then unblocked (same day):** the arithmetic-fragment row of
TD-OILS-A-PROCESS-SUBSTITUTION-A-SECOND-SCAN-FINDS-IN-A-BRACE-BODY-IS-NOT-PARSED-AGAIN.
bash's `${ ... }` scan reads a `<( ... )` in an arithmetic fragment exactly as it
reads one anywhere else in the body -- `x='A${z:0:<(fi)}B'; echo "${x@P}"`
reports the parse twice and then `bad substitution`, where osh printed `AB` --
but a corpus row for it would have been measuring this bug instead, so the
corpus case
`a-process-substitution-a-brace-re-read-meets-is-read-wherever-in-the-braces-it-sits.sh`
left that position out and said so. The fix below removed the obstacle, and the
rows went in the same day: that case now measures a bound in seven further
positions.

**How it was found:** measuring where bash's brace scan reads a `<( ... )`,
while checking whether the `Verbatim::Arith` fragments needed the same row as
the pattern and replacement ones.

**The fix (2026-08-14).** `parse_slice_bounds`
(`userspace/oils/src/parser.rs`) read each bound with `word_from_source`, which
called `tokenize(...)` — a *command* tokenizer — and then joined the surviving
`Tok::Word`s with a literal space. So every operator character was claimed by
the tokenizer instead of reaching the evaluator, and whatever it could not make
a word of was silently dropped. `<` was merely the case that produced an IO
number and a redirect. The rest, all measured against bash 5.2.37 with
`z=abcdef`:

| written | bash | osh, tokenized |
|---|---|---|
| `${z:1<2}` | `bcdef` | `cdef` — `1<` taken for a redirect |
| `${z:1>2}` | `abcdef` | `cdef` — likewise |
| `${z:1<=2}` | `bcdef` | `=2: operand expected` |
| `${z:1 < (2)}` | `bcdef` | `1 2: syntax error` |
| `${z:1;2}` | `;2: invalid arithmetic operator` | `1 2: syntax error` |
| `${z:1&2}` | `abcdef` | `1 2: syntax error` |
| `${z:3|2}` | `def` | `3 2: syntax error` |
| `${z:1&&2}` | `bcdef` | `1 2: syntax error` |
| `${z:1)}` | `1): syntax error in expression` | silently `abcdef` |

Both bounds now go through `word_subscript_from_source_at` — the very reader an
array subscript uses, which is `verbatim_word_at(..., Frag::Arith)` plus
`attach_subscript_reads`. The two arithmetic fragments therefore no longer
disagree with each other, which is what `attach_subscript_reads`'s own doc
comment had been asking for.

Two further defects of the same splitter were found while measuring it, and are
fixed in the same change:

* **Which colon cuts.** bash does not `strchr` for the `:`; `skiparith`
  (subst.c) skips one `:` for every `?` seen, and counts nothing at all inside
  a `( … )`. `${z:1?2:3}` is `cdef` (the whole text is the offset) while
  `${z:1?2:3:1}` is `c`; `${z:1?1?2:3:4}` is `cdef`, two `?` swallowing both
  colons; `${z:(1?2:3):1}` is `c`. osh split on the first `:` unconditionally
  and so reported `` `:' expected for conditional expression `` for all of
  these. Now `slice_split_colon` implements the rule.
* **An empty bounds text.** `${z:}` is `${z:}: bad substitution` in bash, and
  uniformly so — `${@:}`, `${*:}`, `${a[@]:}`, `${a[1]:}` and an unset
  parameter all report it. osh printed the whole value. It is the *text* that
  must be non-empty, not what it expands to: `${z:$e}` with `e=` is `abcdef`.
  `parse_slice_bounds` now returns `None` for an empty text and each of its
  three call sites turns that into `WordPart::BadSubst`.

Verified by the corpus case
`a-slice-cuts-its-bounds-with-skiparith-and-reads-each-as-arithmetic.sh`
(75 rows, IDENTICAL), the lib suite and a full sweep.

**Unblocked, and then done (same day):** the arithmetic-fragment row named
under "Blocks" above was the only thing left of
TD-OILS-A-PROCESS-SUBSTITUTION-A-SECOND-SCAN-FINDS-IN-A-BRACE-BODY-IS-NOT-PARSED-AGAIN,
and it is now closed there. It was a separate row from this entry's — after
this fix `${z:1<(2)}` evaluated correctly but `x='A${z:0:<(fi)}B'; echo
"${x@P}"` still printed `AB`, where bash reads the body for its extent and
reports `bad substitution`. It turned out **not** to be the `Verbatim::Arith`
row this entry's title suggested, because the *subscript* shares that mode and
must not get it: bash's `${ … }` scan steps over a subscript whole
(`skip_matched_pair`), so `${z[<(fi)]}` never offers its body to the scan and
is an `operand expected` in bash — which osh already matched. Only a bound is
walked in the open, so `Frag::Arith` split in two and the new `Frag::Bound`
took the row. See that entry for the change.
