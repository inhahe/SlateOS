### TD-OILS-HISTEXPAND-WORD-SPLITTING. history word splitting did not use `history_word_delimiters`, so ranges over a line containing `(`/`;` differed — ✅ RESOLVED 2026-07-29

**Where:** `userspace/oils/src/histexpand.rs` — `words()`.

**What.** readline splits a history line into words with a tokenizer of its own,
`history_tokenize_word`, whose delimiter set (`history_word_delimiters` =
`" \t\n;&()|<>"`) holds the shell's operator characters as well as its
whitespace. osh split on whitespace alone, so for the event `arr=(x y z)` bash
numbered six words (`arr=` `(` `x` `y` `z` `)`) where osh numbered three
(`arr=(x` `y` `z)`) — every designator over such a line named the wrong text:

```
$ echo !a-b
bash → echo arr= ( x y zb     osh → echo arr=(x y z)b
```

**Fix (2026-07-29).** `words()` is now a port of `history_tokenize_word`, with
`tokenize_word()` for the operator/file-descriptor prefix and `scan_word()` for
readline's `get_word` tail. Every rule was **measured** against bash 5.2 (record
a line, read back `!!:0`, `!!:1`, … until the specifier goes bad) rather than
recalled from the source, which paid off repeatedly — the shape of the code is
readline's, but four of its behaviours are not what the source reads like:

* **`$(` skips the character after it.** readline steps past the two-character
  opener and *then* takes its own loop increment, so the byte right after `$(`
  is never examined. That is not cosmetic: it is exactly why `$((a;b))`
  tokenizes as `$((a;b)` + `)` (the inner `(` is stepped over, so it never
  nests) while `$(a (b;c) d)` is a single word (that `(` *is* seen, and does).
* **Process substitution outranks the operator reading of `<`/`>`, but only when
  lone.** `<(a;b)` and `2>(a;b)` are single words; `>>(a;b)`, `<<(a;b)` and
  `>&(a;b)` keep the operator and leave the `(` behind. `&(`, `;(` and `|(`
  split, since those are not substitution openers.
* **The file-descriptor pairs are exactly `>&`, `<&`, `&>`, `>|`** — and they
  absorb trailing digits and `-`, so `2>&1` and `>&-` are one word each and
  `>&12y` is `>&12` + `y`. The near neighbours `<>`, `<|`, `&<`, `&|`, `|&`,
  `;&` and `;|` do *not* group.
* **A third character joins a doubled `<` only.** `<<<` and `<<-` are
  three-character words; `>>-` is `>>` and then `-b`.

**`:x` had to be decoupled from `words()` in the same change**, because it was
implemented as "quote each word" and switching `words()` to the real tokenizer
would have made `:x` on `a;b` produce `'a' ';' 'b'`. bash gives `'a;b'`: `:x` is
readline's `quote_breaks`, which wraps the *whole* text in one pair of single
quotes and lifts each whitespace character individually back out of them. On
tidy input that is indistinguishable from a word split (`a b` → `'a' 'b'`), but
`a  b` → `'a' '' 'b'` and a tab stays a tab. Now `quote_breaks()`.

**Tests.** `tests/corpus/histexpand-words.sh` (34 probe lines, all using
`history -s` to record an event without running it and `history -p` to expand
without running the result, so the file has no side effects at all and can
safely cover shapes like `>>-b` and `${a;b}`), plus the unit tests
`histexpand::tests::words_are_readlines_history_tokens_not_a_whitespace_split`
and `quote_breaks_lifts_each_whitespace_character_out_of_the_quotes`.

**Note.** This entry previously also carried the leading-`-` arm of
`apply_word_designator()` (`!a-b` selecting words 0..last rather than
0..last-1). That turned out to be one symptom of a broader divergence in how
word-designator *ranges* and out-of-range errors work, so it is tracked on its
own as TD-OILS-HISTEXPAND-WORD-RANGE below and is **not** fixed here.
