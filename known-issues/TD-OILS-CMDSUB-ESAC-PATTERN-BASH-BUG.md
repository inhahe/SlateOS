### TD-OILS-CMDSUB-ESAC-PATTERN-BASH-BUG. `(esac)` as a pattern inside `$( … )` — deliberate divergence — 2026-07-30 — WONTFIX

**Where:** `userspace/oils/src/lexer.rs` — `CaseScan::finish_word`, whose
`CasePhase::Pattern` arm treats `esac` as reserved only where a pattern could
*start*, so the optional `(` makes it a pattern like any other word.

**Symptom.** That is the grammar, and it is what bash itself does outside a
substitution — but bash's own substitution scan gets it wrong, and returns text that
looks like leaked `declare -f` output:

```
$ bash -c 'case esac in (esac) echo E;; esac'
E
$ bash -c 'x=$(case esac in (esac) echo E;; esac); declare -p x'
declare -- x=$'\n        echo E\n    ;;\nesac)'
$ osh -c 'x=$(case esac in (esac) echo E;; esac); declare -p x'
declare -- x="E"
```

Note the control: `x=$(case b in (b) echo B;; esac)` gives `B` in both shells, so it
is `esac`-as-a-pattern specifically, and only inside a substitution. bash's two
answers for the same program contradict each other, so there is nothing coherent to
match; osh gives the answer bash gives everywhere else.

Without the optional `(` the two agree — `case esac in esac) …` is a syntax error in
both, and `tests/corpus/case-in-cmdsub.sh` pins that. This shape is kept *out* of the
corpus because the differ compares byte-for-byte.

**Proper fix.** None wanted. Revisit only if a future bash fixes its scan, at which
point this entry should turn into a corpus case.

**Impact.** `esac` used as a `case` pattern, with the optional open parenthesis,
inside a command or process substitution. Nothing else.
