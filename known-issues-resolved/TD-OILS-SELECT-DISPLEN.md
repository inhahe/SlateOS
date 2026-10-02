### TD-OILS-SELECT-DISPLEN. `select`'s menu measures items in characters, not display columns — 2026-07-27 — ✅ FIXED 2026-08-08

**Where:** `userspace/oils/src/interp.rs` — the free function `display_width`,
used by `select_menu` to compute the widest item (and hence the column stride)
and each element's length (and hence the gutter padding).

**What:** bash's `displen()` (`lib/sh/strtrans.c` → `execute_cmd.c`) decodes the
item to wide characters and returns `wcswidth()`, i.e. the number of *terminal
columns* it occupies; it falls back to the byte length only when the string will
not decode. osh returns `s.chars().count()`.

The two agree for every character whose width is 1 — which is all of ASCII, all
of Latin-1, and the overwhelming majority of what appears in a `select` menu.
They disagree in exactly two directions:

* **zero-width characters** (combining marks U+0300…, ZWJ/ZWNJ, most of
  `Mn`/`Me`/`Cf`): bash counts 0, osh counts 1, so osh over-estimates the item
  and pads one column too few after it;
* **double-width characters** (East Asian Wide/Fullwidth — CJK, Hangul,
  fullwidth forms, many emoji): bash counts 2, osh counts 1, so osh
  under-estimates and the columns after such an item are pushed right.

Neither can misalign a pure-ASCII menu, and neither affects which item a
selection maps to — only where the gutters land.

**Proper fix:** add a `char_display_width(c) -> usize` helper backed by a
Unicode East-Asian-Width + combining-class table (UAX #11 / UAX #44) and sum it
in `display_width`. That table is the actual cost here: osh deliberately carries
no Unicode data tables today (`${#var}` and `${var:off:len}` count characters
for the same reason — see TD-OILS-STRLEN-CHARS), so introducing one is a
crate-wide decision, not a `select`-local one. Doing it would also let the
`help`, `compgen` and `signal_list_columns` layouts stop counting characters.
*(As of the fix below osh does carry one, in `src/width.rs`. It is a width
table only: nothing else about character handling changed, and `${#var}` still
counts characters — which is right, and is not what `displen` measures.)*

**Repro (measured 2026-07-27):**
`COLUMNS=80 <sh> -c 'select o in 日本語 ab cd ef gh ij kl mn; do break; done'
</dev/null`. The item is 3 characters / 6 display columns / 9 UTF-8 bytes, so
the three answers give three different strides and the *shape* of the whole
menu changes, not just a gutter:

```
bash 5.2:                        osh:
1) 日本語  3) cd<TAB>    5) gh<TAB>  7) kl      1) 日本語
2) ab<TAB>      4) ef<TAB>    6) ij<TAB>  8) mn      2) ab
                                             … one item per line …
```

bash reaches `stride = 9+1+2+2 = 14` → 2 rows × 4 columns; osh reaches
`stride = 3+1+2+2 = 8` → the layout comes out one row tall and is transposed
into a single column. Note that *this host's* bash is in the C locale, so its
`displen` takes the byte-length fallback (9) rather than `wcswidth` (6) — the
two happen to agree on the final 2×4 layout here, but they are distinct
behaviours and a proper fix must implement `wcswidth`, not the byte length.
The differential corpus case `select-menu.sh` is deliberately all-ASCII so it
stays byte-exact; a non-ASCII case would have to be waived.

**Fixed 2026-08-08** in a new `userspace/oils/src/width.rs`: a `char_width`
backed by two generated range tables (368 zero-width ranges, 123 wide) and a
`display_width` that is `displen` line for line. The note above about the C
locale was **wrong on one point, and the measurement is what corrected it**:
the corpus pins `LC_ALL=C.UTF-8`, and under *that* locale this host's bash does
take the `wcswidth` path, not the byte-length one. So the case did not have to
be waived — `select-menus-items-are-measured-in-terminal-columns.sh` is
byte-exact.

Three things came out of measuring rather than assuming:

* `displen`'s two fallbacks are **not the same fallback**. A string that will
  not decode takes `slen = 0`, so `wcswidth` is called over no characters and
  returns 0 — the item measures *nothing*. Only a string that decodes but holds
  an unprintable character makes `wcswidth` return -1 and falls back to
  `STRLEN`. Measured: `$'\xff'` and `$'a\xffb'` measure 0, while `$'\x01'`
  followed by ten U+4E00 measures 31 — the byte length, not the 21 columns.
* Hangul Jamo medial vowels and final consonants (U+1160…U+11FF) are zero-width
  although they are `Lo`, not marks; no general-category rule catches them.
* The rule was checked against bash at **1701 range boundaries**, twice: once
  inferring the width bash used, once diffing osh's menu bytes against bash's.
  Both give the same 74 disagreements and every one of them is an *unassigned*
  code point — see the entry below.

The tooling is committed with it: `tests/probe_displen.py` recovers the width
bash used from a menu's column arithmetic (bash prints no width anywhere), and
`tests/gen_display_width.py` generates the tables and re-runs either check.

`width::display_width` has exactly one caller, and reading bash says that is
right. `displen` has exactly two call sites in all of bash
(`execute_cmd.c:3244` and `:3345`), both inside `print_select_list`. The only
*other* place bash measures columns is `help`, and it does that with a
`wcsnwidth`/`wcswidth` of its own in `wdispcolumn` (`builtins/help.def:411`),
to truncate each blurb to half the terminal width — while osh's `help`
deliberately prints one untruncated topic per line (TD-OILS-HELP-LAYOUT) and so
measures nothing at all. `signal_list_columns` pads with literal tabs rather
than to a column, and `compgen` prints one candidate per line; neither measures
a width either. So there is nothing else to repoint today.
