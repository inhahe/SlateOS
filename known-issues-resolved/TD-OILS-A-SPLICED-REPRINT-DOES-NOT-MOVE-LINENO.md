### TD-OILS-A-SPLICED-REPRINT-DOES-NOT-MOVE-LINENO. `$LINENO` after a multi-line re-print reads the line the substitution was written on — 2026-08-07 — ✅ FIXED 2026-08-07

**Where:** `userspace/oils/src/unparse.rs` (`comsub_reprint`) feeds text back into
`userspace/oils/src/parser.rs`; the line map is built by
[`crate::lexer::Lexer::stamp_lines`].

**What.** bash re-prints a `$( … )` body at parse time and splices the result into
the text it goes on to read, so the re-print's newlines are *real* — they push
everything after them down. Now that osh's re-print produces the same multi-line
text as bash (see the entry above), the text agrees but the line numbering does
not: osh stamps the whole spliced region with the line the substitution started
on.

**Repro:**

```sh
r=$(if true; then echo B; fi
echo L2=$LINENO)
#   bash: L2=7    osh: L2=5     (the `if` re-prints over three lines, so the
#                                second statement of the body sits two lower)
```

The older probe in the entry above shows the same thing through an arithmetic
substitution:

```sh
q=$(echo $(( 0 + $(if true; then echo 0; fi) )); echo L=$LINENO)
#   written on line 19            bash: L=21          osh: L=19
```

**Proper fix — and the diagnosis above is wrong about what it is.** There is no
line map to rebuild, because there is no splice: the whole re-print *is* the
body. `parse_comsub` (parse.y:4219–4233) keeps `print_comsub`'s text and calls
`dispose_command` on the parse, so the bytes `command_substitute` reads back at
expansion time are the deparse, start to finish. Number them from `close_line`
like any other lazily-read body and everything follows.

**Fixed 2026-08-07 by deleting the rule that stood in for the re-print.** osh
had re-read the *source* of the body and compensated with a second line-mapping
rule, `LineMap::CmdSub`: reported line = `close_line` + the 0-based **rank** of
the body line among the body lines that carry a command. That rank rule was
measured (11 probes) and it is right — but only as a *shadow* of the re-print,
which is what it was unknowingly describing. The re-print has no blank lines and
no comments and joins nothing, so for a body of simple commands "rank" and "line
of the re-print" are the same number. They part company the moment the re-print
changes a body's *length*, which only a compound command does:

```text
                              source line   re-print line
  if true; then echo B; fi         1              1-3
  echo L=$LINENO                   2              4
```

So the fix is to stop approximating: `CmdSubBody::Parsed::src` is now
`unparse::comsub_body(&prog)` — the re-print — and its lines are a plain
`LineMap::Offset(close_line - 1)`, the same offset a backtick body already used.
`LineMap` is down to that one variant; `build_cmdsub_line_map`, the `ranked`
table, and the `pre` field it needed for re-lexed tails are gone, and `map`,
`shifted` and `unmap` are one line each.

The eager parse stays, because bash's does too until the moment it is disposed:
it is what finds the `)`, what makes `$(for)` a fatal syntax error where
`` `for` `` is silent, what `declare -f` re-prints, and what answers the
`$(< file)` peek. What changed is that it is now also the *source* of the body
text rather than being discarded next to it.

Note what this means: the text a substitution *runs* is the deparse. That is
bash's own behaviour, and it raises the stakes on the deparser — a round-trip bug
that used to show only as a cosmetic `declare -f` difference now changes what
executes. Which is the right way round: it makes deparser bugs loud.

**Verified** on the two repros above, on blank-line/comment/multiple-command-per-
line bodies (where the old rank rule was right and the new model has to agree),
on the backtick control (verbatim, so blank lines still count), and on 20
substitution shapes covering aliases, here-documents, `$'…'`, loops, `case`,
redirections, pipelines, `!`, arrays, `coproc` and nesting. Plus 1360 unit tests
and the full corpus sweep. Corpus case:
`tests/corpus/a-substitution-runs-its-reprint-not-its-source.sh`.

**Found by** the verification pass for
TD-OILS-A-REPRINTED-COMPOUND-COMMAND-IS-KEPT-ON-ONE-LINE, 2026-08-07.
