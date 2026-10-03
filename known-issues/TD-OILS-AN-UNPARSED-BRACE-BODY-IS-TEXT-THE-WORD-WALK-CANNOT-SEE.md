### TD-OILS-AN-UNPARSED-BRACE-BODY-IS-TEXT-THE-WORD-WALK-CANNOT-SEE. `${x!$(( #5 ))}` gets the "bad substitution" wording where bash gives "no closing `)'" — 2026-08-04 — OPEN (accepted deviation)

**Where:** `userspace/oils/src/ast.rs` — `WordPart::first_scanned_arith`, the
`BadSubst` arm.

**What:** `WordPart::BadSubst` holds the *unparsed* text between `${` and `}`,
so a `$((` inside it is characters rather than a node and the word walk has
nothing to find. bash scans that text and reports the eaten closer in
preference to the malformed expansion:

```text
$ echo "${x!$(( #5 ))}"
bash: bad substitution: no closing `)' in "${x!$(( #5 ))}"
osh : ${x!$(( #5 ))}: bad substitution
```

Both are DISCARD-class with status 1 on a word that is malformed either way;
only the wording and the named text differ.

**Why accepted:** finding it means re-scanning raw source for `$((`, and a raw
scan *can* false-positive — putting a spurious "no closing `)'" on a word that
is perfectly valid. That is a regression in the common path traded for a
corner. The structural walk cannot false-positive: it only ever looks at bodies
the parser already delimited.

Worth recording what is *not* affected, since the obvious guesses are wrong and
were measured before being believed. Inside a `${ … }` bash scans raw text and
so ignores quoting — `${x:-'$(( #5 ))'}` and `${x:-<(echo $(( #5 )))}` both
report in bash — and osh agrees on both, because its parser does not build a
`SingleQuoted` or `ProcSub` node there either. Outside braces quoting protects
in both shells (`echo '$(( #5 ))'` and `echo "${x:-\$(( #5 ))}"` are quiet).
`BadSubst` is the single remaining case.

**Trigger for revisiting:** a scanner shared with the parser, which would make
the raw scan exact rather than a guess and so remove the false-positive risk
that is the whole reason to decline.
