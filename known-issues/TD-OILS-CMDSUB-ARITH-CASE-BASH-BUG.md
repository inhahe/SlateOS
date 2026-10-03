### TD-OILS-CMDSUB-ARITH-CASE-BASH-BUG. bash cannot read `$(( case … ) | cat)`; osh runs it — 2026-07-30 — WONTFIX

**Where:** nothing in osh. Recorded so the divergence is not mistaken for a
regression, and kept out of the corpus.

**Symptom.** bash's `$((` backtrack re-reads the body with its own extent scan,
which does not carry the `case` state the first reading would have needed:

```
$ bash -c 'x=$(( case b in b) echo B;; esac ) | cat); echo "x=[$x]"'
bash: -c: line 1: syntax error near unexpected token `)'
$ osh -c 'x=$(( case b in b) echo B;; esac ) | cat); echo "x=[$x]"'
x=[B]
```

The same body inside a *spaced* `$( ( … ) | cat)` is read correctly by bash, so
bash gives two different answers to the same program depending only on the space
— the same incoherence as `TD-OILS-CMDSUB-ESAC-PATTERN-BASH-BUG`. osh's `CaseScan`
runs during the substitution read whether or not a backtrack preceded it, so it
gets one coherent answer, which is the one bash gives for the spaced form.

**Proper fix.** None wanted. Revisit only if a future bash fixes its scan.

**Impact.** A `case` as the first element of a pipeline inside an un-spaced
`$(( … )`. Nothing else.
