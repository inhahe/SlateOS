### TD-OILS-DISCARD-LINENO-DRIFT. bash's `$LINENO` never recovers from a discard; osh deliberately keeps counting correctly — WONTFIX 2026-07-31 — ✅ **SUPERSEDED AND FIXED 2026-08-08**

**Where:** `userspace/oils/src/interp.rs` — `Shell::run_source_flow_out` /
`Shell::exec_program_top`, which abandon the current parse unit and read the
next one with `current_line` still tracking the real source line.

**What.** When a non-fatal word-expansion error discards a parse unit, bash
jumps to its read-parse-execute loop *without* advancing `line_number` over the
lines of the unit that never ran — and it never resynchronises afterwards, so
every subsequent line in that input is reported low by however many lines were
skipped, cumulatively:

```
$ printf '{ a[-9]=v\necho no; }\necho $LINENO\n' > f; bash f
f: line 1: a[-9]: bad array subscript
2                      # the `echo` is on line 3
```

osh reports 3. Every diagnostic after a discard therefore differs by the same
drift, which is why `tests/corpus/discard-scope.sh` strips line numbers with
`sed` and says so in its header comment.

**Proper fix.** ~~None wanted. Reproducing it would mean deliberately
mis-counting lines and would corrupt `$LINENO`, `caller`, `BASH_LINENO` and
every error message after the first discard in a script.~~

**✅ Superseded 2026-08-08 by
`TD-OILS-A-DISCARD-OUT-OF-A-COMPOUND-COMMAND-LOSES-BASH-A-LINE`, which measured
the drift properly and fixed it.** The WONTFIX reasoning above was wrong on its
premise: reproducing bash's counter does *not* mean corrupting the shell's own
idea of where it is. `Shell::line_bias` carries the drift as a separate quantity
and `current_line` is simply held biased — so `$LINENO`, `caller`, `BASH_LINENO`
and every diagnostic report bash's number, which for a byte-fidelity shell *is*
the right one. There was no tradeoff to decline.

The probe in this entry now agrees:

```
$ printf '{ a[-9]=v
echo no; }
echo $LINENO
' > f; osh f
f: line 1: a[-9]: bad array subscript
2
```

`tests/corpus/discard-scope.sh` lost the `sed` helper that had been stripping
line numbers out of its comparison for this reason; it now compares them.

**Impact.** None any more.
