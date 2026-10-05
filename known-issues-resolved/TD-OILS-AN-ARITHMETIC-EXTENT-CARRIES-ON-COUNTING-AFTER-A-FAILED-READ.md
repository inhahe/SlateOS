### TD-OILS-AN-ARITHMETIC-EXTENT-CARRIES-ON-COUNTING-AFTER-A-FAILED-READ. `v='A$((1+$(fi⏎echo x⏎))X)B'; echo "${v@P}"` gives `[AB]` where bash gives `[AX)B]` and reports twice — 2026-08-09 — FIXED 2026-08-09

**Fixed in two steps.** Three of the four variants below went with commit
`a1a0518ee` (see TD-OILS-AN-EXPANSION-TIME-ARITHMETIC-SCAN-IGNORES-THE-COMMENT-RULE),
which re-derives the extent with a real port of the paren count. Variant 2 — the
one with a closer *missing* rather than a byte added — went with the follow-up
below, which gives the count the one shape it could not previously see.

**Where:** `userspace/oils/src/lexer.rs` (~4810-4850). After
`read_balanced('(', ')')` the lexer applies `is_arith_expr` and emits either
`Seg::Arith(expr, false, nested)` or `Seg::CmdSub(raw, line,
SubBody::ArithFallback(nested))` — the arith-vs-comsub decision `param_expand`
makes at expansion time, made at *lex* time instead. For variant 2 the parens do
not balance the way the lexer expects, so no `WordPart::ArithSub` was built,
`Shell::arith_extent_expand` never ran, and the text went straight to the
fallback body with no leftover.

**Reproduce:**

```text
a='A$((1+$(fi
echo x
))X)B';  printf '[%s]\n' "${a@P}"       # printf on line 4

  bash: command substitution: line 5: syntax error near unexpected token `fi'
        command substitution: line 5: `fi'
        command substitution: line 4: syntax error near unexpected token `fi'
        command substitution: line 4: `(1+$(fi'
        [AX)B]
  osh:  command substitution: line 4: syntax error near unexpected token `fi'
        command substitution: line 4: `(1+$(fi'
        [AB]
```

osh gives the *second* report and not the first: the fallback child runs, but
the nested read that should have reported before it never happened.

**Why (measured against bash 5.2.37, then read back in the source).** A `$((` is
extracted by `extract_delimited_string (string, sindex, "$(", "(", ")",
xflags|SX_COMMAND)` (subst.c:1284-1286), a *paren count*. The count recurses into
the nested `$(fi` through `xparse_dolparen` — the first report — and that read
now stops on the reader's line (see
TD-OILS-A-FAILED-EXTENT-READ-RUNS-TO-THE-END-OF-THE-STRING) rather than at the
end of the string, so `*sindex` comes back mid-string and **the count carries
on**. It meets two of the three `)`s and closes there, leaving `)B` for the
enclosing walk. The extent it hands back is `(1+$(fi⏎echo x⏎)`, and
`param_expand`'s `case LPAREN` then finds it does not end in `)` after the
opening one is stripped, so it is not arithmetic at all: bash falls through to
`command_substitute`, whose child parses `(1+$(fi⏎…` and reports it a second
time, blamed on the enclosing line and echoing that text's *first* line. That
second report is osh's `CmdSubBody::ArithFallback` shape.

**How many closers the resumed count wants, measured.** Four variants, all with
the same failed body, differing only in what follows it — the leftover says
exactly where the count stopped:

```text
A$((1+$(fi⏎echo x⏎)))B    -> [A)B]     consumed `…))`,  left `)B`
A$((1+$(fi⏎echo x⏎))X)B   -> [AX)B]    consumed `…))`,  left `X)B`
A$((1+$(fi⏎echo x⏎)Y))B   -> [A)B]     consumed `…)Y)`, left `)B`
A$((1+$(fi⏎echo x⏎Z)))B   -> [A)B]     consumed `…Z))`, left `)B`
```

