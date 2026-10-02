### TD-OILS-A-SUBSTITUTION-INSIDE-A-BRACE-EXPANSION-IS-BLAMED-ON-THE-BODYS-OWN-LINE. `echo ${x:-$(fi)}` from a script names line 1 — 2026-08-07 — ✅ FIXED 2026-08-07

**Where:** `userspace/oils/src/parser.rs` — `Seg::ParamBraced` / the
`parse_braced_param_in` re-lex, and `map_segs`.

**What.** A `${ … }` body is kept as raw text and lexed again by
`parse_braced_param_in`. That second lex numbers its lines from 1, so a `$( … )`
inside it records a `close_line` in the *body's* coordinates — and nothing
renumbers it back, because `map_segs` can only reach the close lines that are
already `Seg`s. The error is then blamed on line 1 and, having no line in the
enclosing source to quote, loses its echoed source line as well:

```text
# a script whose `e() { ( eval "$1" ) 2>&1; … }` sits on line 4
e 'echo ${x:-$(fi)}'
bash: eval: line 4: syntax error near unexpected token `fi'
      eval: line 4: `echo ${x:-$(fi)}'
osh : eval: line 1: syntax error near unexpected token `fi'
      (no second line)
```

Pre-existing and independent of the arithmetic work: `echo ${x:-$(fi)}` shows it
with no `$((` anywhere. The same text at the top level of a `-c` string matches,
because there the two coordinate systems coincide. The arithmetic sibling
`echo $(( ${x:-$(fi)} ))` is *not* affected — an arithmetic scan does not treat
`${ … }` as opaque, so that `$(` is met by the enclosing scan and renumbered
with it (see
TD-OILS-A-NESTED-SUBSTITUTION-INSIDE-AN-ARITHMETIC-SCAN-IS-NOT-PARSED-WHEN-IT-CLOSES).

**Fixed.** `Seg::ParamBraced` now carries the line its `${` sits on, `map_segs`
renumbers it like every other recorded line, and that line is threaded down
through the whole `${ … }` parse as the physical line each *fragment* starts on
— `parse_braced_param_in` → `split_name_subscript` / `parse_slice_bounds` /
`parse_replace_pieces` / `parse_bulk_op` → the `*_from_source` family, each of
which now calls `map_frag_segs` on its freshly-lexed segments.

Per-fragment rather than one offset for the whole body, because a fragment is
not in general on the body's first line. Three carves can push one further down,
and each counts the newlines it steps over (`frag_line`): past a multi-line
subscript, past a slice's offset, and past a replacement's pattern. Everything
else is reached over operator characters only (`:`, `#`, `%`, `^`, `,`, `~`,
`/`, `-`, `=`, `+`, `?`), none of which is a newline, so it inherits its
parent's line unchanged.

Mapping the *segments* rather than rebasing the returned error is what makes the
runtime case work too: a segment carries the line a nested body is numbered
against, so ``${v/aaa/`fi`}`` — which parses fine and fails only when the
expansion runs — now reports `command substitution: line N` with the physical
`N`. An error-only rebase could never reach that.

`e 'echo ${x:-$(( 1 + $(fi) ))}'` is back in
`tests/corpus/arith-scan-parses-a-nested-substitution-in-place.sh`. Regression
test: `tests/corpus/brace-body-fragments-are-numbered-from-the-physical-line.sh`.
