### TD-OILS-AN-UNCLOSED-ARITH-SUBSTITUTION-IN-A-QUOTED-SUBSCRIPT-IS-NOT-CAUGHT-BEFORE-EXPANSION — 2026-08-14

**Where:** `crate::wordscan` (`userspace/oils/src/wordscan.rs`), the word-extent
pass `Shell::begin_word` runs before a word is expanded.

**Repro** (bash 5.2.37):

```sh
declare -a arr=(10 20 30)
echo "$(touch RAN)[${arr['x$(( 1+ ']}]"
```

bash prints ``bad substitution: no closing `)' in "$(touch RAN)[${arr['x$(( 1+ ']}]"``
— the **whole word** — and `RAN` is never created. osh prints
``bad substitution: no closing `)' in 'x$(( 1+ '`` — the fragment — and the
`touch` runs.

The side effect is the real defect; the name follows from it. bash reaches this
one on the *extent* pass, before any part of the word expands, so it names the
string that pass was walking. osh reaches it only when the subscript is expanded,
by which time the substitution ahead of it has already run.

**Not the same as the two entries above.** Those are about which string a fault
found *during* the fragment's expansion names. This one is about a fault bash
finds before expansion starts and osh does not find at all until later.

**What the proper fix looks like.** `wordscan::scan` has rows for `${`,
`` ` ``, `$(`, `$[` and `<(`/`>(`, and its faults are `WordFault::Brace` and
`WordFault::Backquote`. An unclosed `$((` inside a `' … '` in a subscript is a
third: `extract_delimited_string`'s (subst.c:1498), which names the scanned
string and closes it with `)`. Adding it means teaching `word_fault` a fault that
carries its own closing delimiter, and teaching the subscript skip that a `'` in
there does not hide a `$((` from the enclosing scan.

**Impact.** A command substitution written before such a subscript runs when bash
would not have run it. Narrow, but it is a side effect and not just text.