osh's answers to the same four, re-measured after `a1a0518ee`:

```text
1  osh [A)B]   two reports, matches bash
2  osh [AB]    one report,  `(1+$(fi'     bash [AX)B]  two, `fi' then `(1+$(fi'
3  osh [A)B]   two reports, matches bash
4  osh [A)B]   two reports, matches bash
```

All four are now pinned by
`an-arithmetic-extent-read-parses-a-command-substitution-inside-it.sh` (1, 3 and
4 as probes 18-20; 2 as probes 21-23, the third of which is the same shape with
a body that *parses*, where the count still runs past the `))` and the extent
still ends `X`).

All four bash runs report the same two pairs and echo `` `(1+$(fi' `` for the second, and
**none prints `f: command not found`** — the failed read inside the count is
`SX_NOALLOC`, so it finds an extent and runs nothing. So the count resumes at
`stop + 1` and closes on the **second** closer it meets, which is the `$((`'s own
two levels: the `)` that would have closed the nested `$(` was never consumed,
so the three `)`s of a well-formed `$((1+$(…)))` now have one to spare.

The extent **excludes** its closer — `si = i - *sindex - len_closer + 1` and
`*sindex = i` (subst.c:1508-1518) — which is what routes all four to
`command_substitute`:

| case | extent `temp` | why not arithmetic |
|---|---|---|
| 1, 2, 4 | `(1+$(fi⏎echo x⏎)` | ends in `)`, so the `)` is cut and `chk_arithsub` sees `1+$(fi⏎echo x⏎` — one unmatched `(`, `count != 0` → 0 (subst.c:9487-9528) |
| 3 | `(1+$(fi⏎echo x⏎)Y` | `temp2[t_index] != RPAREN`, the earlier of the two `goto comsub`s (subst.c:10586-10591) |

Either way `command_substitute` is handed `temp` *with* its leading `(`, which
is why the second echo is `` `(1+$(fi' `` and not `` `1+$(fi' ``.

**Fix, and a hypothesis the measurement overturned.** This entry previously said
the remaining fix was to stop deciding arith-vs-comsub at lex time altogether —
"a real refactor: `Seg::Arith` and `Seg::CmdSub` collapse into one". A fresh
probe says otherwise. osh's *choice of construct* already matches bash in every
shape measured, including ones where the nested read succeeds
(`A$((1+$(echo q⏎echo x⏎))X)B` is `[AB]` with one report in both shells). The
lexer's verdict was never the divergence; the **extent** was. Where a recorded
hypothesis and a fresh measurement disagree, the measurement wins.

So the fix is the same one the `ArithSub` arm already had, given to the fallback
arm as well:

* `CmdSubBody::ArithFallback` gained a `tail` (`ast.rs`), filled by a third
  `attach_tails_by` pass in `unparse::attach_comsub_tails_in` — separate from the
  arithmetic's pass because `walk_parts` stops at the part it matched and an
  arithmetic can hold one of these.
* `Shell::arith_extent_expand`'s body became `Shell::arith_extent_route` →
  `ArithRoute::{Arith, Comsub, RanOut, Aborted}`: the count, then the two tests
  `param_expand` runs on the extent. Both callers want the same answer over a
  different string.
* `Shell::arith_fallback_expand` runs it over `src + ")" + tail` — the lexer's
  `src` is the text from the byte after the `$(` up to but not including the
  closer it matched, so putting that closer back reconstructs exactly the string
  `extract_command_subst` is handed. The count agreeing with the lexer is the
  ordinary case and delegates to `command_sub_body` unchanged; a count that stops
  earlier runs its own extent and expands the leftover, and a count that runs the
  string out reports as the arithmetic it was written as.
* `Shell::extent_read_of` runs the same count for a fallback met inside a `${ … }`
  scan, for the reason the arithmetic already did — bash's `$(` row does not know
  which of the two it is looking at. The shared half is
  `Shell::arith_scan_count`.
