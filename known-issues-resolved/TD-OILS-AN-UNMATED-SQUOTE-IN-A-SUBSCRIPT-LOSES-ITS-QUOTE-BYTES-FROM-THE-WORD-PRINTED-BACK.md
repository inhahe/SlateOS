### TD-OILS-AN-UNMATED-SQUOTE-IN-A-SUBSCRIPT-LOSES-ITS-QUOTE-BYTES-FROM-THE-WORD-PRINTED-BACK — 2026-08-14 — ✅ FIXED 2026-08-14

**Where:** `attach_subscript_reads` (`userspace/oils/src/parser.rs`), which gives
each top-level `' … '` of an arithmetic fragment its interior parse.

**Repro** (bash 5.2.37):

```sh
declare -a arr=(10 20 30)
declare -A m=([k]=V)
echo "[${arr['x${m:-']}]"
```

bash names `` 'x${m:-' `` — the whole fragment, quotes included. osh named
`x${m:-` — the interior of the run alone.

**Cause, which was not the one first written here.** The first note guessed the
text came from `crate::unparse::word_src` by way of `crate::wordscan::word_fault`.
It does not: `word_fault` returns `None` for these words, and the word source osh
builds is byte-correct. The diagnostic comes from `Shell::expand_unclosed` on an
`Unclosed::BadSubst` whose `text` the *interior's own lexer* filled in with
`Lexer::whole_text` — the interior being a string of osh's making. bash has no
such string: an arithmetic fragment is expanded with `Q_DOUBLE_QUOTES` set, which
switches the single quote off, so `expand_word_internal` walks straight through
the pair and the string it was handed is the fragment. Both "no closing"
reporters echo that string (`report_error (…, string)`, subst.c:1498 for
`$[ … ]`, subst.c:1972 for `${ … }`).

That also explains the shape the note found puzzling — a name that begins one
byte late and ends two bytes early is exactly the interior of a `' … '` run.
There were not two faults there, but there is a second one beside it; see
`TD-OILS-A-BRACE-WHOSE-NAME-SCAN-RUNS-OFF-A-FRAGMENT-TAKES-THE-OTHER-DIAGNOSTIC`.

**Fix.** `attach_subscript_reads` already re-measures the fragment after parsing
an interior — that is what `crate::unparse::attach_comsub_tails` does for a
`$( … )`'s echoed remainder. It now also re-*names*: a new
`name_unclosed_after_the_fragment` walks the interiors it just attached and gives
every top-level `WordPart::Unclosed(Unclosed::BadSubst { text, .. })` the
fragment's source for its `text`. Only the run's own level is renamed; a `" … "`
inside the interior is carved out by `string_extract_double_quoted` as its own
string and keeps naming itself, as one written a character to the left of the `'`
would. `src` is left alone — it is the construct's spelling for a re-print, not a
diagnostic's `%s`.

Corpus:
`a-construct-left-open-in-a-quoted-subscript-names-the-fragment-around-it.sh`
(seven rows: a `${ … }` body scan running off, the same with text after the run,
a `$[ … ]`, both substring bounds, and a run that closes nothing early).
